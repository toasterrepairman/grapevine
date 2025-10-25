use gtk::prelude::*;
use gtk::{glib, Application, Label, Orientation, ScrolledWindow, Align, SearchEntry, ListBox};
use libadwaita::{prelude::*, ViewSwitcher, HeaderBar, ToolbarView, ApplicationWindow, ViewStack, StyleManager, ColorScheme, Breakpoint, BreakpointCondition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashSet;
use chrono::{DateTime, NaiveDateTime};

const APP_ID: &str = "com.example.Grapevine";
const GDELT_API_URL: &str = "https://api.gdeltproject.org/api/v2/doc/doc";

#[derive(Debug, Deserialize, Clone)]
struct GdeltArticle {
    url: String,
    title: String,
    #[serde(default)]
    seendate: String,
    #[serde(default)]
    socialimage: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    sourcecountry: String,
}

#[derive(Debug, Deserialize)]
struct GdeltResponse {
    articles: Vec<GdeltArticle>,
}

fn main() -> glib::ExitCode {
    // Initialize Tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(build_ui);

    let exit_code = app.run();

    // Keep runtime alive until app exits
    drop(_guard);
    drop(rt);

    exit_code
}

fn build_ui(app: &Application) {
    // Enable dark theme support
    let style_manager = StyleManager::default();
    style_manager.set_color_scheme(ColorScheme::PreferDark);

    // Create the main stack for content
    let stack = ViewStack::builder()
        .build();

    // Create Global Affairs view with map
    let global_affairs_view = create_global_affairs_view();
    stack.add_titled(&global_affairs_view, Some("global-affairs"), "Global Affairs");

    // Create Firehose view
    let firehose_view = create_firehose_view();
    stack.add_titled(&firehose_view, Some("firehose"), "Firehose");

    // Create floating ViewSwitcher (compact version)
    let view_switcher = ViewSwitcher::builder()
        .stack(&stack)
        .policy(libadwaita::ViewSwitcherPolicy::Wide)
        .halign(Align::Center)
        .valign(Align::End)
        .margin_bottom(24)
        .build();

    // Add CSS class for styling
    view_switcher.add_css_class("floating-switcher");

    // Create overlay to layer the switcher on top of the stack
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&stack));
    overlay.add_overlay(&view_switcher);

    // Create header bar
    let header_bar = HeaderBar::builder()
        .build();

    // Create toolbar view to contain everything
    let toolbar_view = ToolbarView::builder()
        .build();

    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.set_content(Some(&overlay));

    // Create main window
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Grapevine")
        .default_width(800)
        .default_height(600)
        .build();

    // Load custom CSS for floating switcher styling
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_data(
        ".floating-switcher {
            background-color: alpha(@window_bg_color, 0.85);
            border-radius: 12px;
            padding: 8px;
            box-shadow: 0 4px 12px alpha(black, 0.3);
        }"
    );

    gtk::style_context_add_provider_for_display(
        &gtk::prelude::WidgetExt::display(&window),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    window.set_content(Some(&toolbar_view));
    window.present();
}

fn create_global_affairs_view() -> gtk::Box {
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

    scrollbox_content.append(&search_entry);
    scrollbox_content.append(&results_list);
    scrolled_window.set_child(Some(&scrollbox_content));

    // Clone for async callbacks
    let results_list_clone = results_list.clone();

    // Perform initial search with empty query to get latest news
    glib::spawn_future_local(async move {
        fetch_gdelt_articles("", results_list_clone).await;
    });

    // Set up search entry activation
    let results_list_for_search = results_list.clone();
    search_entry.connect_activate(move |entry| {
        let query = entry.text().to_string();
        let results_list = results_list_for_search.clone();

        glib::spawn_future_local(async move {
            fetch_gdelt_articles(&query, results_list).await;
        });
    });

    // Create the map widget using libshumate
    let map = libshumate::SimpleMap::new();

    // Create a custom dark-themed map source using CartoDB Dark Matter tiles
    let map_source = libshumate::RasterRenderer::from_url(
        "https://a.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}.png"
    );

    map.set_map_source(Some(&map_source));

    // Configure the map view with initial position and zoom constraints
    if let Some(map_view) = map.map() {
        // Set zoom level constraints on the viewport to prevent excessive wrapping
        if let Some(viewport) = map_view.viewport() {
            // Min zoom 1 (whole world visible), max zoom 6 (reasonable detail)
            viewport.set_min_zoom_level(1);
            viewport.set_max_zoom_level(10);
        }

        // Set initial zoom level to 2 (good overview of world)
        map_view.go_to_full(0.0, 0.0, 2.0);
    }

    // Make the map expand to fill the space
    map.set_vexpand(true);
    map.set_hexpand(true);

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
                        paned.set_position(width - 300); // 300px from right for scrollbox
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

fn create_firehose_view() -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    // Create a scrolled window for the firehose content
    let scrolled_window = ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .build();

    let firehose_content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();

    let placeholder_label = Label::builder()
        .label("Firehose feed will appear here")
        .build();

    firehose_content.append(&placeholder_label);
    scrolled_window.set_child(Some(&firehose_content));

    container.append(&scrolled_window);
    container
}

