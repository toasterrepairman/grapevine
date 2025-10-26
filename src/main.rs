use gtk::prelude::*;
use gtk::{glib, Application, Label, Orientation, ScrolledWindow, Align, SearchEntry, ListBox, Popover};
use libadwaita::{prelude::*, ViewSwitcher, HeaderBar, ToolbarView, ApplicationWindow, ViewStack, StyleManager, ColorScheme};
use libshumate::prelude::{MarkerExt, LocationExt};
use serde::Deserialize;
use std::collections::{HashSet, HashMap};
use std::cell::RefCell;
use std::rc::Rc;
use chrono::NaiveDateTime;
use jetstream_oxide::{
    events::{JetstreamEvent, commit::CommitEvent},
    DefaultJetstreamEndpoints, JetstreamCompression, JetstreamConfig, JetstreamConnector,
};
use atrium_api::record::KnownRecord;
use atrium_api::types::string::Nsid;

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

    // Create shared state for refresh functionality
    let current_query = Rc::new(RefCell::new(String::new()));
    let results_list_ref = Rc::new(RefCell::new(None::<ListBox>));
    let marker_layer_ref = Rc::new(RefCell::new(None::<libshumate::MarkerLayer>));

    // Create Global Affairs view with map
    let global_affairs_view = create_global_affairs_view(
        current_query.clone(),
        results_list_ref.clone(),
        marker_layer_ref.clone()
    );
    let _global_affairs_page = stack.add_titled(&global_affairs_view, Some("global-affairs"), "Global Affairs");
    stack.page(&global_affairs_view).set_icon_name(None);

    // Create Firehose view
    let (firehose_view, firehose_control) = create_firehose_view();
    let _firehose_page = stack.add_titled(&firehose_view, Some("firehose"), "Firehose");
    stack.page(&firehose_view).set_icon_name(None);

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

    // Create refresh button (for Global Affairs)
    let refresh_button = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Refresh articles")
        .build();

    // Create plus button (for Firehose)
    let plus_button = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add filtered view")
        .visible(false)
        .build();

    // Connect refresh button to trigger a new search
    let current_query_clone = current_query.clone();
    let results_list_ref_clone = results_list_ref.clone();
    let marker_layer_ref_clone = marker_layer_ref.clone();
    refresh_button.connect_clicked(move |_| {
        let query = current_query_clone.borrow().clone();
        if let Some(results_list) = results_list_ref_clone.borrow().as_ref() {
            let results_list = results_list.clone();
            let marker_layer = marker_layer_ref_clone.borrow().clone();
            glib::spawn_future_local(async move {
                fetch_gdelt_articles(&query, results_list, marker_layer).await;
            });
        }
    });

    // Connect plus button to add split view
    let firehose_control_clone = firehose_control.clone();
    plus_button.connect_clicked(move |_| {
        firehose_control_clone.add_split();
    });

    // Switch buttons based on active view
    let refresh_button_clone = refresh_button.clone();
    let plus_button_clone = plus_button.clone();
    stack.connect_visible_child_notify(move |stack| {
        if let Some(visible_child) = stack.visible_child() {
            if let Some(name) = stack.page(&visible_child).name() {
                if name.as_str() == "firehose" {
                    refresh_button_clone.set_visible(false);
                    plus_button_clone.set_visible(true);
                } else {
                    refresh_button_clone.set_visible(true);
                    plus_button_clone.set_visible(false);
                }
            }
        }
    });

    header_bar.pack_start(&refresh_button);
    header_bar.pack_start(&plus_button);

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

    // Load custom CSS for floating switcher and map markers
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_data(
        ".floating-switcher {
            background-color: alpha(@window_bg_color, 0.85);
            border-radius: 12px;
            padding: 8px;
            box-shadow: 0 4px 12px alpha(black, 0.3);
        }
        .map-marker {
            background-color: alpha(@accent_bg_color, 0.75);
            border-radius: 16px;
            padding: 4px 10px;
            font-size: 11px;
            font-weight: bold;
            min-height: 0;
            min-width: 0;
            box-shadow: 0 2px 6px alpha(black, 0.4);
        }
        .map-marker:hover {
            background-color: alpha(@accent_bg_color, 0.95);
            box-shadow: 0 3px 8px alpha(black, 0.5);
        }
        .map-popover > contents {
            background-color: alpha(@card_bg_color, 0.95);
            border-radius: 12px;
            box-shadow: 0 4px 16px alpha(black, 0.6);
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

fn create_global_affairs_view(
    current_query: Rc<RefCell<String>>,
    results_list_ref: Rc<RefCell<Option<ListBox>>>,
    marker_layer_ref: Rc<RefCell<Option<libshumate::MarkerLayer>>>,
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

    // Perform initial search with empty query to get latest news
    glib::spawn_future_local(async move {
        fetch_gdelt_articles("", results_list_clone, marker_layer_clone).await;
    });

    // Set up automatic refresh every 15 minutes
    let current_query_for_refresh = current_query.clone();
    let results_list_for_refresh = results_list.clone();
    let marker_layer_for_refresh = marker_layer_opt.clone();
    glib::timeout_add_seconds_local(15 * 60, move || {
        let query = current_query_for_refresh.borrow().clone();
        let results_list = results_list_for_refresh.clone();
        let marker_layer = marker_layer_for_refresh.clone();

        glib::spawn_future_local(async move {
            fetch_gdelt_articles(&query, results_list, marker_layer).await;
        });

        glib::ControlFlow::Continue
    });

    // Set up search entry activation
    let results_list_for_search = results_list.clone();
    let marker_layer_for_search = marker_layer_opt.clone();
    let current_query_for_search = current_query.clone();
    search_entry.connect_activate(move |entry| {
        let query = entry.text().to_string();

        // Update the current query
        *current_query_for_search.borrow_mut() = query.clone();

        let results_list = results_list_for_search.clone();
        let marker_layer = marker_layer_for_search.clone();

        glib::spawn_future_local(async move {
            fetch_gdelt_articles(&query, results_list, marker_layer).await;
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

#[derive(Clone)]
struct SplitPane {
    container: gtk::Box,
    list: ListBox,
    search_entry: SearchEntry,
    filter_keyword: Rc<RefCell<String>>,
}

#[derive(Clone)]
struct FirehoseControl {
    root_container: gtk::Box,
    main_pane: SplitPane,
    splits: Rc<RefCell<Vec<SplitPane>>>,
    message_sender: flume::Sender<String>,
}

impl FirehoseControl {
    fn add_split(&self) {
        let mut splits = self.splits.borrow_mut();

        // Create a new split pane
        let split_box = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .hexpand(true)
            .build();

        // Create header box with search and close button
        let header_box = gtk::Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .build();

        let search_entry = SearchEntry::builder()
            .placeholder_text("Filter messages by keyword...")
            .hexpand(true)
            .build();

        let close_button = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Close this split")
            .build();

        header_box.append(&search_entry);
        header_box.append(&close_button);

        // Create list for this split
        let split_list = ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .build();
        split_list.add_css_class("boxed-list");

        let split_scrolled = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();
        split_scrolled.set_child(Some(&split_list));

        split_box.append(&header_box);
        split_box.append(&split_scrolled);

        // Create filter keyword storage
        let filter_keyword = Rc::new(RefCell::new(String::new()));

        // Set up search filtering
        let split_list_for_search = split_list.clone();
        let filter_keyword_for_search = filter_keyword.clone();
        search_entry.connect_search_changed(move |entry| {
            let keyword = entry.text().to_string();
            *filter_keyword_for_search.borrow_mut() = keyword;

            // Clear the list when search changes
            while let Some(child) = split_list_for_search.first_child() {
                split_list_for_search.remove(&child);
            }
        });

        // Add the new split pane
        let split_pane = SplitPane {
            container: split_box.clone(),
            list: split_list.clone(),
            search_entry: search_entry.clone(),
            filter_keyword: filter_keyword.clone(),
        };

        splits.push(split_pane);

        // Rebuild the entire paned structure
        drop(splits); // Drop the borrow before rebuilding
        self.rebuild_layout();

        // Set up close button
        let control_clone = self.clone();
        let split_box_clone = split_box.clone();
        close_button.connect_clicked(move |_| {
            // Find and remove this split
            let mut splits = control_clone.splits.borrow_mut();
            if let Some(pos) = splits.iter().position(|s| s.container == split_box_clone) {
                splits.remove(pos);
                drop(splits); // Drop the borrow before rebuilding
                control_clone.rebuild_layout();
            }
        });
    }

    fn rebuild_layout(&self) {
        // Remove all children from root container
        while let Some(child) = self.root_container.first_child() {
            self.root_container.remove(&child);
        }

        let splits = self.splits.borrow();

        // Unparent all widgets before rebuilding
        if let Some(parent) = self.main_pane.container.parent() {
            if let Some(paned) = parent.downcast_ref::<gtk::Paned>() {
                paned.set_start_child(None::<&gtk::Widget>);
                paned.set_end_child(None::<&gtk::Widget>);
            }
        }

        for split in splits.iter() {
            if let Some(parent) = split.container.parent() {
                if let Some(paned) = parent.downcast_ref::<gtk::Paned>() {
                    paned.set_start_child(None::<&gtk::Widget>);
                    paned.set_end_child(None::<&gtk::Widget>);
                }
            }
        }

        if splits.is_empty() {
            // Only show the main pane
            self.root_container.append(&self.main_pane.container);
        } else {
            // Create nested paned widgets
            let orientation = if self.root_container.width() > self.root_container.height() {
                Orientation::Horizontal
            } else {
                Orientation::Vertical
            };

            // Start with the main pane
            let mut current_widget: gtk::Widget = self.main_pane.container.clone().into();

            // Add each split with a paned separator
            for split in splits.iter() {
                let paned = gtk::Paned::builder()
                    .orientation(orientation)
                    .wide_handle(true)
                    .resize_start_child(true)
                    .shrink_start_child(false)
                    .resize_end_child(true)
                    .shrink_end_child(false)
                    .build();

                paned.set_start_child(Some(&current_widget));
                paned.set_end_child(Some(&split.container));

                // Set position to split evenly
                let paned_weak = paned.downgrade();
                paned.add_tick_callback(move |_widget, _clock| {
                    if let Some(paned) = paned_weak.upgrade() {
                        let total_size = if paned.orientation() == Orientation::Horizontal {
                            paned.width()
                        } else {
                            paned.height()
                        };

                        if total_size > 0 && paned.position() == 0 {
                            paned.set_position(total_size / 2);
                        }
                    }
                    glib::ControlFlow::Continue
                });

                current_widget = paned.into();
            }

            self.root_container.append(&current_widget);
        }

        // Add tick callback to handle orientation changes
        let root_weak = self.root_container.downgrade();
        let control_clone = self.clone();

        self.root_container.add_tick_callback(move |_widget, _clock| {
            if let Some(root) = root_weak.upgrade() {
                let width = root.width();
                let height = root.height();

                if width > 0 && height > 0 {
                    let should_be_horizontal = width > height;

                    // Check if we need to rebuild due to orientation change
                    if let Some(first_child) = root.first_child() {
                        if let Some(paned) = first_child.downcast_ref::<gtk::Paned>() {
                            let is_horizontal = paned.orientation() == Orientation::Horizontal;

                            if should_be_horizontal != is_horizontal {
                                control_clone.rebuild_layout();
                            }
                        }
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    fn broadcast_message(&self, message: &str) {
        let splits = self.splits.borrow();
        for split in splits.iter() {
            let keyword = split.filter_keyword.borrow().clone();
            if !keyword.is_empty() && message.to_lowercase().contains(&keyword.to_lowercase()) {
                add_message_to_list(&split.list, message);
            }
        }
    }
}

fn create_firehose_view() -> (gtk::Box, FirehoseControl) {
    let container = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .build();

    // Create root container that will hold the dynamic paned structure
    let root_container = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .build();

    // Create the main firehose box with search entry
    let main_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .hexpand(true)
        .vexpand(true)
        .build();

    let main_search = SearchEntry::builder()
        .placeholder_text("Filter messages by keyword...")
        .build();

    // Create the main firehose list
    let main_list = ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    main_list.add_css_class("boxed-list");

    let main_scrolled = ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .build();
    main_scrolled.set_child(Some(&main_list));

    main_box.append(&main_search);
    main_box.append(&main_scrolled);

    // Initially add main box to root container
    root_container.append(&main_box);

    container.append(&root_container);

    // Create channels for message passing
    let (tx, rx) = flume::unbounded::<String>();
    let main_filter_keyword = Rc::new(RefCell::new(String::new()));

    // Create the main pane structure
    let main_pane = SplitPane {
        container: main_box.clone(),
        list: main_list.clone(),
        search_entry: main_search.clone(),
        filter_keyword: main_filter_keyword.clone(),
    };

    // Create the control before setting up the receiver
    let control = FirehoseControl {
        root_container: root_container.clone(),
        main_pane,
        splits: Rc::new(RefCell::new(Vec::new())),
        message_sender: tx.clone(),
    };

    // Store references for the UI update
    let main_list_clone = main_list.clone();
    let main_filter_keyword_clone = main_filter_keyword.clone();
    let control_clone = control.clone();

    // Create a buffer for batching messages
    let message_buffer = Rc::new(RefCell::new(Vec::new()));
    let message_buffer_clone = message_buffer.clone();

    // Set up receiver to collect incoming messages into buffer
    glib::spawn_future_local(async move {
        while let Ok(message) = rx.recv_async().await {
            message_buffer_clone.borrow_mut().push(message);
        }
    });

    // Set up a timer to process batched messages 5 times per second (every 200ms)
    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        let mut buffer = message_buffer.borrow_mut();

        if !buffer.is_empty() {
            // Process all buffered messages
            for message in buffer.iter() {
                // Add to main list if it matches the main filter
                let main_keyword = main_filter_keyword_clone.borrow().clone();
                if main_keyword.is_empty() || message.to_lowercase().contains(&main_keyword.to_lowercase()) {
                    add_message_to_list(&main_list_clone, message);
                }

                // Broadcast to all splits
                control_clone.broadcast_message(message);
            }

            // Clear the buffer
            buffer.clear();
        }

        glib::ControlFlow::Continue
    });

    // Start the Jetstream connection in a background task
    let tx_clone = tx.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(e) = start_jetstream(tx_clone).await {
                eprintln!("Jetstream error: {}", e);
            }
        });
    });

    // Handle main search filter
    let main_list_for_search = main_list.clone();
    let main_filter_keyword_for_search = main_filter_keyword.clone();
    main_search.connect_search_changed(move |entry| {
        let keyword = entry.text().to_string();
        *main_filter_keyword_for_search.borrow_mut() = keyword;

        // Clear the main list when search changes
        while let Some(child) = main_list_for_search.first_child() {
            main_list_for_search.remove(&child);
        }
    });

    (container, control)
}

fn add_message_to_list(list: &ListBox, message: &str) {
    let row = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(8)
        .margin_end(8)
        .build();

    let message_label = Label::builder()
        .label(message)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .build();

    row.append(&message_label);

    // Prepend to show newest messages at the top
    list.prepend(&row);

    // Limit to 100 messages to prevent memory issues
    let mut count = 0;
    let mut child = list.first_child();
    while let Some(current) = child {
        count += 1;
        if count > 100 {
            let next = current.next_sibling();
            list.remove(&current);
            child = next;
        } else {
            child = current.next_sibling();
        }
    }
}

async fn start_jetstream(tx: flume::Sender<String>) -> anyhow::Result<()> {
    let nsid: Nsid = "app.bsky.feed.post".parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse NSID: {}", e))?;

    let config = JetstreamConfig {
        endpoint: DefaultJetstreamEndpoints::USEastOne.into(),
        wanted_collections: vec![nsid],
        wanted_dids: vec![],
        compression: JetstreamCompression::Zstd,
        cursor: None,
        max_retries: 10,
        max_delay_ms: 30_000,
        base_delay_ms: 1_000,
        reset_retries_min_ms: 30_000,
    };

    let jetstream = JetstreamConnector::new(config)?;
    let receiver = jetstream.connect().await?;

    eprintln!("Connected to Bluesky Jetstream!");

    while let Ok(event) = receiver.recv_async().await {
        if let JetstreamEvent::Commit(commit_event) = &event {
            match commit_event {
                CommitEvent::Create { commit, .. } => {
                    if let KnownRecord::AppBskyFeedPost(post) = &commit.record {
                        let timestamp = chrono::Utc::now().format("%H:%M:%S").to_string();
                        let rkey = &commit.info.rkey;

                        // Format: [timestamp] rkey: text
                        let message = format!("[{}] {}: {}", timestamp, rkey, post.text);

                        // Send to UI thread
                        if tx.send(message).is_err() {
                            break; // UI is gone, stop streaming
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

async fn fetch_gdelt_articles(query: &str, results_list: ListBox, marker_layer: Option<libshumate::MarkerLayer>) {
    // Clear existing results
    while let Some(child) = results_list.first_child() {
        results_list.remove(&child);
    }

    // Clear existing markers if marker layer is provided
    if let Some(ref layer) = marker_layer {
        layer.remove_all();
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

    // GDELT requires non-empty queries, use a default search term
    let search_query = if query.is_empty() { "news" } else { query };

    // Build the API URL with English language filter - request more to allow deduplication
    // Use timespan=1h to get only the most recent articles
    let url = format!(
        "{}?query={} sourcelang:english&mode=artlist&maxrecords=25&timespan=2h&format=json",
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

                                // Deduplicate by domain - only show one article per source
                                let mut seen_domains = HashSet::new();
                                let mut unique_articles = Vec::new();

                                for article in sorted_articles.iter() {
                                    if !seen_domains.contains(&article.domain) {
                                        seen_domains.insert(article.domain.clone());
                                        unique_articles.push(article);

                                        // Stop once we have 25 unique sources
                                        if unique_articles.len() >= 25 {
                                            break;
                                        }
                                    }
                                }

                                // Display deduplicated articles
                                for article in unique_articles.iter() {
                                    let article_row = create_article_row(article);
                                    results_list.append(&article_row);
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
                                            create_country_marker(layer, country_code, lat, lon, articles);
                                        } else {
                                            eprintln!("No coordinates found for country code: {}", country_code);
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

/// Create a marker for a country with a popover showing articles
fn create_country_marker(
    marker_layer: &libshumate::MarkerLayer,
    country_code: &str,
    lat: f64,
    lon: f64,
    articles: &[GdeltArticle]
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

    // Create a popover to show articles
    let popover = Popover::builder()
        .build();
    popover.add_css_class("map-popover");

    // Create content for the popover
    let popover_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    // Add country header
    let country_label = Label::builder()
        .label(&format!("Articles from {}", country_code))
        .build();
    country_label.add_css_class("title-4");
    popover_box.append(&country_label);

    // Create a scrolled window for the articles
    let scrolled = ScrolledWindow::builder()
        .max_content_height(300)
        .max_content_width(280)
        .propagate_natural_width(true)
        .propagate_natural_height(true)
        .build();

    let articles_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();

    // Sort articles by seendate (most recent first)
    let mut sorted_articles = articles.to_vec();
    sorted_articles.sort_by(|a, b| b.seendate.cmp(&a.seendate));

    // Add each article to the popover
    eprintln!("  Adding {} articles to popover for {}", sorted_articles.len(), country_code);
    for article in sorted_articles.iter() {
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

/// Create a compact article row for the popover
fn create_popover_article_row(article: &GdeltArticle) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(2)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();

    // Article title
    let title_label = Label::builder()
        .label(&article.title)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .max_width_chars(35)
        .build();
    title_label.add_css_class("caption");

    // Domain
    let domain_label = Label::builder()
        .label(&article.domain)
        .xalign(0.0)
        .build();
    domain_label.add_css_class("dim-label");
    domain_label.add_css_class("caption");

    row.append(&title_label);
    row.append(&domain_label);

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

/// Get approximate coordinates for a country code or name
/// Returns (latitude, longitude) or None if country is unknown
fn get_country_coordinates(country: &str) -> Option<(f64, f64)> {
    // Map of country codes and names to approximate center coordinates
    let coords: HashMap<&str, (f64, f64)> = [
        // Country codes
        ("US", (37.0902, -95.7129)),
        ("GB", (55.3781, -3.4360)),
        ("CA", (56.1304, -106.3468)),
        ("AU", (-25.2744, 133.7751)),
        ("DE", (51.1657, 10.4515)),
        ("FR", (46.2276, 2.2137)),
        ("IT", (41.8719, 12.5674)),
        ("ES", (40.4637, -3.7492)),
        ("RU", (61.5240, 105.3188)),
        ("CN", (35.8617, 104.1954)),
        ("JP", (36.2048, 138.2529)),
        ("IN", (20.5937, 78.9629)),
        ("BR", (-14.2350, -51.9253)),
        ("MX", (23.6345, -102.5528)),
        ("AR", (-38.4161, -63.6167)),
        ("ZA", (-30.5595, 22.9375)),
        ("EG", (26.8206, 30.8025)),
        ("NG", (9.0820, 8.6753)),
        ("KE", (-0.0236, 37.9062)),
        ("SA", (23.8859, 45.0792)),
        ("AE", (23.4241, 53.8478)),
        ("TR", (38.9637, 35.2433)),
        ("IL", (31.0461, 34.8516)),
        ("SE", (60.1282, 18.6435)),
        ("NO", (60.4720, 8.4689)),
        ("FI", (61.9241, 25.7482)),
        ("DK", (56.2639, 9.5018)),
        ("NL", (52.1326, 5.2913)),
        ("BE", (50.5039, 4.4699)),
        ("CH", (46.8182, 8.2275)),
        ("AT", (47.5162, 14.5501)),
        ("PL", (51.9194, 19.1451)),
        ("CZ", (49.8175, 15.4730)),
        ("GR", (39.0742, 21.8243)),
        ("PT", (39.3999, -8.2245)),
        ("IE", (53.4129, -8.2439)),
        ("NZ", (-40.9006, 174.8860)),
        ("SG", (1.3521, 103.8198)),
        ("HK", (22.3193, 114.1694)),
        ("KR", (35.9078, 127.7669)),
        ("TH", (15.8700, 100.9925)),
        ("MY", (4.2105, 101.9758)),
        ("ID", (-0.7893, 113.9213)),
        ("PH", (12.8797, 121.7740)),
        ("VN", (14.0583, 108.2772)),
        ("UA", (48.3794, 31.1656)),
        ("RO", (45.9432, 24.9668)),
        ("HU", (47.1625, 19.5033)),
        ("CL", (-35.6751, -71.5430)),
        ("CO", (4.5709, -74.2973)),
        ("PE", (-9.1900, -75.0152)),
        ("VE", (6.4238, -66.5897)),
        ("PK", (30.3753, 69.3451)),
        ("BD", (23.6850, 90.3563)),
        ("NG", (9.0820, 8.6753)),
        ("ET", (9.1450, 40.4897)),
        ("KR", (35.9078, 127.7669)),
        ("IR", (32.4279, 53.6880)),
        ("IQ", (33.2232, 43.6793)),
        ("AF", (33.9391, 67.7100)),
        ("QA", (25.3548, 51.1839)),
        ("KW", (29.3117, 47.4818)),
        ("OM", (21.4735, 55.9754)),
        ("LB", (33.8547, 35.8623)),
        ("JO", (30.5852, 36.2384)),
        ("SY", (34.8021, 38.9968)),
        ("YE", (15.5527, 48.5164)),
        ("TW", (23.6978, 120.9605)),

        // Full country names (what GDELT returns)
        ("United States", (37.0902, -95.7129)),
        ("United Kingdom", (55.3781, -3.4360)),
        ("Canada", (56.1304, -106.3468)),
        ("Australia", (-25.2744, 133.7751)),
        ("Germany", (51.1657, 10.4515)),
        ("France", (46.2276, 2.2137)),
        ("Italy", (41.8719, 12.5674)),
        ("Spain", (40.4637, -3.7492)),
        ("Russia", (61.5240, 105.3188)),
        ("China", (35.8617, 104.1954)),
        ("Japan", (36.2048, 138.2529)),
        ("India", (20.5937, 78.9629)),
        ("Brazil", (-14.2350, -51.9253)),
        ("Mexico", (23.6345, -102.5528)),
        ("Argentina", (-38.4161, -63.6167)),
        ("South Africa", (-30.5595, 22.9375)),
        ("Egypt", (26.8206, 30.8025)),
        ("Nigeria", (9.0820, 8.6753)),
        ("Kenya", (-0.0236, 37.9062)),
        ("Saudi Arabia", (23.8859, 45.0792)),
        ("United Arab Emirates", (23.4241, 53.8478)),
        ("Turkey", (38.9637, 35.2433)),
        ("Israel", (31.0461, 34.8516)),
        ("Sweden", (60.1282, 18.6435)),
        ("Norway", (60.4720, 8.4689)),
        ("Finland", (61.9241, 25.7482)),
        ("Denmark", (56.2639, 9.5018)),
        ("Netherlands", (52.1326, 5.2913)),
        ("Belgium", (50.5039, 4.4699)),
        ("Switzerland", (46.8182, 8.2275)),
        ("Austria", (47.5162, 14.5501)),
        ("Poland", (51.9194, 19.1451)),
        ("Czech Republic", (49.8175, 15.4730)),
        ("Greece", (39.0742, 21.8243)),
        ("Portugal", (39.3999, -8.2245)),
        ("Ireland", (53.4129, -8.2439)),
        ("New Zealand", (-40.9006, 174.8860)),
        ("Singapore", (1.3521, 103.8198)),
        ("Hong Kong", (22.3193, 114.1694)),
        ("South Korea", (35.9078, 127.7669)),
        ("Thailand", (15.8700, 100.9925)),
        ("Malaysia", (4.2105, 101.9758)),
        ("Indonesia", (-0.7893, 113.9213)),
        ("Philippines", (12.8797, 121.7740)),
        ("Vietnam", (14.0583, 108.2772)),
        ("Ukraine", (48.3794, 31.1656)),
        ("Romania", (45.9432, 24.9668)),
        ("Hungary", (47.1625, 19.5033)),
        ("Chile", (-35.6751, -71.5430)),
        ("Colombia", (4.5709, -74.2973)),
        ("Peru", (-9.1900, -75.0152)),
        ("Venezuela", (6.4238, -66.5897)),
        ("Pakistan", (30.3753, 69.3451)),
        ("Bangladesh", (23.6850, 90.3563)),
        ("Ethiopia", (9.1450, 40.4897)),
        ("Iran", (32.4279, 53.6880)),
        ("Iraq", (33.2232, 43.6793)),
        ("Afghanistan", (33.9391, 67.7100)),
        ("Qatar", (25.3548, 51.1839)),
        ("Kuwait", (29.3117, 47.4818)),
        ("Oman", (21.4735, 55.9754)),
        ("Lebanon", (33.8547, 35.8623)),
        ("Jordan", (30.5852, 36.2384)),
        ("Syria", (34.8021, 38.9968)),
        ("Yemen", (15.5527, 48.5164)),
        ("Taiwan", (23.6978, 120.9605)),
    ].iter().cloned().collect();

    coords.get(country).copied()
}
