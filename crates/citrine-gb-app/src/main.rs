#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
const APP_ID: &str = "io.github.zitronenjoghurt.citrine";

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    citrine_gb_app::game_report::log_ring::install(None, log::LevelFilter::Info);
    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_maximized(true)
            .with_drag_and_drop(true)
            .with_title("Citrine")
            .with_app_id(APP_ID)
            .with_icon(load_icon()),
        persistence_path: Some(citrine_gb_app::app_state_file(APP_ID)),
        persist_window: true,
        wgpu_options: citrine_gb_app::wgpu_options(),
        ..Default::default()
    };

    eframe::run_native(
        "Citrine",
        native_options,
        Box::new(|cc| Ok(Box::new(citrine_gb_app::Citrine::new(cc)))),
    )
    .expect("Failed to run egui application.");
}

#[cfg(not(target_arch = "wasm32"))]
fn load_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon-512x512.png");
    let image = image::load_from_memory(icon_bytes)
        .expect("Failed to load icon")
        .into_rgba8();
    let (width, height) = image.dimensions();

    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;
    let web_options = eframe::WebOptions::default();

    citrine_gb_app::game_report::log_ring::install(
        Some(Box::new(eframe::WebLogger::new(log::LevelFilter::Debug))),
        log::LevelFilter::Debug,
    );

    citrine_gb_threads::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("app_canvas")
            .expect("Failed to find app_canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("app_canvas was not a HtmlCanvasElement");

        citrine_gb_app::open_saves().await;

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(citrine_gb_app::Citrine::new(cc)))),
            )
            .await;

        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}
