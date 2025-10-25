use gtk::prelude::*;
use gtk::{glib, Application, Label, Orientation, ScrolledWindow, Align};
use libadwaita::{prelude::*, ViewSwitcher, HeaderBar, ToolbarView, ApplicationWindow, ViewStack, StyleManager, ColorScheme};

const APP_ID: &str = "com.example.Grapevine";

fn main() -> glib::ExitCode {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(build_ui);
    app.run()
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
            backdrop-filter: blur(10px);
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
    let container = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .build();

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
            viewport.set_max_zoom_level(6);
        }

        // Set initial zoom level to 2 (good overview of world)
        map_view.go_to_full(0.0, 0.0, 2.0);
    }

    // Make the map expand to fill the space
    map.set_vexpand(true);
    map.set_hexpand(true);

    container.append(&map);
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
