mod app;
mod bench;
mod geometry;
mod images;
mod input;
mod model;
mod paint;
mod smoke;
mod snapping;
mod storage;
mod stroke;
mod svg;
#[cfg(target_os = "linux")]
mod tablet;

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

fn main() {
    let mut path = None;
    let mut smoke = false;
    let mut bench = false;
    for arg in std::env::args_os().skip(1) {
        if arg == "--smoke-test" {
            smoke = true;
        } else if arg == "--bench" {
            bench = true;
        } else if arg == "--help" || arg == "-h" {
            println!(
                "Romo — a native GPUI whiteboard\n\nUsage: romo [board.romo]\n\nOpen a board file, or restore your previous local session."
            );
            return;
        } else {
            path = Some(std::path::PathBuf::from(arg));
        }
    }
    let opened = path.is_some();
    Application::new()
        .with_assets(paint::Assets)
        .run(move |cx: &mut App| {
            cx.text_system()
                .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                    "../assets/InterVariable.ttf"
                ))])
                .expect("Could not load Romo's bundled font");
            input::bind_keys(cx);
            let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(gpui::TitlebarOptions {
                            title: Some("Romo".into()),
                            ..Default::default()
                        }),
                        app_id: Some("romo".into()),
                        window_min_size: Some(size(px(800.0), px(640.0))),
                        ..Default::default()
                    },
                    // The benchmark, like the smoke test, never touches saved boards.
                    move |window, cx| cx.new(|cx| app::Romo::new(path, smoke || bench, window, cx)),
                )
                .expect("Could not open the Romo window");
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            #[cfg(target_os = "linux")]
            tablet::watch(window, cx);
            if smoke {
                smoke::run(window, cx);
            } else if bench {
                bench::run(window, opened, cx);
            }
            cx.activate(true);
        });
}
