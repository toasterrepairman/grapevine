use gtk::prelude::*;
use gtk::{glib, Label, Orientation, ScrolledWindow, ListBox, SearchEntry};
use std::cell::RefCell;
use std::rc::Rc;
use jetstream_oxide::{
    events::{JetstreamEvent, commit::CommitEvent},
    DefaultJetstreamEndpoints, JetstreamCompression, JetstreamConfig, JetstreamConnector,
};
use atrium_api::record::KnownRecord;
use atrium_api::types::string::Nsid;
use atrium_api::app::bsky::feed::post::RecordData as PostRecord;

use crate::data::{FirehosePost, PostEmbed, PostFacet, FacetType};

#[derive(Clone)]
struct SplitPane {
    container: gtk::Box,
    list: ListBox,
    search_entry: SearchEntry,
    filter_keyword: Rc<RefCell<String>>,
}

#[derive(Clone)]
pub struct FirehoseControl {
    root_container: gtk::Box,
    main_pane: SplitPane,
    splits: Rc<RefCell<Vec<SplitPane>>>,
    message_sender: flume::Sender<FirehosePost>,
    scroll_paused_until: Rc<RefCell<std::time::Instant>>,
}

impl FirehoseControl {
    pub fn add_split(&self) {
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

        let split_scrolled = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();
        split_scrolled.set_child(Some(&split_list));

        // Set up scroll event handler for this split
        let scroll_paused_clone = self.scroll_paused_until.clone();
        let split_vadjustment = split_scrolled.vadjustment();
        split_vadjustment.connect_value_changed(move |_| {
            // Pause for 2 seconds after any scroll activity
            *scroll_paused_clone.borrow_mut() = std::time::Instant::now() + std::time::Duration::from_secs(2);
        });

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

    fn broadcast_message(&self, post: &FirehosePost) {
        let splits = self.splits.borrow();
        for split in splits.iter() {
            let keyword = split.filter_keyword.borrow().clone();
            if !keyword.is_empty() && post.text.to_lowercase().contains(&keyword.to_lowercase()) {
                add_message_to_list(&split.list, post);
            }
        }
    }
}

pub fn create_firehose_view() -> (gtk::Box, FirehoseControl) {
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
    let (tx, rx) = flume::unbounded::<FirehosePost>();
    let main_filter_keyword = Rc::new(RefCell::new(String::new()));

    // Create shared state for scroll pause tracking
    let scroll_paused_until = Rc::new(RefCell::new(std::time::Instant::now()));

    // Set up scroll event handler for main scrolled window
    let scroll_paused_clone = scroll_paused_until.clone();
    let main_vadjustment = main_scrolled.vadjustment();
    main_vadjustment.connect_value_changed(move |_| {
        // Pause for 2 seconds after any scroll activity
        *scroll_paused_clone.borrow_mut() = std::time::Instant::now() + std::time::Duration::from_secs(2);
    });

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
        scroll_paused_until: scroll_paused_until.clone(),
    };

    // Store references for the UI update
    let main_list_clone = main_list.clone();
    let main_filter_keyword_clone = main_filter_keyword.clone();
    let control_clone = control.clone();

    // Create a buffer for batching messages
    let message_buffer = Rc::new(RefCell::new(Vec::new()));
    let message_buffer_clone = message_buffer.clone();

    // Set up receiver to collect incoming posts into buffer
    glib::spawn_future_local(async move {
        while let Ok(post) = rx.recv_async().await {
            message_buffer_clone.borrow_mut().push(post);
        }
    });

    // Set up a timer to process batched messages 5 times per second (every 200ms)
    let scroll_paused_for_timer = scroll_paused_until.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        // Check if we're currently paused due to scrolling
        let is_paused = *scroll_paused_for_timer.borrow() > std::time::Instant::now();

