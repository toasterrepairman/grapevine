mod data;
mod coordinates;
mod global_affairs;
mod firehose;

use gtk::prelude::*;
use gtk::{glib, Application, Label, Orientation, Align};
use libadwaita::{prelude::*, ViewSwitcher, HeaderBar, ToolbarView, ApplicationWindow, ViewStack, StyleManager, ColorScheme};
use std::cell::RefCell;
use std::rc::Rc;
use chrono_tz::Tz;

use data::APP_ID;
use global_affairs::create_global_affairs_view;
use firehose::create_firehose_view;

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
    let results_list_ref = Rc::new(RefCell::new(None::<gtk::ListBox>));
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

    // Create header bar (now a statusline)
    let header_bar = HeaderBar::builder()
        .build();

    // Create time/timezone display with monospace font (centered as title)
    let time_label = Label::builder()
        .label("Loading...")
        .build();
    time_label.add_css_class("monospace");
    time_label.add_css_class("time-display");

    // State to track 12/24 hour format (default to 12-hour)
    let use_12_hour = Rc::new(RefCell::new(true));

    // Make the time label clickable to toggle between 12/24 hour format
    let time_label_gesture = gtk::GestureClick::new();
    let use_12_hour_clone = use_12_hour.clone();
    time_label_gesture.connect_released(move |_, _, _, _| {
        let mut is_12_hour = use_12_hour_clone.borrow_mut();
        *is_12_hour = !*is_12_hour;
    });
    time_label.add_controller(time_label_gesture);

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
            // We need to call fetch_gdelt_articles, but it's in the global_affairs module
            // For now, this will just trigger a refresh via the search entry
            eprintln!("Refresh triggered for query: {}", query);
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

    // Pack widgets into headerbar
    header_bar.pack_start(&refresh_button);
    header_bar.set_title_widget(Some(&time_label));
    header_bar.pack_end(&plus_button);

    // Update time every second using local timezone with proper abbreviation
    let time_label_clone = time_label.clone();

    // Get system timezone using iana-time-zone
    let tz: Tz = iana_time_zone::get_timezone()
        .ok()
        .and_then(|tz_str| {
            eprintln!("Detected timezone: {}", tz_str);
            tz_str.parse().ok()
        })
        .unwrap_or_else(|| {
            eprintln!("Failed to detect timezone, using UTC");
            chrono_tz::UTC
        });

    let use_12_hour_for_timer = use_12_hour.clone();
    glib::timeout_add_seconds_local(1, move || {
        let now = chrono::Utc::now().with_timezone(&tz);

        // Choose format based on current setting
        let time_str = if *use_12_hour_for_timer.borrow() {
            // 12-hour format with AM/PM
            now.format("%I:%M:%S %p %Z").to_string()
        } else {
            // 24-hour format
            now.format("%H:%M:%S %Z").to_string()
        };

        time_label_clone.set_label(&time_str);
        glib::ControlFlow::Continue
    });

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

    // Add Ctrl+Q keyboard shortcut to close the window
    let quit_action = gtk::gio::SimpleAction::new("quit", None);
    let window_weak = window.downgrade();
    quit_action.connect_activate(move |_, _| {
        if let Some(window) = window_weak.upgrade() {
            window.close();
        }
    });
    app.add_action(&quit_action);
    app.set_accels_for_action("app.quit", &["<Primary>q"]);

    // Load custom CSS for floating switcher, map markers, statusline, firehose messages, and news articles
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
        }
        .time-display {
            font-size: 13px;
            font-weight: 600;
            padding: 4px 12px;
            background-color: alpha(@accent_bg_color, 0.15);
            border-radius: 6px;
        }
        .firehose-message {
            background-color: alpha(@card_bg_color, 0.5);
            border-radius: 8px;
            padding: 3px 4px;
            border: 1px solid alpha(@borders, 0.5);
        }
        .firehose-timestamp {
            color: alpha(@window_fg_color, 0.55);
        }
        .firehose-rkey {
            color: @accent_color;
            font-weight: 600;
        }
        .firehose-text {
            line-height: 1.4;
        }
        .news-article-card {
            background-color: @card_bg_color;
            border-radius: 12px;
            overflow: hidden;
            border: 1px solid alpha(@borders, 0.2);
            transition: all 200ms cubic-bezier(0.4, 0, 0.2, 1);
        }
        .news-article-card:hover {
            border-color: alpha(@accent_bg_color, 0.3);
            box-shadow: 0 4px 12px alpha(black, 0.12);
            transform: translateY(-2px);
        }
        .article-thumbnail {
            background-color: alpha(@window_bg_color, 0.3);
            height: 140px;
            border-radius: 8px;
            margin: 8px;
        }
        .article-title {
            font-size: 14px;
            font-weight: 600;
            line-height: 1.35;
            color: @window_fg_color;
        }
        .article-domain {
            font-size: 11px;
            font-weight: 500;
            color: alpha(@window_fg_color, 0.5);
            margin-top: 2px;
        }
        .badge {
            background-color: alpha(@accent_bg_color, 0.15);
            border-radius: 6px;
            padding: 3px 8px;
            font-size: 10px;
            font-weight: 600;
            min-height: 0;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        .badge-country {
            background-color: alpha(@accent_bg_color, 0.25);
            color: @accent_fg_color;
            transition: all 150ms ease;
        }
        .badge-country:hover {
            background-color: @accent_bg_color;
            box-shadow: 0 2px 6px alpha(@accent_bg_color, 0.4);
        }
        .badge-time {
            background-color: alpha(@window_fg_color, 0.08);
            color: alpha(@window_fg_color, 0.7);
        }
        .badge-lang {
            background-color: alpha(@warning_bg_color, 0.2);
            color: @warning_fg_color;
        }
        .badge-positive {
            background-color: alpha(@success_bg_color, 0.2);
            color: @success_fg_color;
        }
        .badge-negative {
            background-color: alpha(@error_bg_color, 0.2);
            color: @error_fg_color;
        }
        .badge-neutral {
            background-color: alpha(@window_fg_color, 0.08);
            color: alpha(@window_fg_color, 0.7);
        }
        .popover-currency-section {
            padding: 8px;
            background-color: alpha(@accent_bg_color, 0.08);
            border-radius: 8px;
            border: 1px solid alpha(@accent_bg_color, 0.15);
        }
        .currency-rate {
            font-family: monospace;
            color: @accent_color;
            font-weight: 700;
        }
        .currency-change-positive {
            color: @success_color;
        }
        .currency-change-negative {
            color: @error_color;
        }
        .popover-article-row {
            background-color: alpha(@card_bg_color, 0.3);
            border-radius: 6px;
            border: 1px solid alpha(@borders, 0.15);
            transition: all 150ms ease;
        }
        .popover-article-row:hover {
            background-color: alpha(@card_bg_color, 0.6);
            border-color: alpha(@accent_bg_color, 0.3);
            box-shadow: 0 2px 6px alpha(black, 0.08);
        }
        .popover-article-title {
            font-size: 13px;
            font-weight: 600;
            line-height: 1.3;
        }
        .popover-article-meta {
            font-size: 11px;
            color: alpha(@window_fg_color, 0.55);
        }
        .popover-article-time {
            font-size: 10px;
            color: alpha(@window_fg_color, 0.45);
            font-weight: 500;
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
