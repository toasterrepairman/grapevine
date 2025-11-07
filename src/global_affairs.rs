use gtk::prelude::*;
use gtk::{glib, Label, Orientation, ScrolledWindow, ListBox, SearchEntry, Popover};
use libshumate::prelude::{MarkerExt, LocationExt};
use std::collections::HashMap;
use std::cell::RefCell;
use std::rc::Rc;
use chrono::NaiveDateTime;

use crate::data::{GdeltArticle, GdeltResponse, CurrencyInfo, GDELT_API_URL};
use crate::coordinates::{get_country_coordinates, get_country_currency, get_country_timezone};

pub fn create_global_affairs_view(
    current_query: Rc<RefCell<String>>,
    results_list_ref: Rc<RefCell<Option<ListBox>>>,
    marker_layer_ref: Rc<RefCell<Option<libshumate::MarkerLayer>>>,
    use_12_hour: Rc<RefCell<bool>>,
) -> gtk::Box {
    // Create a responsive container that switches orientation based on window size
    let container = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .build();

    // Create the scrollbox for additional content
    let scrolled_window = ScrolledWindow::builder()
        .vexpand(false)
        .hexpand(true)
        .build();

    let scrollbox_content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    // Create search entry for GDELT queries
    let search_entry = SearchEntry::builder()
        .placeholder_text("Search GDELT news...")
        .build();

    // Create a list box for search results
    let results_list = ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    results_list.add_css_class("boxed-list");

    // Store results_list in the shared reference
    *results_list_ref.borrow_mut() = Some(results_list.clone());

    scrollbox_content.append(&search_entry);
    scrollbox_content.append(&results_list);
    scrolled_window.set_child(Some(&scrollbox_content));

    // Create the map widget using libshumate
    let map = libshumate::SimpleMap::new();

    // Create a custom dark-themed map source using CartoDB Dark Matter tiles
    let map_source = libshumate::RasterRenderer::from_url(
        "https://a.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}.png"
    );

    map.set_map_source(Some(&map_source));

    // Get the viewport to create the marker layer
    let marker_layer_opt = if let Some(map_view) = map.map() {
        if let Some(viewport) = map_view.viewport() {
            // Create a marker layer for country markers
            let marker_layer = libshumate::MarkerLayer::new(&viewport);

            // Add the marker layer to the map
            map_view.add_layer(&marker_layer);

            // Set zoom level constraints on the viewport to prevent excessive wrapping
            // Min zoom 1 (whole world visible), max zoom 10 (reasonable detail)
            viewport.set_min_zoom_level(1);
            viewport.set_max_zoom_level(6);

            // Set initial zoom level to 2 (good overview of world)
            map_view.go_to_full(0.0, 0.0, 2.0);

            Some(marker_layer)
        } else {
            None
        }
    } else {
        None
    };

    // Store marker layer in the shared reference
    *marker_layer_ref.borrow_mut() = marker_layer_opt.clone();

    // Make the map expand to fill the space
    map.set_vexpand(true);
    map.set_hexpand(true);

    // Clone marker layer for use in async callback
    let marker_layer_clone = marker_layer_opt.clone();
    let results_list_clone = results_list.clone();
    let use_12_hour_clone = use_12_hour.clone();

    // Perform initial search with empty query to get latest news
    glib::spawn_future_local(async move {
        fetch_gdelt_articles("", results_list_clone, marker_layer_clone, use_12_hour_clone).await;
    });

    // Set up automatic refresh every 15 minutes
    let current_query_for_refresh = current_query.clone();
    let results_list_for_refresh = results_list.clone();
    let marker_layer_for_refresh = marker_layer_opt.clone();
    let use_12_hour_for_refresh = use_12_hour.clone();
    glib::timeout_add_seconds_local(15 * 60, move || {
        let query = current_query_for_refresh.borrow().clone();
        let results_list = results_list_for_refresh.clone();
        let marker_layer = marker_layer_for_refresh.clone();
        let use_12_hour = use_12_hour_for_refresh.clone();

        glib::spawn_future_local(async move {
            fetch_gdelt_articles(&query, results_list, marker_layer, use_12_hour).await;
        });

        glib::ControlFlow::Continue
    });

    // Set up search entry activation
    let results_list_for_search = results_list.clone();
    let marker_layer_for_search = marker_layer_opt.clone();
    let current_query_for_search = current_query.clone();
    let use_12_hour_for_search = use_12_hour.clone();
    search_entry.connect_activate(move |entry| {
        let query = entry.text().to_string();

        // Update the current query
        *current_query_for_search.borrow_mut() = query.clone();

        let results_list = results_list_for_search.clone();
        let marker_layer = marker_layer_for_search.clone();
        let use_12_hour = use_12_hour_for_search.clone();

        glib::spawn_future_local(async move {
            fetch_gdelt_articles(&query, results_list, marker_layer, use_12_hour).await;
        });
    });

    // Create an orientable paned widget for responsive layout
    let paned = gtk::Paned::builder()
        .orientation(Orientation::Vertical)
        .wide_handle(true)
        .build();

    // Set the scrollbox as the first child (top in vertical, left in horizontal)
    paned.set_start_child(Some(&scrolled_window));
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(false);

    // Set the map as the second child (bottom in vertical, right in horizontal)
    paned.set_end_child(Some(&map));
    paned.set_resize_end_child(true);
    paned.set_shrink_end_child(false);

    // Set initial position (200px for scrollbox in vertical mode)
    paned.set_position(200);

    // Add a tick callback to change orientation based on window size
    let paned_weak = paned.downgrade();
    paned.add_tick_callback(move |_widget, _clock| {
        if let Some(paned) = paned_weak.upgrade() {
            let width = paned.width();
            let height = paned.height();

            if width > 0 && height > 0 {
                let should_be_horizontal = width > height;
                let is_horizontal = paned.orientation() == Orientation::Horizontal;

                if should_be_horizontal != is_horizontal {
                    if should_be_horizontal {
                        paned.set_orientation(Orientation::Horizontal);
                        paned.set_position(width - 500); // 250px from right for scrollbox
                    } else {
                        paned.set_orientation(Orientation::Vertical);
                        paned.set_position(200); // 200px from top for scrollbox
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });

    container.append(&paned);
    container
}

async fn fetch_gdelt_articles(query: &str, results_list: ListBox, marker_layer: Option<libshumate::MarkerLayer>, use_12_hour: Rc<RefCell<bool>>) {
    // Clear existing results
    while let Some(child) = results_list.first_child() {
        results_list.remove(&child);
    }

    // Create a shared map to store marker buttons by country code
    let marker_buttons_map: Rc<RefCell<HashMap<String, gtk::Button>>> = Rc::new(RefCell::new(HashMap::new()));

    // Clear existing markers if marker layer is provided
    if let Some(ref layer) = marker_layer {
        layer.remove_all();
        marker_buttons_map.borrow_mut().clear();
    }

    // Show loading indicator
    let loading_row = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    let loading_label = Label::builder()
        .label("Loading...")
        .build();
    loading_row.append(&loading_label);
    results_list.append(&loading_row);

    // Build the API URL with English language filter
    // Use timespan=2h to get only the most recent articles
    let url = if query.is_empty() {
        // For empty queries, use "world" as default query to get broader news coverage
        format!(
            "{}?query=world sourcelang:english&mode=artlist&maxrecords=50&timespan=2h&format=json",
            GDELT_API_URL
        )
    } else {
        format!(
            "{}?query={} sourcelang:english&mode=artlist&maxrecords=50&timespan=2h&format=json",
            GDELT_API_URL,
            urlencoding::encode(query)
        )
    };

    eprintln!("Fetching from URL: {}", url);

    // Fetch data from GDELT API
    match reqwest::get(&url).await {
        Ok(response) => {
            // Get the raw text first to help debug
            match response.text().await {
                Ok(text) => {
                    eprintln!("Response text (first 500 chars): {}", &text.chars().take(500).collect::<String>());

                    // Check if response is empty or null
                    if text.trim().is_empty() || text.trim() == "null" {
                        // Clear all children (including loading indicator)
                        while let Some(child) = results_list.first_child() {
                            results_list.remove(&child);
                        }
                        let no_results = Label::builder()
                            .label("No articles found for this search")
                            .margin_top(12)
                            .margin_bottom(12)
                            .build();
                        results_list.append(&no_results);
                        return;
                    }

                    // Try to parse the JSON
                    match serde_json::from_str::<GdeltResponse>(&text) {
                        Ok(data) => {
                            process_gdelt_articles(data, results_list, marker_layer, marker_buttons_map, use_12_hour.clone());
                        }
                        Err(e) => {
                            // Try parsing as a direct array of articles
                            match serde_json::from_str::<Vec<GdeltArticle>>(&text) {
                                Ok(articles) => {
                                    let data = GdeltResponse { articles };
                                    process_gdelt_articles(data, results_list, marker_layer, marker_buttons_map, use_12_hour.clone());
                                }
                                Err(_) => {
                                    // Clear all children (including loading indicator)
                                    while let Some(child) = results_list.first_child() {
                                        results_list.remove(&child);
                                    }
                                    eprintln!("JSON parse error: {}", e);
                                    eprintln!("Response preview: {}", &text.chars().take(200).collect::<String>());
                                    let error_label = Label::builder()
                                        .label("Error: Could not parse news feed. The API may be unavailable or returned unexpected data.")
                                        .wrap(true)
                                        .margin_top(12)
                                        .margin_bottom(12)
                                        .build();
                                    results_list.append(&error_label);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    // Clear all children (including loading indicator)
                    while let Some(child) = results_list.first_child() {
                        results_list.remove(&child);
                    }
                    eprintln!("Error reading response text: {}", e);
                    let error_label = Label::builder()
                        .label(&format!("Error reading response: {}", e))
                        .margin_top(12)
                        .margin_bottom(12)
                        .build();
                    results_list.append(&error_label);
                }
            }
        }
        Err(e) => {
            // Clear all children (including loading indicator)
            while let Some(child) = results_list.first_child() {
                results_list.remove(&child);
            }
            eprintln!("Error fetching articles: {}", e);
            let error_label = Label::builder()
                .label(&format!("Error fetching articles: {}", e))
                .margin_top(12)
                .margin_bottom(12)
                .build();
            results_list.append(&error_label);
        }
    }
}

fn process_gdelt_articles(
    data: GdeltResponse,
    results_list: ListBox,
    marker_layer: Option<libshumate::MarkerLayer>,
    marker_buttons_map: Rc<RefCell<HashMap<String, gtk::Button>>>,
    use_12_hour: Rc<RefCell<bool>>,
) {
    // Clear all children (including loading indicator)
    while let Some(child) = results_list.first_child() {
        results_list.remove(&child);
    }

    if data.articles.is_empty() {
        let no_results = Label::builder()
            .label("No articles found")
            .margin_top(12)
            .margin_bottom(12)
            .build();
        results_list.append(&no_results);
    } else {
        // Sort articles by seendate (most recent first)
        let mut sorted_articles = data.articles.clone();
        sorted_articles.sort_by(|a, b| b.seendate.cmp(&a.seendate));

        // Deduplicate by domain - limit to 3 articles per domain
        let mut domain_counts: HashMap<String, usize> = HashMap::new();
        let max_per_domain = 3;

        for article in sorted_articles.iter() {
            let count = domain_counts.entry(article.domain.clone()).or_insert(0);
            if *count < max_per_domain {
                let marker_data = if marker_layer.is_some() {
                    Some((marker_buttons_map.clone(), marker_layer.clone().unwrap()))
                } else {
                    None
                };
                let article_row = create_article_row_with_markers(article, marker_data);
                results_list.append(&article_row);
                *count += 1;
            }
        }

        // Group articles by country and place markers on the map
        if let Some(ref layer) = marker_layer {
            let mut articles_by_country: HashMap<String, Vec<GdeltArticle>> = HashMap::new();

            // Group ALL articles by country (not just unique ones)
            for article in data.articles.iter() {
                if !article.sourcecountry.is_empty() {
                    articles_by_country
                        .entry(article.sourcecountry.clone())
                        .or_insert_with(Vec::new)
                        .push(article.clone());
                }
            }

            eprintln!("Found {} countries with articles", articles_by_country.len());

            // Create markers for each country
            for (country_code, articles) in articles_by_country.iter() {
                if let Some((lat, lon)) = get_country_coordinates(country_code) {
                    eprintln!("Creating marker for {} with {} articles at ({}, {})",
                             country_code, articles.len(), lat, lon);
                    create_country_marker(layer, country_code, lat, lon, articles, marker_buttons_map.clone(), use_12_hour.clone());
                } else {
                    eprintln!("No coordinates found for country code: {}", country_code);
                }
            }
        }
    }
}

/// Create a compact, modern article widget with vertical layout
/// Optimized for narrow screens with uniform design
fn create_article_row_with_markers(
    article: &GdeltArticle,
    country_marker_data: Option<(Rc<RefCell<HashMap<String, gtk::Button>>>, libshumate::MarkerLayer)>
) -> gtk::Box {
    // Main card container - vertical layout
    let card = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(6)
        .margin_end(6)
        .build();
    card.add_css_class("news-article-card");

    // Image header (if available)
    if !article.socialimage.is_empty() {
        let picture = gtk::Picture::builder()
            .height_request(140)
            .width_request(0)
            .hexpand(true)
            .can_shrink(true)
            .content_fit(gtk::ContentFit::Cover)
            .visible(false)
            .build();
        picture.add_css_class("article-thumbnail");

        card.append(&picture);

        // Load image from URL asynchronously
        let url = article.socialimage.clone();
        let picture_clone = picture.clone();
        glib::spawn_future_local(async move {
            match reqwest::get(&url).await {
                Ok(response) => {
                    if let Ok(bytes) = response.bytes().await {
                        let bytes_vec = bytes.to_vec();
                        let bytes = glib::Bytes::from(&bytes_vec);
                        if let Ok(texture) = gtk::gdk::Texture::from_bytes(&bytes) {
                            picture_clone.set_paintable(Some(&texture));
                            picture_clone.set_visible(true);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to load image: {}", e);
                }
            }
        });
    }

    // Content container with padding
    let content_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(10)
        .margin_end(10)
        .build();

    // Title
    let title_label = Label::builder()
        .label(&article.title)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Word)
        .xalign(0.0)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    title_label.add_css_class("article-title");
    content_box.append(&title_label);

    // Metadata badges row
    let badges_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .build();

    // Country badge (clickable)
    if !article.sourcecountry.is_empty() {
        let country_button = gtk::Button::builder()
            .label(&article.sourcecountry)
            .build();
        country_button.add_css_class("badge");
        country_button.add_css_class("badge-country");

        // If we have marker data, make the button click the corresponding map marker
        if let Some((marker_buttons_map, _)) = country_marker_data.clone() {
            let country_code = article.sourcecountry.clone();
            country_button.connect_clicked(move |_| {
                if let Some(marker_button) = marker_buttons_map.borrow().get(&country_code) {
                    marker_button.emit_by_name::<()>("clicked", &[]);
                    eprintln!("Triggered map marker for {}", country_code);
                } else {
                    eprintln!("No marker found for country: {}", country_code);
                }
            });
        }

        badges_box.append(&country_button);
    }

    // Time badge
    if !article.seendate.is_empty() {
        let formatted_date = parse_gdelt_timestamp(&article.seendate);
        let time_badge = gtk::Label::builder()
            .label(&formatted_date)
            .build();
        time_badge.add_css_class("badge");
        time_badge.add_css_class("badge-time");
        badges_box.append(&time_badge);
    }

    // Language badge
    if !article.language.is_empty() && article.language.to_uppercase() != "ENGLISH" {
        let lang_badge = gtk::Label::builder()
            .label(&article.language.to_uppercase())
            .build();
        lang_badge.add_css_class("badge");
        lang_badge.add_css_class("badge-lang");
        badges_box.append(&lang_badge);
    }

    content_box.append(&badges_box);

    // Domain footer
    if !article.domain.is_empty() {
        let domain_label = Label::builder()
            .label(&article.domain)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        domain_label.add_css_class("article-domain");
        content_box.append(&domain_label);
    }

    card.append(&content_box);

    // Make the entire card clickable to open article
    let gesture = gtk::GestureClick::new();
    let url = article.url.clone();
    gesture.connect_released(move |_, _, _, _| {
        if let Err(e) = open::that(&url) {
            eprintln!("Failed to open URL: {}", e);
        }
    });
    card.add_controller(gesture);

    // Add hover styling
    card.add_css_class("activatable");

    card
}

fn parse_gdelt_timestamp(timestamp: &str) -> String {
    // GDELT format: 20251024T074500Z (YYYYMMDDTHHMMSSZ)
    if timestamp.len() < 15 {
        return timestamp.to_string();
    }

    // Parse the timestamp
    if let Ok(dt) = NaiveDateTime::parse_from_str(timestamp, "%Y%m%dT%H%M%SZ") {
        // Calculate time ago
        let now = chrono::Utc::now().naive_utc();
        let duration = now.signed_duration_since(dt);

        if duration.num_days() > 0 {
            format!("{} days ago", duration.num_days())
        } else if duration.num_hours() > 0 {
            format!("{} hours ago", duration.num_hours())
        } else if duration.num_minutes() > 0 {
            format!("{} minutes ago", duration.num_minutes())
        } else {
            "Just now".to_string()
        }
    } else {
        // Fallback if parsing fails
        timestamp.to_string()
    }
}

/// Create a marker for a country with a popover showing articles
fn create_country_marker(
    marker_layer: &libshumate::MarkerLayer,
    country_code: &str,
    lat: f64,
    lon: f64,
    articles: &[GdeltArticle],
    marker_buttons_map: Rc<RefCell<HashMap<String, gtk::Button>>>,
    use_12_hour: Rc<RefCell<bool>>,
) {
    eprintln!("  Creating marker button for {}", country_code);

    // Create a more compact label - use abbreviated names for long countries
    let display_name = match country_code {
        "United States" => "US",
        "United Kingdom" => "UK",
        "United Arab Emirates" => "UAE",
        "South Africa" => "S. Africa",
        "South Korea" => "S. Korea",
        "New Zealand" => "NZ",
        "Saudi Arabia" => "Saudi",
        _ => country_code,
    };

    // Create a button to serve as the marker
    let marker_button = gtk::Button::builder()
        .label(&format!("{} {}", display_name, articles.len()))
        .build();
    marker_button.add_css_class("map-marker");

    // Store the button in the map for later access from article widgets
    marker_buttons_map.borrow_mut().insert(country_code.to_string(), marker_button.clone());

    // Create a popover to show articles
    let popover = Popover::builder()
        .build();
    popover.add_css_class("map-popover");

    // Create content for the popover
    let popover_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(10)
        .margin_end(10)
        .build();

    // Header with country name and local time
    let header_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .build();

    // Country name and time row
    let country_time_row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();

    let country_label = Label::builder()
        .label(country_code)
        .xalign(0.0)
        .hexpand(true)
        .build();
    country_label.add_css_class("title-3");
    country_time_row.append(&country_label);

    // Create time label that will be updated every second
    let time_label = Label::builder()
        .label("--:--:--")
        .xalign(1.0)
        .build();
    time_label.add_css_class("monospace");
    time_label.add_css_class("dim-label");
    country_time_row.append(&time_label);

    header_box.append(&country_time_row);

    let articles_count_label = Label::builder()
        .label(&format!("{} articles", articles.len()))
        .xalign(0.0)
        .build();
    articles_count_label.add_css_class("dim-label");
    articles_count_label.add_css_class("caption");
    header_box.append(&articles_count_label);

    popover_box.append(&header_box);

    // Set up timezone and time update
    if let Some(tz_str) = get_country_timezone(country_code) {
        if let Ok(tz) = tz_str.parse::<chrono_tz::Tz>() {
            // Update time immediately
            let time_label_clone = time_label.clone();
            let use_12_hour_clone = use_12_hour.clone();
            let update_time = move || {
                let now = chrono::Utc::now().with_timezone(&tz);
                let time_str = if *use_12_hour_clone.borrow() {
                    now.format("%I:%M:%S %p").to_string()
                } else {
                    now.format("%H:%M:%S").to_string()
                };
                time_label_clone.set_label(&time_str);
            };
            update_time();

            // Update every second
            glib::timeout_add_seconds_local(1, move || {
                update_time();
                glib::ControlFlow::Continue
            });
        }
    }

    // Currency section placeholder (will be populated asynchronously)
    let currency_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .visible(false)
        .build();
    currency_box.add_css_class("popover-currency-section");

    popover_box.append(&currency_box);

    // Load currency data asynchronously
    if let Some(currency_code) = get_country_currency(country_code) {
        let currency_box_clone = currency_box.clone();
        let currency_code = currency_code.to_string();
        glib::spawn_future_local(async move {
            if let Some(currency_info) = fetch_currency_info(&currency_code).await {
                // Currency header with rate and last updated timestamp
                let currency_header = gtk::Box::builder()
                    .orientation(Orientation::Horizontal)
                    .spacing(8)
                    .build();

                let currency_label = Label::builder()
                    .label(&format!("{} to USD", currency_info.code))
                    .xalign(0.0)
                    .hexpand(true)
                    .build();
                currency_label.add_css_class("title-4");

                currency_header.append(&currency_label);

                // Add last updated timestamp (right-justified)
                let updated_label = Label::builder()
                    .label(&format!("Updated: {}", chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")))
                    .xalign(1.0)
                    .build();
                updated_label.add_css_class("dim-label");
                updated_label.add_css_class("caption");
                currency_header.append(&updated_label);

                currency_box_clone.append(&currency_header);

                // Rate display with 24hr change indicator
                let rate_box = gtk::Box::builder()
                    .orientation(Orientation::Horizontal)
                    .spacing(8)
                    .build();

                let rate_label = Label::builder()
                    .label(&format!("{:.4}", currency_info.rate_to_usd))
                    .xalign(0.0)
                    .build();
                rate_label.add_css_class("title-3");
                rate_label.add_css_class("currency-rate");

                rate_box.append(&rate_label);

                // Add colored 24hr change next to rate
                if let Some(change_24h) = currency_info.change_24h {
                    let change_label = Label::builder()
                        .label(&format!("({}{:.2}%)",
                            if change_24h > 0.0 { "+" } else { "" },
                            change_24h))
                        .build();
                    change_label.add_css_class("title-4");
                    if change_24h > 0.0 {
                        change_label.add_css_class("currency-change-positive");
                    } else if change_24h < 0.0 {
                        change_label.add_css_class("currency-change-negative");
                    }
                    // If change_24h == 0.0, don't add any color class (default color)
                    rate_box.append(&change_label);
                }

                currency_box_clone.append(&rate_box);

                // 14-day change badge
                if let Some(change_7d) = currency_info.change_7d {
                    let change_7d_badge = Label::builder()
                        .label(&format!("14d: {}{:.2}%",
                            if change_7d > 0.0 { "+" } else { "" },
                            change_7d))
                        .build();
                    change_7d_badge.add_css_class("badge");
                    if change_7d > 0.0 {
                        change_7d_badge.add_css_class("badge-positive");
                    } else if change_7d < 0.0 {
                        change_7d_badge.add_css_class("badge-negative");
                    } else {
                        // Neutral - no change
                        change_7d_badge.add_css_class("badge-neutral");
                    }
                    currency_box_clone.append(&change_7d_badge);
                }

                // Simple sparkline visualization
                if !currency_info.trend_data.is_empty() {
                    let sparkline = create_sparkline(&currency_info.trend_data);
                    currency_box_clone.append(&sparkline);
                }

                // Show the currency box
                currency_box_clone.set_visible(true);
            }
        });
    }

    // Separator
    let separator = gtk::Separator::builder()
        .orientation(Orientation::Horizontal)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    popover_box.append(&separator);

    // Articles section header
    let news_header = Label::builder()
        .label("Recent News")
        .xalign(0.0)
        .build();
    news_header.add_css_class("title-4");
    popover_box.append(&news_header);

    // Create a scrolled window for the articles
    let scrolled = ScrolledWindow::builder()
        .min_content_height(120)
        .max_content_height(280)
        .max_content_width(320)
        .propagate_natural_width(true)
        .propagate_natural_height(false)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let articles_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(2)
        .build();

    // Sort articles by seendate (most recent first)
    let mut sorted_articles = articles.to_vec();
    sorted_articles.sort_by(|a, b| b.seendate.cmp(&a.seendate));

    // Add each article to the popover - limit to 8 most recent
    eprintln!("  Adding {} articles to popover for {}", sorted_articles.len(), country_code);
    for article in sorted_articles.iter().take(8) {
        let article_widget = create_popover_article_row(article);
        articles_box.append(&article_widget);
    }

    scrolled.set_child(Some(&articles_box));
    popover_box.append(&scrolled);

    popover.set_child(Some(&popover_box));
    popover.set_parent(&marker_button);

    // Connect button click to show popover
    let country_code_clone = country_code.to_string();
    marker_button.connect_clicked(move |_| {
        eprintln!("Marker clicked for {}", country_code_clone);
        popover.popup();
    });

    // Create the marker
    let marker = libshumate::Marker::new();
    marker.set_child(Some(&marker_button));
    marker.set_location(lat, lon);

    eprintln!("  Adding marker to layer for {}", country_code);
    // Add marker to the layer
    marker_layer.add_marker(&marker);

    eprintln!("  Marker added successfully for {}", country_code);
}

/// Create a simple sparkline visualization for currency trend with axis labels
fn create_sparkline(data: &[f64]) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .build();

    let drawing_area = gtk::DrawingArea::builder()
        .content_width(280)
        .content_height(60)
        .build();

    // Enable tooltip support
    drawing_area.set_has_tooltip(true);

    let data = data.to_vec();
    let data_for_tooltip = data.clone();

    // Calculate min/max for labels
    let min = if !data.is_empty() {
        data.iter().cloned().fold(f64::INFINITY, f64::min)
    } else {
        0.0
    };
    let max = if !data.is_empty() {
        data.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    } else {
        0.0
    };

    drawing_area.set_draw_func(move |_, cr, width, height| {
        if data.is_empty() {
            return;
        }

        let width = width as f64;
        let height = height as f64;

        // Add margins for better visualization
        let margin_left = 8.0;
        let margin_right = 8.0;
        let margin_top = 15.0;
        let margin_bottom = 20.0;

        let plot_width = width - margin_left - margin_right;
        let plot_height = height - margin_top - margin_bottom;

        // Find min and max for scaling
        let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let range = max - min;

        // Draw subtle grid lines
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.15);
        cr.set_line_width(0.5);

        // Horizontal grid lines (3 lines: top, middle, bottom)
        for i in 0..=2 {
            let y = margin_top + (plot_height * i as f64 / 2.0);
            cr.move_to(margin_left, y);
            cr.line_to(margin_left + plot_width, y);
        }
        let _ = cr.stroke();

        if range == 0.0 {
            // Draw a flat line if no variation
            cr.set_source_rgb(0.5, 0.5, 0.5);
            cr.set_line_width(2.0);
            cr.move_to(margin_left, margin_top + plot_height / 2.0);
            cr.line_to(margin_left + plot_width, margin_top + plot_height / 2.0);
            let _ = cr.stroke();
            return;
        }

        // Draw the area under the curve with neutral color
        let point_spacing = plot_width / (data.len() - 1).max(1) as f64;

        // Use neutral blue color for area fill
        cr.set_source_rgba(0.4, 0.6, 0.9, 0.12); // Neutral blue tint

        // Draw filled area
        cr.move_to(margin_left, margin_top + plot_height);
        for (i, &value) in data.iter().enumerate() {
            let x = margin_left + (i as f64 * point_spacing);
            let y = margin_top + plot_height - ((value - min) / range) * plot_height;
            cr.line_to(x, y);
        }
        cr.line_to(margin_left + plot_width, margin_top + plot_height);
        cr.close_path();
        let _ = cr.fill();

        // Draw the sparkline in neutral blue
        cr.set_source_rgb(0.4, 0.6, 0.9); // Neutral blue
        cr.set_line_width(2.0);

        for (i, &value) in data.iter().enumerate() {
            let x = margin_left + (i as f64 * point_spacing);
            let y = margin_top + plot_height - ((value - min) / range) * plot_height;

            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }

        let _ = cr.stroke();

        // Draw points
        for (i, &value) in data.iter().enumerate() {
            let x = margin_left + (i as f64 * point_spacing);
            let y = margin_top + plot_height - ((value - min) / range) * plot_height;
            cr.arc(x, y, 2.5, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }

        // Draw axis labels (Y-axis values)
        cr.set_source_rgba(0.7, 0.7, 0.7, 0.8);
        cr.set_font_size(9.0);

        // Max value label (top)
        let max_text = format!("{:.4}", max);
        cr.move_to(margin_left, margin_top - 2.0);
        let _ = cr.show_text(&max_text);

        // Min value label (bottom)
        let min_text = format!("{:.4}", min);
        cr.move_to(margin_left, margin_top + plot_height + 12.0);
        let _ = cr.show_text(&min_text);
    });

    // Add tooltip handler for hover
    drawing_area.connect_query_tooltip(move |widget, x, y, _keyboard_mode, tooltip| {
        if data_for_tooltip.is_empty() {
            return false;
        }

        let width = widget.width() as f64;
        let height = widget.height() as f64;

        // Margins must match those in draw_func
        let margin_left = 8.0;
        let margin_right = 8.0;
        let margin_top = 15.0;
        let margin_bottom = 20.0;

        let plot_width = width - margin_left - margin_right;
        let plot_height = height - margin_top - margin_bottom;

        let point_spacing = plot_width / (data_for_tooltip.len() - 1).max(1) as f64;

        // Find the closest data point to the mouse cursor
        let mouse_x = x as f64;
        let mouse_y = y as f64;

        // Check if mouse is within the plot area
        if mouse_x < margin_left || mouse_x > margin_left + plot_width ||
           mouse_y < margin_top || mouse_y > margin_top + plot_height {
            return false;
        }

        // Find closest point
        let mut closest_idx = 0;
        let mut closest_dist = f64::INFINITY;

        let min = data_for_tooltip.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = data_for_tooltip.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let range = max - min;

        if range == 0.0 {
            return false;
        }

        for (i, &value) in data_for_tooltip.iter().enumerate() {
            let point_x = margin_left + (i as f64 * point_spacing);
            let point_y = margin_top + plot_height - ((value - min) / range) * plot_height;

            let dist = ((point_x - mouse_x).powi(2) + (point_y - mouse_y).powi(2)).sqrt();

            if dist < closest_dist {
                closest_dist = dist;
                closest_idx = i;
            }
        }

        // Only show tooltip if mouse is reasonably close to a point (within 20 pixels)
        if closest_dist > 20.0 {
            return false;
        }

        let value = data_for_tooltip[closest_idx];
        let days_ago = data_for_tooltip.len() - 1 - closest_idx;

        let tooltip_text = if days_ago == 0 {
            format!("Today: {:.4}", value)
        } else if days_ago == 1 {
            format!("Yesterday: {:.4}", value)
        } else {
            format!("{} days ago: {:.4}", days_ago, value)
        };

        tooltip.set_text(Some(&tooltip_text));
        true
    });

    container.append(&drawing_area);

    // Add X-axis label
    let x_axis_label = Label::builder()
        .label("14-day trend")
        .xalign(0.5)
        .build();
    x_axis_label.add_css_class("dim-label");
    x_axis_label.add_css_class("caption");
    container.append(&x_axis_label);

    container
}

/// Create a compact article row for the popover
fn create_popover_article_row(article: &GdeltArticle) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    row.add_css_class("popover-article-row");

    // Article title
    let title_label = Label::builder()
        .label(&article.title)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(45)
        .width_chars(45)
        .build();
    title_label.add_css_class("popover-article-title");

    row.append(&title_label);

    // Metadata row with domain and time
    let metadata_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();

    // Domain
    if !article.domain.is_empty() {
        let domain_label = Label::builder()
            .label(&article.domain)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .hexpand(true)
            .build();
        domain_label.add_css_class("popover-article-meta");
        metadata_box.append(&domain_label);
    }

    // Time badge
    if !article.seendate.is_empty() {
        let formatted_date = parse_gdelt_timestamp(&article.seendate);
        let time_label = Label::builder()
            .label(&formatted_date)
            .xalign(1.0)
            .build();
        time_label.add_css_class("popover-article-time");
        metadata_box.append(&time_label);
    }

    row.append(&metadata_box);

    // Make the row clickable
    let gesture = gtk::GestureClick::new();
    let url = article.url.clone();
    gesture.connect_released(move |_, _, _, _| {
        if let Err(e) = open::that(&url) {
            eprintln!("Failed to open URL: {}", e);
        }
    });
    row.add_controller(gesture);

    // Add hover styling
    row.add_css_class("activatable");

    row
}

/// Fetch currency information from Frankfurter API
/// Returns currency info with current rate and trend data
async fn fetch_currency_info(currency_code: &str) -> Option<CurrencyInfo> {
    use crate::data::{FrankfurterLatestResponse, FrankfurterHistoricalResponse};

    if currency_code == "USD" {
        // USD is the base, so rate is always 1.0
        return Some(CurrencyInfo {
            code: currency_code.to_string(),
            rate_to_usd: 1.0,
            change_24h: Some(0.0),
            change_7d: Some(0.0),
            trend_data: vec![1.0; 8], // Flat trend for USD
        });
    }

    // Get today's date and 14 days ago (for better trend visualization)
    let today = chrono::Utc::now().date_naive();
    let fourteen_days_ago = today - chrono::Duration::days(14);

    // Fetch latest rate (currency to USD)
    let latest_url = format!(
        "https://api.frankfurter.app/latest?from={}&to=USD",
        currency_code
    );

    let latest_rate = match reqwest::get(&latest_url).await {
        Ok(response) => {
            match response.json::<FrankfurterLatestResponse>().await {
                Ok(data) => data.rates.rates.get("USD").copied(),
                Err(e) => {
                    eprintln!("Failed to parse latest currency data for {}: {}", currency_code, e);
                    None
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to fetch latest currency data for {}: {}", currency_code, e);
            None
        }
    };

    let latest_rate = latest_rate?;

    // Fetch 14-day historical data for trend
    let historical_url = format!(
        "https://api.frankfurter.app/{}..{}?from={}&to=USD",
        fourteen_days_ago.format("%Y-%m-%d"),
        today.format("%Y-%m-%d"),
        currency_code
    );

    let (change_24h, change_7d, trend_data) = match reqwest::get(&historical_url).await {
        Ok(response) => {
            match response.json::<FrankfurterHistoricalResponse>().await {
                Ok(data) => {
                    // Extract rates sorted by date
                    let mut dates: Vec<_> = data.rates.keys().collect();
                    dates.sort();

                    let rates: Vec<f64> = dates
                        .iter()
                        .filter_map(|date| {
                            data.rates.get(*date).and_then(|r| r.rates.get("USD").copied())
                        })
                        .collect();

                    let change_24h = if rates.len() >= 2 {
                        let yesterday = rates[rates.len() - 2];
                        Some(((latest_rate - yesterday) / yesterday) * 100.0)
                    } else {
                        None
                    };

                    let change_7d = if !rates.is_empty() {
                        let week_ago = rates[0];
                        Some(((latest_rate - week_ago) / week_ago) * 100.0)
                    } else {
                        None
                    };

                    (change_24h, change_7d, rates)
                }
                Err(e) => {
                    eprintln!("Failed to parse historical currency data for {}: {}", currency_code, e);
                    (None, None, vec![])
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to fetch historical currency data for {}: {}", currency_code, e);
            (None, None, vec![])
        }
    };

    Some(CurrencyInfo {
        code: currency_code.to_string(),
        rate_to_usd: latest_rate,
        change_24h,
        change_7d,
        trend_data,
    })
}