        if !is_paused {
            let mut buffer = message_buffer.borrow_mut();

            if !buffer.is_empty() {
                // Process all buffered posts
                for post in buffer.iter() {
                    // Add to main list if it matches the main filter
                    let main_keyword = main_filter_keyword_clone.borrow().clone();
                    if main_keyword.is_empty() || post.text.to_lowercase().contains(&main_keyword.to_lowercase()) {
                        add_message_to_list(&main_list_clone, post);
                    }

                    // Broadcast to all splits
                    control_clone.broadcast_message(post);
                }

                // Clear the buffer
                buffer.clear();
            }
        }
        // If paused, messages remain in buffer and will be processed after pause ends

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

fn add_message_to_list(list: &ListBox, post: &FirehosePost) {
    // Create main container with card styling (similar to news articles)
    let row = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(6)
        .margin_end(6)
        .build();
    row.add_css_class("firehose-message");

    // Handle embeds first (images, external links)
    if let Some(ref embed) = post.embed {
        match embed {
            PostEmbed::Images { count, alt_texts } => {
                // Create a simple indicator box showing image count and alt text
                let image_indicator = gtk::Box::builder()
                    .orientation(Orientation::Vertical)
                    .spacing(4)
                    .margin_top(6)
                    .margin_bottom(6)
                    .margin_start(8)
                    .margin_end(8)
                    .build();
                image_indicator.add_css_class("popover-currency-section");

                // Image count badge
                let count_badge = Label::builder()
                    .label(&format!("🖼️ {} image{}", count, if *count > 1 { "s" } else { "" }))
                    .xalign(0.0)
                    .build();
                count_badge.add_css_class("badge");
                count_badge.add_css_class("badge-country");
                image_indicator.append(&count_badge);

                // Show alt text if available
                for (i, alt) in alt_texts.iter().enumerate() {
                    if !alt.is_empty() {
                        let alt_label = Label::builder()
                            .label(&format!("[{}] {}", i + 1, alt))
                            .xalign(0.0)
                            .wrap(true)
                            .wrap_mode(gtk::pango::WrapMode::WordChar)
                            .build();
                        alt_label.add_css_class("caption");
                        image_indicator.append(&alt_label);
                    }
                }

                row.append(&image_indicator);
            }
            PostEmbed::External { uri, title, description } => {
                // Create a compact external link preview
                let external_box = gtk::Box::builder()
                    .orientation(Orientation::Vertical)
                    .spacing(4)
                    .margin_top(6)
                    .margin_bottom(6)
                    .margin_start(8)
                    .margin_end(8)
                    .build();
                external_box.add_css_class("popover-currency-section");

                // Link icon/badge
                let link_badge = Label::builder()
                    .label("🔗 External Link")
                    .xalign(0.0)
                    .build();
                link_badge.add_css_class("badge");
                link_badge.add_css_class("badge-lang");
                external_box.append(&link_badge);

                // Link title
                if !title.is_empty() {
                    let link_title = Label::builder()
                        .label(title)
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .lines(1)
                        .build();
                    link_title.add_css_class("caption");
                    external_box.append(&link_title);
                }

                // Link description
                if !description.is_empty() {
                    let link_desc = Label::builder()
                        .label(description)
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .lines(2)
                        .build();
                    link_desc.add_css_class("caption");
                    link_desc.add_css_class("dim-label");
                    external_box.append(&link_desc);
                }

                // Make clickable
                let gesture = gtk::GestureClick::new();
                let uri_clone = uri.clone();
                gesture.connect_released(move |_, _, _, _| {
                    if let Err(e) = open::that(&uri_clone) {
                        eprintln!("Failed to open URL: {}", e);
                    }
                });
                external_box.add_controller(gesture);
                external_box.add_css_class("activatable");

                row.append(&external_box);
            }
            PostEmbed::Video => {
                // Show a video indicator badge
                let video_badge = Label::builder()
                    .label("📹 Video")
                    .margin_start(8)
                    .margin_end(8)
                    .margin_top(6)
                    .margin_bottom(6)
                    .build();
                video_badge.add_css_class("badge");
                video_badge.add_css_class("badge-lang");
                row.append(&video_badge);
            }
        }
    }

    // Content container with padding
    let content_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(8)
        .margin_end(8)
        .build();

    // Create header box for metadata (timestamp and did/rkey)
    let header = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();

    // Timestamp label with monospace font
    let timestamp_label = Label::builder()
        .label(&post.timestamp)
        .xalign(0.0)
        .build();
    timestamp_label.add_css_class("caption");
    timestamp_label.add_css_class("monospace");
    timestamp_label.add_css_class("firehose-timestamp");

    // DID/rkey label with accent color (show last 8 chars of DID + rkey)
    let did_short = if post.did.len() > 12 {
        format!("{}...{}", &post.did[..8], &post.rkey[..8.min(post.rkey.len())])
    } else {
        post.rkey.clone()
    };

    let rkey_label = Label::builder()
        .label(&did_short)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(20)
        .build();
    rkey_label.add_css_class("caption");
    rkey_label.add_css_class("monospace");
    rkey_label.add_css_class("firehose-rkey");

    header.append(&timestamp_label);
    header.append(&rkey_label);
    content_box.append(&header);

    // Show post text
    let message_label = Label::builder()
        .label(&post.text)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .selectable(true)
        .build();
    message_label.add_css_class("firehose-text");
    content_box.append(&message_label);

    // Show facets as badges if present
    if let Some(ref facets) = post.facets {
        if !facets.is_empty() {
            let facets_box = gtk::Box::builder()
                .orientation(Orientation::Horizontal)
                .spacing(4)
                .margin_top(4)
                .build();

            // Count facet types
            let mut mention_count = 0;
            let mut link_count = 0;
            let mut tag_count = 0;

            for facet in facets {
                match &facet.facet_type {
                    FacetType::Mention(_) => mention_count += 1,
                    FacetType::Link(_) => link_count += 1,
                    FacetType::Tag(_) => tag_count += 1,
                }
            }

            // Show count badges
            if mention_count > 0 {
                let badge = Label::builder()
                    .label(&format!("@{}", mention_count))
                    .build();
                badge.add_css_class("badge");
                badge.add_css_class("badge-time");
                facets_box.append(&badge);
            }

            if link_count > 0 {
                let badge = Label::builder()
                    .label(&format!("🔗{}", link_count))
                    .build();
                badge.add_css_class("badge");
                badge.add_css_class("badge-time");
                facets_box.append(&badge);
            }

            if tag_count > 0 {
                let badge = Label::builder()
                    .label(&format!("#{}", tag_count))
                    .build();
                badge.add_css_class("badge");
                badge.add_css_class("badge-time");
                facets_box.append(&badge);
            }

            content_box.append(&facets_box);
        }
    }

    row.append(&content_box);

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

async fn start_jetstream(tx: flume::Sender<FirehosePost>) -> anyhow::Result<()> {
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
                CommitEvent::Create { commit, info } => {
                    if let KnownRecord::AppBskyFeedPost(post) = &commit.record {
                        let timestamp = chrono::Utc::now().format("%H:%M:%S").to_string();

                        // Parse embeds
                        let embed = post.embed.as_ref().and_then(|e| parse_embed(e));

                        // Parse facets
                        let facets = post.facets.as_ref().map(|f| parse_facets(f));

                        let firehose_post = FirehosePost {
                            timestamp,
                            did: info.did.to_string(),
                            rkey: commit.info.rkey.clone(),
                            text: post.text.clone(),
                            embed,
                            facets,
                        };

                        // Send to UI thread
                        if tx.send(firehose_post).is_err() {
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

fn parse_embed(embed: &atrium_api::types::Union<atrium_api::app::bsky::feed::post::RecordEmbedRefs>) -> Option<PostEmbed> {
    use atrium_api::app::bsky::feed::post::RecordEmbedRefs;
    use atrium_api::types::Union;

    match embed {
        Union::Refs(RecordEmbedRefs::AppBskyEmbedImagesMain(images)) => {
            let count = images.images.len();
            if count > 0 {
                // Extract alt text from images
                let alt_texts: Vec<String> = images.images.iter()
                    .map(|img| img.alt.clone())
                    .collect();
                Some(PostEmbed::Images { count, alt_texts })
            } else {
                None
            }
        }
        Union::Refs(RecordEmbedRefs::AppBskyEmbedExternalMain(external)) => {
            Some(PostEmbed::External {
                uri: external.external.uri.clone(),
                title: external.external.title.clone(),
                description: external.external.description.clone(),
            })
        }
        Union::Refs(RecordEmbedRefs::AppBskyEmbedVideoMain(_video)) => {
            Some(PostEmbed::Video)
        }
        _ => None,
    }
}

fn parse_facets(facets: &[atrium_api::app::bsky::richtext::facet::Main]) -> Vec<PostFacet> {
    use atrium_api::app::bsky::richtext::facet::MainFeaturesItem;
    use atrium_api::types::Union;

    let mut parsed_facets = Vec::new();

    for facet in facets {
        let byte_start = facet.index.byte_start as usize;
        let byte_end = facet.index.byte_end as usize;

        // Check features to determine facet type
        for feature in &facet.features {
            let facet_type = match feature {
                Union::Refs(MainFeaturesItem::Mention(mention_data)) => {
                    Some(FacetType::Mention(mention_data.did.to_string()))
                }
                Union::Refs(MainFeaturesItem::Link(link_data)) => {
                    Some(FacetType::Link(link_data.uri.clone()))
                }
                Union::Refs(MainFeaturesItem::Tag(tag_data)) => {
                    Some(FacetType::Tag(tag_data.tag.clone()))
                }
                _ => None,
            };

            if let Some(ft) = facet_type {
                parsed_facets.push(PostFacet {
                    start: byte_start,
                    end: byte_end,
                    facet_type: ft,
                });
            }
        }
    }

    parsed_facets
}
