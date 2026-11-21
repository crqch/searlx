//! Phase 0 Linux benchmark: create N WebKitGTK tabs, load a deterministic local
//! page, switch between them, and report time and memory.
//!
//! This is evidence code for P0-08, not the product engine. It runs in a GTK
//! shell because gpui 0.2.2 has no Linux native-view API (ADR-0002).
//!
//! Output is `BENCH ...` lines. Run one tab count per process so memory is not
//! polluted by an earlier run:
//!
//! ```sh
//! GDK_BACKEND=x11 DISPLAY=:0 XDG_RUNTIME_DIR=/tmp/run-1000 \
//!   ./target/debug/searlx-bench-linux --tabs 1
//! ```
//!
//! Load time is from the first `load_html` call to the last `Finished` signal,
//! so it includes WebKit process startup but not GTK startup (logged
//! separately). Switch time is the time to change the notebook page and drain
//! pending GTK events; it is not a measured frame rate.

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("searlx-bench-linux only runs on Linux");
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

    use gtk::prelude::*;
    use webkit2gtk::{SettingsExt, WebViewExt};

    struct Args {
        tabs: u32,
        cycles: u32,
        seconds: u32,
        url: Option<String>,
    }

    impl Args {
        fn parse() -> Result<Self, String> {
            let mut args = Self {
                tabs: 1,
                cycles: 20,
                seconds: 45,
                url: None,
            };
            let mut iter = std::env::args().skip(1);
            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "--tabs" => {
                        args.tabs = iter
                            .next()
                            .ok_or("--tabs needs a value")?
                            .parse()
                            .map_err(|_| "--tabs must be a positive whole number")?;
                        if args.tabs == 0 {
                            return Err("--tabs must be at least 1".to_owned());
                        }
                    }
                    "--cycles" => {
                        args.cycles = iter
                            .next()
                            .ok_or("--cycles needs a value")?
                            .parse()
                            .map_err(|_| "--cycles must be a whole number")?;
                    }
                    "--seconds" => {
                        args.seconds = iter
                            .next()
                            .ok_or("--seconds needs a value")?
                            .parse()
                            .map_err(|_| "--seconds must be a whole number")?;
                    }
                    "--url" => {
                        args.url = Some(iter.next().ok_or("--url needs a value")?);
                    }
                    "--help" | "-h" => {
                        println!(
                            "usage: searlx-bench-linux [--tabs N] [--cycles N]\n\
                             \x20                         [--seconds N] [--url URL]\n\
                             \n\
                             Default: 1 tab, 20 switch cycles, 45 s deadline, local page.\n\
                             Run once per tab count for a clean memory reading."
                        );
                        std::process::exit(0);
                    }
                    other => return Err(format!("unknown argument: {other}")),
                }
            }
            Ok(args)
        }
    }

    struct Bench {
        started: Cell<Instant>,
        tabs: u32,
        finished: Cell<u32>,
        per_tab_ms: RefCell<Vec<(u32, u128)>>,
        failure: RefCell<Option<String>>,
    }

    impl Bench {
        fn new(tabs: u32) -> Self {
            Self {
                started: Cell::new(Instant::now()),
                tabs,
                finished: Cell::new(0),
                per_tab_ms: RefCell::new(Vec::new()),
                failure: RefCell::new(None),
            }
        }

        fn fail(&self, reason: String) {
            println!("BENCH failure {reason}");
            if self.failure.borrow().is_none() {
                *self.failure.borrow_mut() = Some(reason);
            }
        }
    }

    pub fn run() -> std::process::ExitCode {
        let args = match Args::parse() {
            Ok(args) => args,
            Err(error) => {
                eprintln!("searlx-bench-linux: {error}");
                return std::process::ExitCode::from(2);
            }
        };

        let process_started = Instant::now();
        if let Err(error) = gtk::init() {
            eprintln!("searlx-bench-linux: GTK did not initialise: {error}");
            return std::process::ExitCode::FAILURE;
        }
        println!(
            "BENCH gtk_init ms={} tabs={} cycles={} mode={}",
            process_started.elapsed().as_millis(),
            args.tabs,
            args.cycles,
            if args.url.is_some() { "url" } else { "local" }
        );

        let bench = Rc::new(Bench::new(args.tabs));
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("searlx — Phase 0 Linux benchmark");
        window.set_default_size(1180, 780);
        window.set_size_request(640, 420);

        let notebook = gtk::Notebook::new();
        notebook.set_show_border(false);
        notebook.set_scrollable(true);

        let create_started = Instant::now();
        let mut views = Vec::with_capacity(args.tabs as usize);
        for index in 0..args.tabs {
            let view = webkit2gtk::WebView::new();
            configure_view(&view);
            let label = gtk::Label::new(Some(&format!("{}", index + 1)));
            notebook.append_page(&view, Some(&label));
            views.push(view);
        }
        let create_ms = create_started.elapsed().as_millis();
        println!("BENCH tabs_created count={} ms={create_ms}", args.tabs);

        window.add(&notebook);
        window.show_all();
        notebook.set_current_page(Some(0));
        println!(
            "BENCH window_ready ms={} width={} height={}",
            process_started.elapsed().as_millis(),
            window.window().map_or(0, |window| window.width()),
            window.window().map_or(0, |window| window.height())
        );

        // Connect after the views exist so each index is captured.
        for (index, view) in views.iter().enumerate() {
            let finished_bench = bench.clone();
            let finished_notebook = notebook.clone();
            let cycles = args.cycles;
            let tabs = args.tabs;
            view.connect_load_changed(move |_view, event| {
                if event != webkit2gtk::LoadEvent::Finished {
                    return;
                }
                let elapsed = finished_bench.started.get().elapsed().as_millis();
                finished_bench
                    .per_tab_ms
                    .borrow_mut()
                    .push((index as u32, elapsed));
                let done = finished_bench.finished.get() + 1;
                finished_bench.finished.set(done);
                if done != finished_bench.tabs {
                    return;
                }
                let samples = finished_bench.per_tab_ms.borrow();
                let sum: u128 = samples.iter().map(|(_, ms)| *ms).sum();
                let average = sum / u128::from(samples.len() as u64);
                let max = samples.iter().map(|(_, ms)| *ms).max().unwrap_or(0);
                let min = samples.iter().map(|(_, ms)| *ms).min().unwrap_or(0);
                println!(
                    "BENCH all_finished tabs={} ms={elapsed} per_tab_avg_ms={average} per_tab_min_ms={min} per_tab_max_ms={max}",
                    finished_bench.tabs
                );
                log_memory("after_load", tabs);
                log_processes();
                run_switch_bench(&finished_notebook, tabs, cycles);
            });
        }

        let fail_bench = bench.clone();
        for (index, view) in views.iter().enumerate() {
            let fail_bench = fail_bench.clone();
            view.connect_load_failed(move |_view, event, uri, error| {
                fail_bench.fail(format!(
                    "tab {index} load failed at {event:?} for {uri}: {error}"
                ));
                false
            });
        }

        let deadline_bench = bench.clone();
        let seconds = args.seconds;
        glib::timeout_add_seconds_local(seconds, move || {
            deadline_bench.fail(format!(
                "deadline after {seconds}s (finished={}/{})",
                deadline_bench.finished.get(),
                deadline_bench.tabs
            ));
            gtk::main_quit();
            glib::ControlFlow::Break
        });

        bench.started.set(Instant::now());
        println!("BENCH load_started tabs={}", args.tabs);
        for (index, view) in views.iter().enumerate() {
            match &args.url {
                Some(url) => view.load_uri(url),
                None => view.load_html(&bench_html(index as u32), None),
            }
        }

        gtk::main();

        if let Some(failure) = bench.failure.borrow().as_ref() {
            println!("BENCH result=fail reason={failure}");
            std::process::ExitCode::FAILURE
        } else {
            log_memory("final", args.tabs);
            println!("BENCH result=ok tabs={}", args.tabs);
            std::process::ExitCode::SUCCESS
        }
    }

    fn configure_view(view: &webkit2gtk::WebView) {
        if let Some(settings) = WebViewExt::settings(view) {
            settings.set_enable_javascript(true);
            settings.set_enable_webgl(true);
            settings.set_enable_media(true);
            settings.set_enable_page_cache(false);
            settings.set_enable_developer_extras(false);
        }
    }

    fn bench_html(index: u32) -> String {
        let mut paragraphs = String::new();
        for paragraph in 0..240 {
            paragraphs.push_str(&format!(
                "<p id=\"p{paragraph}\">Tab {index}, paragraph {paragraph}. \
                 The quick brown fox jumps over the lazy dog. \
                 This sentence exists to give the page a stable height.</p>\n"
            ));
        }
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <title>bench-{index}</title>\
             <style>body{{font-family:sans-serif;max-width:62ch;margin:2rem auto;line-height:1.5}}\
             h1{{font-size:1.4rem}}</style></head><body>\
             <h1>Bench tab {index}</h1>{paragraphs}</body></html>"
        )
    }

    fn run_switch_bench(notebook: &gtk::Notebook, tabs: u32, cycles: u32) {
        if cycles == 0 {
            println!("BENCH switch skipped=cycles-zero");
            schedule_quit();
            return;
        }
        let switch_started = Instant::now();
        let mut samples = Vec::with_capacity((tabs * cycles) as usize);
        for _ in 0..cycles {
            for index in 0..tabs {
                let started = Instant::now();
                notebook.set_current_page(Some(index));
                drain_events();
                samples.push(started.elapsed().as_micros());
            }
        }
        let total = switch_started.elapsed();
        let sum: u128 = samples.iter().sum();
        let average = sum / u128::from(samples.len() as u64);
        let max = samples.iter().copied().max().unwrap_or(0);
        let min = samples.iter().copied().min().unwrap_or(0);
        println!(
            "BENCH switch count={} total_us={} avg_us={average} min_us={min} max_us={max}",
            samples.len(),
            total.as_micros()
        );
        log_memory("after_switch", tabs);
        schedule_quit();
    }

    fn drain_events() {
        // Bound the drain so a busy page cannot hang the benchmark forever.
        for _ in 0..10_000 {
            if !gtk::events_pending() {
                return;
            }
            gtk::main_iteration_do(false);
        }
    }

    fn schedule_quit() {
        glib::timeout_add_local_once(Duration::from_millis(250), gtk::main_quit);
    }

    fn log_memory(label: &str, tabs: u32) {
        let mut pids = Vec::new();
        collect_processes(std::process::id(), 0, &mut pids);
        pids.sort_unstable();
        pids.dedup();
        let mut total_kb = 0_u64;
        let mut measured = 0_u32;
        for pid in &pids {
            if let Some(kb) = read_rss_kb(*pid) {
                total_kb = total_kb.saturating_add(kb);
                measured = measured.saturating_add(1);
            }
        }
        let self_kb = read_rss_kb(std::process::id()).unwrap_or(0);
        println!(
            "BENCH memory label={label} tabs={tabs} self_kb={self_kb} total_kb={total_kb} processes={} measured={measured}",
            pids.len()
        );
    }

    fn log_processes() {
        let mut pids = Vec::new();
        collect_processes(std::process::id(), 0, &mut pids);
        pids.sort_unstable();
        pids.dedup();
        for pid in pids {
            let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .map_or_else(|_| "?".to_owned(), |comm| comm.trim().to_owned());
            let rss = read_rss_kb(pid).unwrap_or(0);
            println!("BENCH process pid={pid} comm={comm} rss_kb={rss}");
        }
    }

    fn collect_processes(pid: u32, depth: u32, out: &mut Vec<u32>) {
        if depth > 8 {
            return;
        }
        out.push(pid);
        for child in read_children(pid) {
            collect_processes(child, depth + 1, out);
        }
    }

    fn read_children(pid: u32) -> Vec<u32> {
        let mut children = Vec::new();
        let tasks = match std::fs::read_dir(format!("/proc/{pid}/task")) {
            Ok(tasks) => tasks,
            Err(_) => return children,
        };
        for task in tasks.flatten() {
            let path: PathBuf = task.path().join("children");
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            for child in text
                .split_whitespace()
                .filter_map(|value| value.parse::<u32>().ok())
            {
                children.push(child);
            }
        }
        children.sort_unstable();
        children.dedup();
        children
    }

    fn read_rss_kb(pid: u32) -> Option<u64> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                return rest.split_whitespace().next()?.parse::<u64>().ok();
            }
        }
        None
    }
}
