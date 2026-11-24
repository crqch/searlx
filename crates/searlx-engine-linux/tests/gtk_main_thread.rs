//! Main-thread GTK shell integration test for `WebKitGtkEngine`.
//!
//! Kept in its own integration-test binary so GTK initializes on the OS/test
//! main thread (`new_on_current_thread`) without conflicting with the shared
//! worker-thread test binary.

#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use searlx_core::{PageUrl, SpaceId};
use searlx_engine::{Engine, EngineEvent, Rect, TabOptions};
use searlx_engine_linux::WebKitGtkEngine;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("searlx-gtk-main-{}", SpaceId::new()));
        std::fs::create_dir_all(&dir).expect("create test temp dir");
        Self(dir)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn gtk_main_thread_shell_can_mount_webview_widget_and_switch_tabs() {
    let temp = TestDir::new();
    let require = std::env::var_os("SEARLX_REQUIRE_WEBKITGTK").is_some()
        || std::env::var_os("DISPLAY").is_some();
    let mut engine =
        match WebKitGtkEngine::with_data_root_on_current_thread(temp.path().to_path_buf()) {
            Ok(engine) => engine,
            Err(err) if !require => {
                eprintln!("skipping GTK main-thread shell test (no display): {err}");
                return;
            }
            Err(err) => panic!("expected GTK main-thread engine with display available: {err}"),
        };

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(960, 640);
    let stage = gtk::Box::new(gtk::Orientation::Vertical, 0);
    window.add(&stage);

    let t1 = engine
        .create_tab(TabOptions {
            url: Some(PageUrl::parse("search://newtab").expect("url 1")),
            ..TabOptions::default()
        })
        .expect("t1");
    let t2 = engine
        .create_tab(TabOptions {
            url: Some(PageUrl::parse("search://settings").expect("url 2")),
            background: true,
            ..TabOptions::default()
        })
        .expect("t2");

    let w1 = engine
        .webview_widget(t1)
        .expect("widget 1 on GTK main thread");
    let w2 = engine
        .webview_widget(t2)
        .expect("widget 2 on GTK main thread");

    if let Some(parent) = w1
        .parent()
        .and_then(|p| p.downcast::<gtk::Container>().ok())
    {
        parent.remove(&w1);
    }
    if let Some(parent) = w2
        .parent()
        .and_then(|p| p.downcast::<gtk::Container>().ok())
    {
        parent.remove(&w2);
    }

    stage.pack_start(&w1, true, true, 0);
    stage.pack_start(&w2, true, true, 0);
    window.show_all();

    engine.set_visible(t1, true).expect("show t1");
    engine.set_visible(t2, false).expect("hide t2");
    engine
        .set_bounds(
            t1,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 960.0,
                height: 640.0,
            },
        )
        .expect("bounds");

    let start = Instant::now();
    let mut finished_t1 = false;
    let mut finished_t2 = false;
    while start.elapsed() < Duration::from_secs(5) {
        let events = engine.pump().expect("pump");
        for ev in events {
            if let EngineEvent::NavigationFinished { tab, .. } = ev {
                if tab == t1 {
                    finished_t1 = true;
                } else if tab == t2 {
                    finished_t2 = true;
                }
            }
        }
        if finished_t1 && finished_t2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        finished_t1 && finished_t2,
        "both internal pages must finish"
    );

    engine.set_visible(t1, false).expect("hide t1");
    engine.set_visible(t2, true).expect("show t2");
    engine.focus(t2).expect("focus t2");
    let snap = engine.snapshot(t2).expect("snapshot t2");
    assert!(snap.png.starts_with(b"\x89PNG\r\n\x1a\n"));

    window.close();
}
