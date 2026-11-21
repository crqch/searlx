//! Phase 0 Linux spike: can WebKitGTK be hosted and driven on this machine?
//!
//! This binary is evidence-gathering code, not a product engine. It does not
//! pretend to be embedded in gpui: gpui 0.2.2 has no Linux native-view API
//! (`Window::paint_surface` is macOS-only), so this spike measures the GTK
//! shell baseline that ADR-0002 compares against.
//!
//! What it measures, and prints as `SPIKE ...` lines:
//! - GTK/WebKit startup on the selected GDK backend.
//! - Real HTTPS page load and commit/finish timing.
//! - JavaScript execution and a programmatic scroll.
//! - A GTK overlay above the page (the closest equivalent to a browser UI
//!   overlay on this shell).
//! - Resize tracking via `size-allocate`.
//! - WebKit process isolation (child processes).
//! - A page snapshot via `webkit_web_view_get_snapshot`, plus a whole-window
//!   snapshot on X11 where the X server allows readback.
//!
//! It cannot verify real IME input, frame rate, or video without a person at
//! an unlocked display; those stay manual checks in `docs/spike/phase-0.md`.

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("searlx-spike-linux only runs on Linux");
    std::process::ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    linux::run()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use gtk::gdk::prelude::*;
    use gtk::prelude::*;
    use webkit2gtk::{SettingsExt, SnapshotOptions, SnapshotRegion, WebViewExt};

    struct Args {
        url: String,
        page_png: PathBuf,
        window_png: PathBuf,
        seconds: u32,
    }

    impl Args {
        fn parse() -> Result<Self, String> {
            let mut args = Self {
                url: "https://example.com/".to_owned(),
                page_png: PathBuf::from("/tmp/searlx-spike-page.png"),
                window_png: PathBuf::from("/tmp/searlx-spike-window.png"),
                seconds: 30,
            };
            let mut iter = std::env::args().skip(1);
            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "--url" => {
                        args.url = iter.next().ok_or("--url needs a value")?;
                    }
                    "--page-png" => {
                        args.page_png =
                            PathBuf::from(iter.next().ok_or("--page-png needs a value")?);
                    }
                    "--window-png" => {
                        args.window_png =
                            PathBuf::from(iter.next().ok_or("--window-png needs a value")?);
                    }
                    "--seconds" => {
                        let value = iter.next().ok_or("--seconds needs a value")?;
                        args.seconds = value
                            .parse()
                            .map_err(|_| "--seconds must be a whole number")?;
                    }
                    "--help" | "-h" => {
                        println!(
                            "usage: searlx-spike-linux [--url URL] [--page-png PATH]\n\
                             \x20                         [--window-png PATH] [--seconds N]\n\
                             \n\
                             Run with GDK_BACKEND=x11 DISPLAY=:0 (or GDK_BACKEND=wayland\n\
                             WAYLAND_DISPLAY=wayland-0) and an XDG_RUNTIME_DIR."
                        );
                        std::process::exit(0);
                    }
                    other => return Err(format!("unknown argument: {other}")),
                }
            }
            Ok(args)
        }
    }

    struct Probe {
        started: Instant,
        finished: Cell<bool>,
        failure: RefCell<Option<String>>,
        exit_code: Cell<u8>,
        page_png: PathBuf,
        window_png: PathBuf,
    }

    impl Probe {
        fn new(args: &Args) -> Self {
            Self {
                started: Instant::now(),
                finished: Cell::new(false),
                failure: RefCell::new(None),
                exit_code: Cell::new(0),
                page_png: args.page_png.clone(),
                window_png: args.window_png.clone(),
            }
        }

        fn fail(&self, reason: String) {
            println!("SPIKE failure {reason}");
            self.exit_code.set(1);
            if self.failure.borrow().is_none() {
                *self.failure.borrow_mut() = Some(reason);
            }
        }
    }

    pub fn run() -> std::process::ExitCode {
        let args = match Args::parse() {
            Ok(args) => args,
            Err(error) => {
                eprintln!("searlx-spike-linux: {error}");
                return std::process::ExitCode::from(2);
            }
        };

        if let Err(error) = gtk::init() {
            eprintln!("searlx-spike-linux: GTK did not initialise: {error}");
            return std::process::ExitCode::FAILURE;
        }

        let probe = Rc::new(Probe::new(&args));
        log_environment();

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("searlx — Phase 0 Linux spike");
        window.set_default_size(1180, 780);
        window.set_size_request(640, 420);

        let view = webkit2gtk::WebView::new();
        if let Some(settings) = WebViewExt::settings(&view) {
            settings.set_enable_javascript(true);
            settings.set_enable_webgl(true);
            settings.set_enable_media(true);
            settings.set_enable_media_stream(true);
            settings.set_enable_mediasource(true);
            settings.set_enable_webaudio(true);
            settings.set_enable_page_cache(false);
            settings.set_enable_developer_extras(true);
        }

        let overlay = gtk::Overlay::new();
        overlay.add(&view);

        // A GTK overlay above the web view. This is the Linux-shell equivalent
        // of the gpui overlay chip; it proves composition is possible at the
        // toolkit level even though gpui cannot host WebKit yet.
        let chip = gtk::Button::with_label("overlay above page");
        chip.set_halign(gtk::Align::End);
        chip.set_valign(gtk::Align::Start);
        chip.set_margin_top(14);
        chip.set_margin_end(14);
        chip.set_relief(gtk::ReliefStyle::None);
        chip.set_tooltip_text(Some("GTK overlay hit-test probe"));
        chip.connect_clicked(|_| println!("SPIKE overlay_clicked"));
        overlay.add_overlay(&chip);

        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.set_margin_top(8);
        bar.set_margin_bottom(8);
        bar.set_margin_start(8);
        bar.set_margin_end(8);
        let entry = gtk::Entry::new();
        entry.set_text(&args.url);
        entry.set_hexpand(true);
        let go = gtk::Button::with_label("Load");
        bar.pack_start(&entry, true, true, 0);
        bar.pack_start(&go, false, false, 0);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.pack_start(&bar, false, false, 0);
        root.pack_start(&overlay, true, true, 0);
        window.add(&root);
        window.show_all();
        entry.grab_focus();

        let started = Instant::now();
        if let Some(gdk_window) = window.window() {
            println!(
                "SPIKE window_ready ms={} width={} height={}",
                started.elapsed().as_millis(),
                gdk_window.width(),
                gdk_window.height()
            );
        }

        window.connect_size_allocate(|_, allocation| {
            println!(
                "SPIKE window_alloc width={} height={}",
                allocation.width(),
                allocation.height()
            );
        });
        view.clone().connect_size_allocate(|_, allocation| {
            println!(
                "SPIKE webview_alloc width={} height={}",
                allocation.width(),
                allocation.height()
            );
        });

        // Resize once after first paint so the web view's resize path is
        // exercised and visible in the log.
        let resize_window = window.clone();
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            resize_window.resize(900, 620);
            println!("SPIKE resize_requested width=900 height=620");
        });

        let load_probe = probe.clone();
        let load_window = window.clone();
        let overlay_chip = chip.clone();
        view.connect_load_changed(move |view, event| match event {
            webkit2gtk::LoadEvent::Started => {
                println!("SPIKE load_started uri={:?}", view.uri());
            }
            webkit2gtk::LoadEvent::Committed => {
                println!("SPIKE load_committed uri={:?}", view.uri());
            }
            webkit2gtk::LoadEvent::Finished => {
                if load_probe.finished.replace(true) {
                    return;
                }
                let ms = load_probe.started.elapsed().as_millis();
                println!(
                    "SPIKE load_finished ms={ms} title={:?} uri={:?}",
                    view.title(),
                    view.uri()
                );
                log_memory();
                log_children();
                view.grab_focus();
                overlay_chip.clicked();
                println!(
                    "SPIKE focus after_page_grab view={} window={}",
                    view.has_focus(),
                    load_window.has_toplevel_focus()
                );
                probe_javascript(load_probe.clone(), view.clone());
            }
            webkit2gtk::LoadEvent::Redirected => {
                println!("SPIKE load_redirected uri={:?}", view.uri());
            }
            _ => {}
        });

        let fail_probe = probe.clone();
        view.connect_load_failed(move |_view, event, uri, error| {
            fail_probe.fail(format!("load failed at {event:?} for {uri}: {error}"));
            false
        });

        // Hard deadline so a headless CI run can never hang.
        let deadline_probe = probe.clone();
        glib::timeout_add_seconds_local(args.seconds, move || {
            deadline_probe.fail(format!(
                "deadline after {}s (finished={})",
                args.seconds,
                deadline_probe.finished.get()
            ));
            gtk::main_quit();
            glib::ControlFlow::Break
        });

        let reload_entry = entry.clone();
        let reload_view = view.clone();
        go.connect_clicked(move |_| {
            let text = reload_entry.text();
            if !text.trim().is_empty() {
                reload_view.load_uri(text.trim());
            }
        });
        let enter_view = view.clone();
        entry.connect_activate(move |entry| {
            let text = entry.text();
            if !text.trim().is_empty() {
                enter_view.load_uri(text.trim());
            }
        });

        println!("SPIKE load_requested uri={:?}", args.url);
        view.load_uri(&args.url);
        gtk::main();

        if let Some(failure) = probe.failure.borrow().as_ref() {
            println!("SPIKE result=fail reason={failure}");
        } else {
            println!("SPIKE result=ok");
        }
        std::process::ExitCode::from(probe.exit_code.get())
    }

    fn probe_javascript(probe: Rc<Probe>, view: webkit2gtk::WebView) {
        // The page is short, so append a tall element to exercise compositing
        // and scrolling rather than measuring the site's own layout.
        let script = "(() => { \
            let d = document.getElementById('searlx-probe'); \
            if (!d) { d = document.createElement('div'); d.id = 'searlx-probe'; \
                d.style.height = '4000px'; d.style.background = '#e8eef7'; \
                document.body.appendChild(d); } \
            window.scrollTo(0, 240); \
            return JSON.stringify({ scrollY: window.scrollY, \
                bodyHeight: document.body.scrollHeight, \
                title: document.title, focused: document.hasFocus() }); \
        })()";
        let scroll_probe = probe.clone();
        let scroll_view = view.clone();
        glib::timeout_add_local_once(Duration::from_millis(200), move || {
            scroll_view.evaluate_javascript(
                script,
                None,
                None,
                None::<&gtk::gio::Cancellable>,
                move |result| match result {
                    Ok(value) => println!("SPIKE js_result {value}"),
                    Err(error) => {
                        scroll_probe.fail(format!("javascript failed: {error}"));
                    }
                },
            );
        });

        let window_probe = probe;
        let snapshot_view = view.clone();
        glib::timeout_add_local_once(Duration::from_millis(700), move || {
            take_page_snapshot(window_probe, snapshot_view);
        });
    }

    fn take_page_snapshot(probe: Rc<Probe>, view: webkit2gtk::WebView) {
        let window = view.toplevel();
        let path = probe.page_png.clone();
        let snapshot_probe = probe.clone();
        view.snapshot(
            SnapshotRegion::Visible,
            SnapshotOptions::NONE,
            None::<&gtk::gio::Cancellable>,
            move |result| match result {
                Ok(surface) => {
                    let mut bytes = Vec::new();
                    match surface.write_to_png(&mut bytes) {
                        Ok(()) => match std::fs::write(&path, &bytes) {
                            Ok(()) => println!(
                                "SPIKE page_snapshot path={} bytes={}",
                                path.display(),
                                bytes.len()
                            ),
                            Err(error) => snapshot_probe
                                .fail(format!("could not write {}: {error}", path.display())),
                        },
                        Err(error) => {
                            snapshot_probe.fail(format!("page PNG encode failed: {error}"));
                        }
                    }
                }
                Err(error) => snapshot_probe.fail(format!("page snapshot failed: {error}")),
            },
        );

        let window_probe = probe;
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            if let Some(window) = window.and_downcast::<gtk::Window>() {
                save_window_snapshot(window_probe.clone(), &window);
            } else {
                println!("SPIKE window_snapshot skipped=no-toplevel");
            }
            log_memory();
            glib::timeout_add_local_once(Duration::from_millis(250), gtk::main_quit);
        });
    }

    fn save_window_snapshot(probe: Rc<Probe>, window: &gtk::Window) {
        let Some(gdk_window) = window.window() else {
            println!("SPIKE window_snapshot skipped=no-gdk-window");
            return;
        };
        let (width, height) = (gdk_window.width(), gdk_window.height());
        let Some(pixbuf) = gdk_window.pixbuf(0, 0, width, height) else {
            println!(
                "SPIKE window_snapshot skipped=readback-unavailable width={width} height={height}"
            );
            return;
        };
        let path = probe.window_png.clone();
        match pixbuf.savev(&path, "png", &[]) {
            Ok(()) => println!(
                "SPIKE window_snapshot path={} width={width} height={height}",
                path.display()
            ),
            Err(error) => probe.fail(format!("could not write {}: {error}", path.display())),
        }
    }

    fn log_environment() {
        let display = gtk::gdk::Display::default();
        let display_type = display
            .as_ref()
            .map(|display| display.type_().name().to_string())
            .unwrap_or_else(|| "none".to_owned());
        println!(
            "SPIKE environment display_type={display_type} GDK_BACKEND={:?} DISPLAY={:?} WAYLAND_DISPLAY={:?} XDG_SESSION_TYPE={:?}",
            std::env::var("GDK_BACKEND").ok(),
            std::env::var("DISPLAY").ok(),
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        );
        println!(
            "SPIKE environment disable_compositing={:?}",
            std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").ok()
        );
    }

    fn log_memory() {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    println!("SPIKE memory {line}");
                }
            }
        }
    }

    fn log_children() {
        let mut children = Vec::new();
        if let Ok(tasks) = std::fs::read_dir("/proc/self/task") {
            for task in tasks.flatten() {
                let path = task.path().join("children");
                if let Ok(text) = std::fs::read_to_string(path) {
                    for pid in text
                        .split_whitespace()
                        .filter_map(|pid| pid.parse::<u32>().ok())
                    {
                        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                            .map_or_else(|_| "?".to_owned(), |comm| comm.trim().to_owned());
                        children.push((pid, comm));
                    }
                }
            }
        }
        children.sort_unstable();
        children.dedup();
        for (pid, comm) in children {
            println!("SPIKE child pid={pid} comm={comm}");
        }
    }

    // Keeps `Path` in scope for the doc links above without exposing it.
    #[allow(dead_code)]
    fn _path_marker(_: &Path) {}
}
