//! Phase 0 hard-interaction prototype for the Linux GTK shell (P0-07).
//!
//! Five interactions, exercised as far as a locked session allows:
//! 1. A menu popover above a live page.
//! 2. A tab lift panel and a notebook reorder.
//! 3. A split view whose divider position is changed.
//! 4. Emoji in the address field and in page text (IME itself is manual).
//! 5. HTML video fullscreen and picture-in-picture capability.
//!
//! The program prints `INTERACT ...` lines and writes a window snapshot on
//! X11. It calls the same handlers a pointer would, but a programmatic popup or
//! `set_position` is **not** proof that a person can drag or click under the
//! compositor. Those checks stay manual; see `docs/spike/phase-0.md`.

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("searlx-interactions-linux only runs on Linux");
    std::process::ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    linux::run()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use gtk::gdk::prelude::*;
    use gtk::prelude::*;
    use webkit2gtk::{SettingsExt, WebViewExt};

    struct Args {
        video: Option<PathBuf>,
        window_png: PathBuf,
        seconds: u32,
    }

    impl Args {
        fn parse() -> Result<Self, String> {
            let mut args = Self {
                video: None,
                window_png: PathBuf::from("/tmp/searlx-interactions-window.png"),
                seconds: 30,
            };
            let mut iter = std::env::args().skip(1);
            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "--video" => {
                        args.video =
                            Some(PathBuf::from(iter.next().ok_or("--video needs a value")?));
                    }
                    "--window-png" => {
                        args.window_png =
                            PathBuf::from(iter.next().ok_or("--window-png needs a value")?);
                    }
                    "--seconds" => {
                        args.seconds = iter
                            .next()
                            .ok_or("--seconds needs a value")?
                            .parse()
                            .map_err(|_| "--seconds must be a whole number")?;
                    }
                    "--help" | "-h" => {
                        println!(
                            "usage: searlx-interactions-linux [--video PATH]\n\
                             \x20                                 [--window-png PATH]\n\
                             \x20                                 [--seconds N]\n\
                             \n\
                             Run on X11: GDK_BACKEND=x11 DISPLAY=:0 XDG_RUNTIME_DIR=/tmp/run-1000"
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
        failure: RefCell<Option<String>>,
        exit_code: Cell<u8>,
        window_png: PathBuf,
    }

    impl Probe {
        fn fail(&self, reason: String) {
            println!("INTERACT failure {reason}");
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
                eprintln!("searlx-interactions-linux: {error}");
                return std::process::ExitCode::from(2);
            }
        };
        if let Err(error) = gtk::init() {
            eprintln!("searlx-interactions-linux: GTK did not initialise: {error}");
            return std::process::ExitCode::FAILURE;
        }

        let probe = Rc::new(Probe {
            started: Instant::now(),
            failure: RefCell::new(None),
            exit_code: Cell::new(0),
            window_png: args.window_png.clone(),
        });
        log_environment();

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("searlx — Phase 0 interactions");
        window.set_default_size(1180, 780);
        window.set_size_request(640, 420);

        // 1. Address field with an emoji. Real IME composition is manual.
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("Address or search"));
        entry.set_hexpand(true);
        entry.connect_changed(|entry| {
            println!("INTERACT entry_changed text={:?}", entry.text());
        });
        entry.connect_preedit_changed(|_entry, preedit| {
            println!("INTERACT entry_preedit {preedit:?}");
        });

        let menu_button = gtk::MenuButton::new();
        menu_button.set_label("Menu");
        let popover = gtk::Popover::new(Some(&menu_button));
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 4);
        for label in ["New tab", "Split view", "Find on page"] {
            let item = gtk::ModelButton::new();
            item.set_text(Some(label));
            menu.pack_start(&item, false, false, 0);
        }
        popover.add(&menu);
        menu_button.set_popover(Some(&popover));

        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        top.set_margin_top(8);
        top.set_margin_bottom(8);
        top.set_margin_start(8);
        top.set_margin_end(8);
        top.pack_start(&entry, true, true, 0);
        top.pack_start(&menu_button, false, false, 0);

        // 3. Split view: two live pages in a paned container.
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        let left = webkit_page("left");
        let right = webkit_page("right");
        configure_view(&left);
        configure_view(&right);
        paned.pack1(&left, true, true);
        paned.pack2(&right, true, true);
        paned.set_position(560);
        paned.connect_position_notify(|paned| {
            println!("INTERACT split_position position={}", paned.position());
        });
        paned.connect_size_allocate(|paned, allocation| {
            println!(
                "INTERACT split_alloc width={} height={} position={}",
                allocation.width(),
                allocation.height(),
                paned.position()
            );
        });

        // 2. Tab strip. The notebook can reorder tabs; the lift panel is a
        // popup window that a drag gesture would follow.
        let notebook = gtk::Notebook::new();
        notebook.set_scrollable(true);
        let first_tab = gtk::Label::new(Some("Split"));
        notebook.append_page(&paned, Some(&first_tab));
        let second = webkit_page("tab");
        configure_view(&second);
        let second_tab = gtk::Label::new(Some("Page"));
        notebook.append_page(&second, Some(&second_tab));
        notebook.set_tab_reorderable(&paned, true);
        notebook.set_tab_reorderable(&second, true);
        notebook.set_tab_detachable(&paned, true);
        notebook.set_tab_detachable(&second, true);

        // The overlay chip sits above the notebook and the page.
        let overlay = gtk::Overlay::new();
        overlay.add(&notebook);
        let chip = gtk::Button::with_label("overlay above page");
        chip.set_halign(gtk::Align::End);
        chip.set_valign(gtk::Align::Start);
        chip.set_margin_top(14);
        chip.set_margin_end(14);
        chip.set_relief(gtk::ReliefStyle::None);
        chip.connect_clicked(|_| println!("INTERACT overlay_clicked"));
        overlay.add_overlay(&chip);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.pack_start(&top, false, false, 0);
        root.pack_start(&overlay, true, true, 0);
        window.add(&root);
        window.show_all();
        entry.grab_focus();

        let lift = gtk::Window::new(gtk::WindowType::Popup);
        lift.set_decorated(false);
        lift.set_skip_taskbar_hint(true);
        lift.set_default_size(180, 38);
        let lift_label = gtk::Label::new(Some("Moving tab"));
        lift.add(&lift_label);
        lift.set_transient_for(Some(&window));

        // Wait for the left page, then run the scripted sequence. The right
        // page may still be loading; the video probe waits for it.
        let script_probe = probe.clone();
        let script_paned = paned.clone();
        let script_popover = popover.clone();
        let script_menu_button = menu_button.clone();
        let script_notebook = notebook.clone();
        let script_lift = lift.clone();
        let script_window = window.clone();
        let script_left = left.clone();
        let script_right = right.clone();
        let script_entry = entry.clone();
        let left_done = Rc::new(Cell::new(false));
        let left_done_closure = left_done.clone();
        left.connect_load_changed(move |view, event| {
            if event != webkit2gtk::LoadEvent::Finished {
                return;
            }
            if left_done_closure.replace(true) {
                return;
            }
            let view = view.clone();
            let probe = script_probe.clone();
            let paned = script_paned.clone();
            let popover = script_popover.clone();
            let menu_button = script_menu_button.clone();
            let notebook = script_notebook.clone();
            let lift = script_lift.clone();
            let window = script_window.clone();
            let left = script_left.clone();
            let right = script_right.clone();
            let entry = script_entry.clone();
            glib::timeout_add_local_once(Duration::from_millis(400), move || {
                println!(
                    "INTERACT pages_loaded left={:?} right={:?}",
                    view.title(),
                    right.title()
                );
                probe_entry(&entry);
                probe_page_emoji(&probe, &left);
                probe_overlay(&probe, &popover, &menu_button, &window);
                glib::timeout_add_local_once(Duration::from_millis(500), move || {
                    probe_split(&probe, &paned);
                    glib::timeout_add_local_once(Duration::from_millis(500), move || {
                        probe_video(&probe, &right, &window);
                        probe_lift(&lift, &window, &notebook);
                        glib::timeout_add_local_once(Duration::from_millis(1500), move || {
                            probe_finish(&probe, &window);
                        });
                    });
                });
            });
        });

        let fail_probe = probe.clone();
        left.connect_load_failed(move |_view, event, uri, error| {
            fail_probe.fail(format!("left page failed at {event:?} for {uri}: {error}"));
            false
        });
        let fail_probe = probe.clone();
        right.connect_load_failed(move |_view, event, uri, error| {
            fail_probe.fail(format!("right page failed at {event:?} for {uri}: {error}"));
            false
        });

        left.load_html(&page_html("left", 180), None);
        match &args.video {
            Some(video) => {
                println!("INTERACT video_page path={}", video.display());
                // A file base URI gives the page a file origin so the local
                // video is not blocked as a cross-origin subresource.
                right.load_html(&video_html(video), Some("file:///tmp/"));
            }
            None => {
                println!("INTERACT video_page skipped=no-video");
                right.load_html(&page_html("right", 120), None);
            }
        }

        let deadline_probe = probe.clone();
        let seconds = args.seconds;
        glib::timeout_add_seconds_local(seconds, move || {
            deadline_probe.fail(format!("deadline after {seconds}s"));
            gtk::main_quit();
            glib::ControlFlow::Break
        });

        gtk::main();
        if let Some(failure) = probe.failure.borrow().as_ref() {
            println!("INTERACT result=fail reason={failure}");
        } else {
            println!(
                "INTERACT result=ok elapsed_ms={}",
                probe.started.elapsed().as_millis()
            );
        }
        std::process::ExitCode::from(probe.exit_code.get())
    }

    fn webkit_page(id: &str) -> webkit2gtk::WebView {
        let view = webkit2gtk::WebView::new();
        view.set_widget_name(id);
        view
    }

    fn configure_view(view: &webkit2gtk::WebView) {
        if let Some(settings) = WebViewExt::settings(view) {
            settings.set_enable_javascript(true);
            settings.set_enable_webgl(true);
            settings.set_enable_media(true);
            settings.set_enable_media_stream(true);
            settings.set_enable_mediasource(true);
            settings.set_enable_webaudio(true);
            settings.set_enable_page_cache(false);
            // This is a probe: allow the scripted video to autoplay without a
            // real gesture. The product must keep the user-gesture default.
            settings.set_media_playback_requires_user_gesture(false);
            settings.set_media_playback_allows_inline(false);
        }
    }

    fn video_html(video: &std::path::Path) -> String {
        let source = format!("file://{}", video.display());
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <title>video-probe</title><style>body{{font-family:sans-serif;\
             margin:2rem}}video{{width:640px;max-width:100%;background:#000}}</style>\
             </head><body><h1>Video probe</h1>\
             <video id=\"probe-video\" src=\"{source}\" controls autoplay loop muted></video>\
             <p id=\"status\">waiting</p></body></html>"
        )
    }

    fn page_html(id: &str, paragraphs: usize) -> String {
        let mut body = String::new();
        for index in 0..paragraphs {
            body.push_str(&format!(
                "<p id=\"{id}-{index}\">Page {id}, paragraph {index}. \
                 A deterministic line for the split-view probe.</p>\n"
            ));
        }
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <title>probe-{id}</title><style>body{{font-family:sans-serif;\
             max-width:60ch;margin:2rem auto;line-height:1.5}}</style></head>\
             <body><h1>Probe {id}</h1>{body}</body></html>"
        )
    }

    fn probe_entry(entry: &gtk::Entry) {
        entry.set_text("searlx 🙂");
        let text = entry.text();
        println!(
            "INTERACT entry_emoji text={:?} has_emoji={}",
            text,
            text.contains('🙂')
        );
    }

    fn probe_page_emoji(probe: &Rc<Probe>, view: &webkit2gtk::WebView) {
        let script = "(() => { \
            document.body.dataset.emoji = '🙂'; \
            document.title = 'emoji-🙂'; \
            return JSON.stringify({ title: document.title, \
                emoji: document.body.dataset.emoji, \
                chars: document.body.textContent.length }); \
        })()";
        let callback_probe = probe.clone();
        view.evaluate_javascript(
            script,
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            move |result| match result {
                Ok(value) => println!("INTERACT page_emoji {value}"),
                Err(error) => callback_probe.fail(format!("page emoji script failed: {error}")),
            },
        );
    }

    fn probe_overlay(
        probe: &Rc<Probe>,
        popover: &gtk::Popover,
        menu_button: &gtk::MenuButton,
        window: &gtk::Window,
    ) {
        menu_button.set_active(true);
        popover.popup();
        println!(
            "INTERACT overlay_popover visible={} relative_to_menu={}",
            popover.is_visible(),
            popover.relative_to().is_some()
        );
        let snapshot_probe = probe.clone();
        let snapshot_window = window.clone();
        let popover = popover.clone();
        let popover_window = popover.window();
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            save_window_snapshot(&snapshot_probe, &snapshot_window, "overlay");
            if let Some(popover_window) = popover_window {
                save_gdk_window(&snapshot_probe, &popover_window, "popover");
                for (index, child) in popover_window.children().iter().enumerate() {
                    save_gdk_window(&snapshot_probe, child, &format!("popover-child-{index}"));
                }
            }
            popover.popdown();
            let popover = popover.clone();
            glib::timeout_add_local_once(Duration::from_millis(500), move || {
                println!("INTERACT overlay_popdown visible={}", popover.is_visible());
            });
        });
    }

    fn probe_split(probe: &Rc<Probe>, paned: &gtk::Paned) {
        let width = paned.allocated_width();
        let first = width / 2;
        paned.set_position(first);
        println!(
            "INTERACT split_set request={first} actual={} width={width}",
            paned.position()
        );
        let narrow = (width as f64 * 0.3) as i32;
        paned.set_position(narrow);
        println!(
            "INTERACT split_set request={narrow} actual={} width={width}",
            paned.position()
        );
        if paned.position() <= 0 || paned.position() >= width {
            probe.fail(format!(
                "split position {} is outside 0..{width}",
                paned.position()
            ));
        }
    }

    fn probe_video(probe: &Rc<Probe>, view: &webkit2gtk::WebView, window: &gtk::Window) {
        let script = "(() => { \
            const video = document.querySelector('video'); \
            if (!video) return JSON.stringify({ video: false }); \
            const pipEnabled = typeof document.pictureInPictureEnabled === 'boolean' \
                && document.pictureInPictureEnabled; \
            const result = { \
                video: true, \
                pipEnabled: pipEnabled, \
                fullscreenApi: typeof video.requestFullscreen === 'function', \
                pipApi: typeof video.requestPictureInPicture === 'function', \
            }; \
            video.muted = true; \
            const play = video.play ? video.play() : null; \
            if (play && play.then) { \
                play.then(() => { video.dataset.playState = 'ok'; }) \
                    .catch((error) => { video.dataset.playState = 'fail:' + error.name; }); \
            } \
            const full = result.fullscreenApi ? video.requestFullscreen() : null; \
            if (full && full.then) { \
                full.then(() => { document.title = 'fullscreen-ok'; }) \
                    .catch((error) => { document.title = 'fullscreen-fail:' + error.name; }); \
            } \
            if (pipEnabled && result.pipApi) { \
                video.requestPictureInPicture() \
                    .then(() => { document.title = 'pip-ok'; }) \
                    .catch((error) => { document.title = 'pip-fail:' + error.name; }); \
            } \
            return JSON.stringify(result); \
        })()";
        let callback_probe = probe.clone();
        let callback_window = window.clone();
        let callback_view = view.clone();
        view.evaluate_javascript(
            script,
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            move |result| {
                match result {
                    Ok(value) => println!("INTERACT video_capability {value}"),
                    Err(error) => callback_probe.fail(format!("video script failed: {error}")),
                }
                let probe = callback_probe.clone();
                let view = callback_view.clone();
                glib::timeout_add_local_once(Duration::from_millis(900), move || {
                    let read_probe = probe.clone();
                    view.evaluate_javascript(
                        "(() => { const v = document.querySelector('video'); \
                            return JSON.stringify({ title: document.title, \
                                fullscreen: document.fullscreenElement != null, \
                                pip: document.pictureInPictureElement != null, \
                                readyState: v ? v.readyState : -1, \
                                videoWidth: v ? v.videoWidth : 0, \
                                videoHeight: v ? v.videoHeight : 0, \
                                currentTime: v ? v.currentTime : 0, \
                                paused: v ? v.paused : null, \
                                playState: v ? (v.dataset.playState || 'pending') : 'no-video', \
                                error: v && v.error ? v.error.code : null }); })()",
                        None,
                        None,
                        None::<&gtk::gio::Cancellable>,
                        move |result| match result {
                            Ok(value) => println!("INTERACT video_state {value}"),
                            Err(error) => {
                                read_probe.fail(format!("video state read failed: {error}"));
                            }
                        },
                    );
                    save_window_snapshot(&probe, &callback_window, "video");
                    view.evaluate_javascript(
                        "document.fullscreenElement ? document.exitFullscreen() : null",
                        None,
                        None,
                        None::<&gtk::gio::Cancellable>,
                        |_| {},
                    );
                });
            },
        );
    }

    fn probe_lift(lift: &gtk::Window, window: &gtk::Window, notebook: &gtk::Notebook) {
        let (x, y) = window.position();
        lift.move_(x + 320, y + 120);
        lift.show_all();
        println!(
            "INTERACT tab_lift shown={} position={:?}",
            lift.is_visible(),
            lift.position()
        );
        // Reorder as a drop would. A real drag is still manual.
        if let Some(page) = notebook.nth_page(Some(1)) {
            notebook.reorder_child(&page, Some(0));
            println!(
                "INTERACT tab_reorder current={} pages={}",
                notebook.current_page().unwrap_or(0),
                notebook.n_pages()
            );
        }
        let lift = lift.clone();
        glib::timeout_add_local_once(Duration::from_millis(400), move || {
            lift.hide();
            println!("INTERACT tab_lift hidden={}", !lift.is_visible());
        });
    }

    fn probe_finish(probe: &Rc<Probe>, window: &gtk::Window) {
        save_window_snapshot(probe, window, "final");
        log_memory();
        glib::timeout_add_local_once(Duration::from_millis(250), gtk::main_quit);
    }

    fn save_window_snapshot(probe: &Rc<Probe>, window: &gtk::Window, label: &str) {
        let Some(gdk_window) = window.window() else {
            println!("INTERACT snapshot skipped=no-gdk-window label={label}");
            return;
        };
        save_gdk_window(probe, &gdk_window, label);
    }

    fn save_gdk_window(probe: &Rc<Probe>, gdk_window: &gtk::gdk::Window, label: &str) {
        let (width, height) = (gdk_window.width(), gdk_window.height());
        let Some(pixbuf) = gdk_window.pixbuf(0, 0, width, height) else {
            println!(
                "INTERACT snapshot skipped=readback-unavailable label={label} width={width} height={height}"
            );
            return;
        };
        let file_name = probe
            .window_png
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("searlx-interactions.png");
        let path = probe
            .window_png
            .with_file_name(format!("{file_name}-{label}.png"));
        match pixbuf.savev(&path, "png", &[]) {
            Ok(()) => println!(
                "INTERACT snapshot label={label} path={} width={width} height={height}",
                path.display()
            ),
            Err(error) => probe.fail(format!("could not write {}: {error}", path.display())),
        }
    }

    fn log_environment() {
        println!(
            "INTERACT environment display_type={:?} GDK_BACKEND={:?} DISPLAY={:?} WAYLAND_DISPLAY={:?}",
            gtk::gdk::Display::default().map(|display| display.type_().name().to_string()),
            std::env::var("GDK_BACKEND").ok(),
            std::env::var("DISPLAY").ok(),
            std::env::var("WAYLAND_DISPLAY").ok()
        );
    }

    fn log_memory() {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    println!("INTERACT memory {line}");
                }
            }
        }
    }
}