async fn fetch_gdelt_articles(query: &str, results_list: ListBox) {
    // Clear existing results
    while let Some(child) = results_list.first_child() {
        results_list.remove(&child);
    }

    // Show loading indicator
    let loading_label = Label::builder()
        .label("Loading...")
        .margin_top(12)
        .margin_bottom(12)
        .build();
    results_list.append(&loading_label);

    // GDELT requires non-empty queries, use a default search term
    let search_query = if query.is_empty() { "news" } else { query };

    // Build the API URL with English language filter - request more to allow deduplication
    let url = format!(
        "{}?query={} sourcelang:english&mode=artlist&maxrecords=50&timespan=7d&format=json",
        GDELT_API_URL,
        urlencoding::encode(search_query)
    );

    eprintln!("Fetching from URL: {}", url);

    // Fetch data from GDELT API
    match reqwest::get(&url).await {
        Ok(response) => {
            // Get the raw text first to help debug
            match response.text().await {
                Ok(text) => {
                    eprintln!("Response text (first 500 chars): {}", &text.chars().take(500).collect::<String>());

                    // Try to parse the JSON
                    match serde_json::from_str::<GdeltResponse>(&text) {
                        Ok(data) => {
                            // Remove loading indicator BEFORE adding new content
                            results_list.remove(&loading_label);

                            if data.articles.is_empty() {
                                let no_results = Label::builder()
                                    .label("No articles found")
                                    .margin_top(12)
                                    .margin_bottom(12)
                                    .build();
                                results_list.append(&no_results);
                            } else {
                                // Deduplicate by domain - only show one article per source
                                let mut seen_domains = HashSet::new();
                                let mut unique_articles = Vec::new();

                                for article in data.articles.iter() {
                                    if !seen_domains.contains(&article.domain) {
                                        seen_domains.insert(article.domain.clone());
                                        unique_articles.push(article);

                                        // Stop once we have 10 unique sources
                                        if unique_articles.len() >= 10 {
                                            break;
                                        }
                                    }
                                }

                                // Display deduplicated articles
                                for article in unique_articles {
                                    let article_row = create_article_row(article);
                                    results_list.append(&article_row);
                                }
                            }
                        }
                        Err(e) => {
                            results_list.remove(&loading_label);
                            eprintln!("JSON parse error: {}", e);
                            let error_label = Label::builder()
                                .label(&format!("Error parsing response: {}", e))
                                .wrap(true)
                                .margin_top(12)
                                .margin_bottom(12)
                                .build();
                            results_list.append(&error_label);
                        }
                    }
                }
                Err(e) => {
                    results_list.remove(&loading_label);
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
            results_list.remove(&loading_label);
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

fn create_article_row(article: &GdeltArticle) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(4)
        .margin_end(4)
        .build();

    // Article title
    let title_label = Label::builder()
        .label(&article.title)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .build();
    title_label.add_css_class("heading");

    // Metadata (domain, date, country)
    let mut metadata_parts = Vec::new();
    if !article.domain.is_empty() {
        metadata_parts.push(article.domain.clone());
    }
    if !article.seendate.is_empty() {
        // Parse GDELT timestamp format: 20251024T074500Z
        let formatted_date = parse_gdelt_timestamp(&article.seendate);
        metadata_parts.push(formatted_date);
    }
    if !article.sourcecountry.is_empty() {
        metadata_parts.push(article.sourcecountry.clone());
    }

    let metadata_label = Label::builder()
        .label(&metadata_parts.join(" • "))
        .wrap(true)
        .xalign(0.0)
        .build();
    metadata_label.add_css_class("dim-label");
    metadata_label.add_css_class("caption");

    row.append(&title_label);
    row.append(&metadata_label);

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
