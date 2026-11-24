//! Real WebKitGTK contract tests (`ADR-0002`, `AGENTS.md` §8).
//!
//! Run against an X11/Xvfb/Xvnc display (`GDK_BACKEND=x11`) so real
//! `WebKitWebView` / `WebKitWebProcess` / `WebKitUserContentManager` /
//! `WebKitUserContentFilterStore` / `WebKitFindController` / `WebKitDownload`
//! paths execute end-to-end against a local `127.0.0.1` HTTP server.

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use searlx_core::{PageUrl, ProfileId, SpaceId, compile_webkit_content_rules};
use searlx_engine::{
    DataKinds, DialogKind, Engine, EngineError, EngineEvent, FindOptions, InjectAt, PermissionKind,
    Rect, ScriptWorld, TabOptions,
};
use searlx_engine_linux::{InternalPageResponse, WebKitGtkEngine};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("searlx-wk-test-{}", SpaceId::new()));
        std::fs::create_dir_all(&dir).expect("create test temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn try_engine_for_contract_test(root: &Path) -> Option<WebKitGtkEngine> {
    let require = std::env::var_os("SEARLX_REQUIRE_WEBKITGTK").is_some()
        || std::env::var_os("DISPLAY").is_some();
    match WebKitGtkEngine::with_data_root(root.to_path_buf()) {
        Ok(engine) => Some(engine),
        Err(err) if !require => {
            eprintln!("skipping live WebKitGTK contract test (no display): {err}");
            None
        }
        Err(err) => panic!("expected live WebKitGTK engine with display available: {err}"),
    }
}

fn pump_until(
    engine: &mut WebKitGtkEngine,
    timeout: Duration,
    mut predicate: impl FnMut(&[EngineEvent]) -> bool,
) -> Vec<EngineEvent> {
    let start = Instant::now();
    let mut collected = Vec::new();
    while start.elapsed() < timeout {
        let batch = engine.pump().expect("engine pump");
        if !batch.is_empty() {
            collected.extend(batch);
            if predicate(&collected) {
                return collected;
            }
        }
        thread::sleep(Duration::from_millis(15));
    }
    collected
}

struct TestServer {
    base_url: String,
    tracker_hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener.set_nonblocking(true).expect("nonblocking");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}");
        let tracker_hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));

        let hits_clone = tracker_hits.clone();
        let stop_clone = stop.clone();
        let handle = thread::spawn(move || {
            while !stop_clone.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                        let mut buf = [0u8; 4096];
                        let n = stream.read(&mut buf).unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..n]);
                        let first_line = req.lines().next().unwrap_or("");
                        let path = first_line.split_whitespace().nth(1).unwrap_or("/");
                        let cookie_hdr = req
                            .lines()
                            .find(|l| l.to_ascii_lowercase().starts_with("cookie:"))
                            .map(|l| l.split_once(':').map_or("", |(_, v)| v.trim()))
                            .unwrap_or("")
                            .to_owned();

                        let resp = match path {
                            "/one" => http_html(
                                "200 OK",
                                "<!doctype html><html><head><title>Page One</title></head>\
                                 <body style='background:#224466;color:#fff'>\
                                 <h1>Alpha alpha ALPHA omega</h1>\
                                 <p>searchable example phrase</p></body></html>",
                                None,
                            ),
                            "/two" => http_html(
                                "200 OK",
                                "<!doctype html><html><head><title>Page Two</title></head>\
                                 <body style='background:#336644;color:#fff'>\
                                 <h1>Second Page</h1></body></html>",
                                None,
                            ),
                            "/set-cookie" => http_html(
                                "200 OK",
                                "<!doctype html><html><head><title>Cookie Set</title></head>\
                                 <body>ok</body></html>",
                                Some("Set-Cookie: searlx_test=cookie123; Path=/\r\n"),
                            ),
                            "/check-cookie" => {
                                let body = format!(
                                    "<!doctype html><html><head><title>cookie={cookie_hdr}</title></head>\
                                     <body><pre id='c'>{cookie_hdr}</pre></body></html>"
                                );
                                http_html("200 OK", &body, None)
                            }
                            "/tracker-host" => http_html(
                                "200 OK",
                                "<!doctype html><html><head><title>Tracker Host</title>\
                                 <script>window.__trackerLoaded = false;</script>\
                                 <script src='/blocked-tracker.js'></script></head>\
                                 <body>host</body></html>",
                                None,
                            ),
                            "/blocked-tracker.js" => {
                                hits_clone.fetch_add(1, Ordering::SeqCst);
                                http_raw(
                                    "200 OK",
                                    "application/javascript; charset=utf-8",
                                    b"window.__trackerLoaded = true;",
                                    None,
                                )
                            }
                            "/download-me" => http_raw(
                                "200 OK",
                                "application/octet-stream",
                                b"hello-searlx-download-payload",
                                Some(
                                    "Content-Disposition: attachment; filename=\"report.txt\"\r\n",
                                ),
                            ),
                            _ => http_html(
                                "404 Not Found",
                                "<!doctype html><title>404</title><h1>Not found</h1>",
                                None,
                            ),
                        };
                        let _ = stream.write_all(resp.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            base_url,
            tracker_hits,
            stop,
            handle: Some(handle),
        }
    }

    fn url(&self, path: &str) -> PageUrl {
        PageUrl::parse(&format!("{}{path}", self.base_url)).expect("valid local url")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn http_html(status: &str, body: &str, extra_headers: Option<&str>) -> String {
    http_raw(
        status,
        "text/html; charset=utf-8",
        body.as_bytes(),
        extra_headers,
    )
}

fn http_raw(status: &str, content_type: &str, body: &[u8], extra_headers: Option<&str>) -> String {
    let extra = extra_headers.unwrap_or("");
    let body_str = String::from_utf8_lossy(body);
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body_str}",
        body.len()
    )
}

#[test]
fn webkitgtk_lifecycle_navigation_history_visibility_bounds_focus_zoom_mute_snapshot() {
    let temp = TestDir::new();
    let Some(mut engine) = try_engine_for_contract_test(temp.path()) else {
        return;
    };
    let server = TestServer::start();

    let space = SpaceId::new();
    let profile = ProfileId::new();
    let first = engine
        .create_tab(TabOptions {
            space,
            profile,
            ..TabOptions::default()
        })
        .expect("create first");
    let second = engine
        .create_tab(TabOptions {
            space,
            profile,
            background: true,
            ..TabOptions::default()
        })
        .expect("create second");
    assert_ne!(first, second);

    let one = server.url("/one");
    let two = server.url("/two");

    engine.navigate(first, one.clone()).expect("nav one");
    let ev1 = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab, url }
                    if *tab == first && url.identity() == one.identity()
            )
        })
    });
    assert!(ev1.iter().any(|e| matches!(
        e,
        EngineEvent::NavigationCommitted { tab, url }
            if *tab == first && url.identity() == one.identity()
    )));
    assert!(ev1.iter().any(|e| matches!(
        e,
        EngineEvent::TitleChanged { tab, title } if *tab == first && title == "Page One"
    )));

    engine.navigate(first, two.clone()).expect("nav two");
    let ev2 = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab, url }
                    if *tab == first && url.identity() == two.identity()
            )
        })
    });
    assert!(ev2.iter().any(|e| matches!(
        e,
        EngineEvent::HistoryChanged { tab, can_go_back: true, .. } if *tab == first
    )));

    engine.back(first).expect("back");
    let ev_back = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab, url }
                    if *tab == first && url.identity() == one.identity()
            )
        })
    });
    assert!(ev_back.iter().any(|e| matches!(
        e,
        EngineEvent::NavigationCommitted { tab, url }
            if *tab == first && url.identity() == one.identity()
    )));

    engine.forward(first).expect("forward");
    let ev_fwd = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab, url }
                    if *tab == first && url.identity() == two.identity()
            )
        })
    });
    assert!(ev_fwd.iter().any(|e| matches!(
        e,
        EngineEvent::NavigationCommitted { tab, url }
            if *tab == first && url.identity() == two.identity()
    )));

    engine.reload(first, true).expect("reload bypass cache");
    let ev_reload = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab, url }
                    if *tab == first && url.identity() == two.identity()
            )
        })
    });
    assert!(!ev_reload.is_empty());

    // Visibility switch does not reload!
    engine.set_visible(first, false).expect("hide first");
    engine.set_visible(first, true).expect("show first");
    let switch_events = engine.pump().expect("pump");
    assert!(
        !switch_events
            .iter()
            .any(|e| matches!(e, EngineEvent::NavigationStarted { .. })),
        "visibility toggle must not reload tab"
    );

    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };
    engine.set_bounds(first, rect).expect("set bounds");
    engine.focus(second).expect("focus");
    engine.set_zoom(second, 1.5).expect("zoom");
    assert!(engine.set_zoom(second, 0.05).is_err());
    assert!(engine.set_zoom(second, f64::NAN).is_err());

    engine.set_muted(second, true).expect("mute");
    let mute_events = pump_until(&mut engine, Duration::from_secs(2), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::MediaStateChanged { tab, muted: true, .. } if *tab == second
            )
        })
    });
    assert!(mute_events.iter().any(|e| matches!(
        e,
        EngineEvent::MediaStateChanged { tab, muted: true, .. } if *tab == second
    )));

    let diag = engine
        .tab_diagnostics(first)
        .expect("diag")
        .expect("tab present");
    assert_eq!(diag.bounds, rect);
    assert!(diag.visible);
    assert!(!diag.ephemeral);

    let diag2 = engine
        .tab_diagnostics(second)
        .expect("diag2")
        .expect("tab2 present");
    assert!((diag2.zoom - 1.5).abs() < 0.01);
    assert!(diag2.muted);

    let snap = engine.snapshot(first).expect("snapshot");
    assert!(snap.png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(snap.width > 0 && snap.height > 0);

    engine.close_tab(first).expect("close first");
    engine.close_tab(second).expect("close second");
    assert!(matches!(
        engine.close_tab(first),
        Err(EngineError::NoSuchTab(id)) if id == first
    ));
}

#[test]
fn webkitgtk_data_store_isolation_private_ephemeral_and_clear_data() {
    let temp = TestDir::new();
    let Some(mut engine) = try_engine_for_contract_test(temp.path()) else {
        return;
    };
    let server = TestServer::start();

    let profile = ProfileId::new();
    let space_a = SpaceId::new();
    let space_b = SpaceId::new();

    let store_a = engine.data_store(space_a, profile, false).expect("store a");
    let store_b = engine.data_store(space_b, profile, false).expect("store b");
    let store_priv = engine
        .data_store(space_a, profile, true)
        .expect("store priv");
    assert!(!store_a.ephemeral);
    assert!(!store_b.ephemeral);
    assert!(store_priv.ephemeral);
    assert_ne!(store_a, store_b);
    assert_ne!(store_a, store_priv);

    let tab_a = engine
        .create_tab(TabOptions {
            space: space_a,
            profile,
            private: false,
            ..TabOptions::default()
        })
        .expect("tab a");
    let tab_b = engine
        .create_tab(TabOptions {
            space: space_b,
            profile,
            private: false,
            ..TabOptions::default()
        })
        .expect("tab b");
    let tab_priv = engine
        .create_tab(TabOptions {
            space: space_a,
            profile,
            private: true,
            ..TabOptions::default()
        })
        .expect("tab priv");

    assert!(
        engine
            .tab_diagnostics(tab_priv)
            .expect("diag")
            .expect("present")
            .ephemeral
    );

    // Set cookie in space_a
    let set_cookie = server.url("/set-cookie");
    engine
        .navigate(tab_a, set_cookie.clone())
        .expect("set cookie");
    let _ = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab, url }
                    if *tab == tab_a && url.identity() == set_cookie.identity()
            )
        })
    });

    // Check cookie in space_a, space_b, and private tab
    let check_cookie = server.url("/check-cookie");
    for (tab, expect_cookie) in [(tab_a, true), (tab_b, false), (tab_priv, false)] {
        engine.navigate(tab, check_cookie.clone()).expect("check");
        let events = pump_until(&mut engine, Duration::from_secs(5), |events| {
            events.iter().any(|e| {
                matches!(
                    e,
                    EngineEvent::NavigationFinished { tab: t, url }
                        if *t == tab && url.identity() == check_cookie.identity()
                )
            })
        });
        let title = events
            .iter()
            .rev()
            .find_map(|e| match e {
                EngineEvent::TitleChanged { tab: t, title } if *t == tab => Some(title.clone()),
                _ => None,
            })
            .unwrap_or_default();
        assert_eq!(
            title.contains("searlx_test=cookie123"),
            expect_cookie,
            "unexpected cookie visibility in tab {tab:?}: title={title:?}"
        );
    }

    // Clear cookies from store_a and re-verify
    engine
        .clear_data(
            store_a,
            DataKinds {
                cookies: true,
                ..DataKinds::none()
            },
        )
        .expect("clear cookies");

    engine
        .navigate(tab_a, check_cookie.clone())
        .expect("recheck");
    let after_clear = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab, url }
                    if *tab == tab_a && url.identity() == check_cookie.identity()
            )
        })
    });
    let cleared_title = after_clear
        .iter()
        .rev()
        .find_map(|e| match e {
            EngineEvent::TitleChanged { tab, title } if *tab == tab_a => Some(title.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(
        !cleared_title.contains("searlx_test=cookie123"),
        "cookie persisted after clear_data: {cleared_title:?}"
    );

    engine.close_tab(tab_priv).expect("close private");
    assert!(engine.tab_diagnostics(tab_priv).expect("diag").is_none());
}

#[test]
fn webkitgtk_script_worlds_inject_eval_and_user_stylesheet() {
    let temp = TestDir::new();
    let Some(mut engine) = try_engine_for_contract_test(temp.path()) else {
        return;
    };
    let server = TestServer::start();
    let tab = engine.create_tab(TabOptions::default()).expect("create");

    // Inject isolated script at DocumentStart before navigating
    engine
        .inject(
            tab,
            ScriptWorld::Isolated,
            InjectAt::DocumentStart,
            "globalThis.__searlxIsoSecret = 'isolated-42'; \
             window.webkit.messageHandlers.searlx.postMessage('boot-isolated');",
        )
        .expect("inject isolated");

    let one = server.url("/one");
    engine.navigate(tab, one.clone()).expect("navigate");
    let nav_events = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::ScriptMessage { tab: t, world: ScriptWorld::Isolated, body }
                    if *t == tab && body == "boot-isolated"
            )
        }) && events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab: t, url }
                    if *t == tab && url.identity() == one.identity()
            )
        })
    });
    assert!(nav_events.iter().any(|e| matches!(
        e,
        EngineEvent::ScriptMessage { tab: t, world: ScriptWorld::Isolated, body }
            if *t == tab && body == "boot-isolated"
    )));

    // Verify Page world cannot see Search's isolated variable or isolated handler
    engine
        .eval(
            tab,
            ScriptWorld::Page,
            "const hidden = (typeof globalThis.__searlxIsoSecret === 'undefined') \
             && (typeof window.webkit?.messageHandlers?.searlx === 'undefined'); \
             window.webkit.messageHandlers.searlxPage.postMessage(hidden ? 'isolated-hidden' : 'leaked');",
        )
        .expect("eval page");
    let page_events = pump_until(&mut engine, Duration::from_secs(3), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::ScriptMessage { tab: t, world: ScriptWorld::Page, .. } if *t == tab
            )
        })
    });
    assert!(page_events.iter().any(|e| matches!(
        e,
        EngineEvent::ScriptMessage { tab: t, world: ScriptWorld::Page, body }
            if *t == tab && body == "isolated-hidden"
    )));

    // Apply user stylesheet and confirm computed style via isolated eval
    engine
        .apply_user_stylesheet(
            tab,
            "body { background-color: rgb(12, 34, 56) !important; }",
            true,
            true,
        )
        .expect("apply stylesheet");
    engine
        .eval(
            tab,
            ScriptWorld::Isolated,
            "window.webkit.messageHandlers.searlx.postMessage(getComputedStyle(document.body).backgroundColor);",
        )
        .expect("eval style check");
    let style_events = pump_until(&mut engine, Duration::from_secs(3), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::ScriptMessage { tab: t, world: ScriptWorld::Isolated, body }
                    if *t == tab && body == "rgb(12, 34, 56)"
            )
        })
    });
    assert!(style_events.iter().any(|e| matches!(
        e,
        EngineEvent::ScriptMessage { tab: t, world: ScriptWorld::Isolated, body }
            if *t == tab && body == "rgb(12, 34, 56)"
    )));

    engine
        .clear_user_stylesheets(tab)
        .expect("clear stylesheets");
}

#[test]
fn webkitgtk_content_filter_store_blocks_trackers() {
    let temp = TestDir::new();
    let Some(mut engine) = try_engine_for_contract_test(temp.path()) else {
        return;
    };
    let server = TestServer::start();

    // Verify core's WebKit JSON rules compile cleanly in WebKitGTK
    let core_rules = compile_webkit_content_rules(&[]);
    engine
        .apply_content_rules("searlx-shield", &core_rules)
        .expect("compile core content rules");

    // Apply a deterministic loopback tracker rule
    let test_rules = r#"[{"trigger":{"url-filter":"^https?://127\\.0\\.0\\.1:[0-9]+/blocked-tracker\\.js$"},"action":{"type":"block"}}]"#;
    engine
        .apply_content_rules("test-tracker-block", test_rules)
        .expect("compile test blocker");

    let tab = engine.create_tab(TabOptions::default()).expect("create");
    let host_url = server.url("/tracker-host");
    engine.navigate(tab, host_url.clone()).expect("navigate");
    let _ = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab: t, url }
                    if *t == tab && url.identity() == host_url.identity()
            )
        })
    });

    engine
        .eval(
            tab,
            ScriptWorld::Isolated,
            "window.webkit.messageHandlers.searlx.postMessage(String(Boolean(window.__trackerLoaded)));",
        )
        .expect("check blocked");
    let blocked_events = pump_until(&mut engine, Duration::from_secs(3), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::ScriptMessage { tab: t, world: ScriptWorld::Isolated, .. } if *t == tab
            )
        })
    });
    assert!(blocked_events.iter().any(|e| matches!(
        e,
        EngineEvent::ScriptMessage { tab: t, world: ScriptWorld::Isolated, body }
            if *t == tab && body == "false"
    )));
    assert_eq!(server.tracker_hits.load(Ordering::SeqCst), 0);

    // Clear the test rule, reload, and verify the script now loads.
    engine
        .apply_content_rules("test-tracker-block", "[]")
        .expect("remove test blocker");
    engine.reload(tab, true).expect("reload");
    let _ = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab: t, url }
                    if *t == tab && url.identity() == host_url.identity()
            )
        })
    });
    assert!(server.tracker_hits.load(Ordering::SeqCst) >= 1);
}

#[test]
fn webkitgtk_find_controller_matches_case_and_step() {
    let temp = TestDir::new();
    let Some(mut engine) = try_engine_for_contract_test(temp.path()) else {
        return;
    };
    let server = TestServer::start();
    let tab = engine.create_tab(TabOptions::default()).expect("create");
    let one = server.url("/one");
    engine.navigate(tab, one.clone()).expect("nav");
    let _ = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab: t, url }
                    if *t == tab && url.identity() == one.identity()
            )
        })
    });

    let insensitive = engine
        .find(
            tab,
            "alpha",
            FindOptions {
                case_sensitive: false,
                wrap: true,
                ..FindOptions::default()
            },
        )
        .expect("find insensitive");
    assert_eq!(insensitive.matches, 3);
    assert_eq!(insensitive.active, 1);
    assert!(insensitive.finished);

    let step2 = engine
        .find(
            tab,
            "alpha",
            FindOptions {
                case_sensitive: false,
                wrap: true,
                ..FindOptions::default()
            },
        )
        .expect("find next");
    assert_eq!(step2.matches, 3);
    assert_eq!(step2.active, 2);

    let sensitive = engine
        .find(
            tab,
            "ALPHA",
            FindOptions {
                case_sensitive: true,
                wrap: true,
                ..FindOptions::default()
            },
        )
        .expect("find sensitive");
    assert_eq!(sensitive.matches, 1);
    assert_eq!(sensitive.active, 1);

    let missing = engine
        .find(tab, "definitely-not-present-xyz", FindOptions::default())
        .expect("find missing");
    assert_eq!(missing.matches, 0);
    assert_eq!(missing.active, 0);

    let cleared = engine
        .find(tab, "", FindOptions::default())
        .expect("clear find");
    assert_eq!(cleared.matches, 0);
}

#[test]
fn webkitgtk_search_uri_scheme_and_custom_handler() {
    let temp = TestDir::new();
    let Some(mut engine) = try_engine_for_contract_test(temp.path()) else {
        return;
    };
    let tab = engine.create_tab(TabOptions::default()).expect("create");

    let settings_url = PageUrl::parse("search://settings").expect("settings url");
    engine
        .navigate(tab, settings_url.clone())
        .expect("nav settings");
    let default_events = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab: t, url }
                    if *t == tab && url.identity() == settings_url.identity()
            )
        })
    });
    assert!(default_events.iter().any(|e| matches!(
        e,
        EngineEvent::NavigationCommitted { tab: t, url }
            if *t == tab && url.identity() == settings_url.identity()
    )));

    engine
        .register_internal_handler(|uri| {
            InternalPageResponse::html(format!(
                "<!doctype html><html><head><title>Custom {uri}</title></head>\
                 <body><h1>{uri}</h1></body></html>"
            ))
        })
        .expect("register handler");

    let history_url = PageUrl::parse("search://history").expect("history url");
    engine
        .navigate(tab, history_url.clone())
        .expect("nav history");
    let custom_events = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::TitleChanged { tab: t, title }
                    if *t == tab && title == "Custom search://history"
            )
        })
    });
    assert!(custom_events.iter().any(|e| matches!(
        e,
        EngineEvent::TitleChanged { tab: t, title }
            if *t == tab && title == "Custom search://history"
    )));
}

#[test]
fn webkitgtk_downloads_permissions_dialogs_and_process_crash() {
    let temp = TestDir::new();
    let downloads_dir = temp.path().join("dl");
    let Some(mut engine) = try_engine_for_contract_test(temp.path()) else {
        return;
    };
    engine
        .set_downloads_dir(downloads_dir.clone())
        .expect("set downloads dir");
    let server = TestServer::start();

    let tab = engine.create_tab(TabOptions::default()).expect("create");
    let one = server.url("/one");
    engine.navigate(tab, one.clone()).expect("nav");
    let _ = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::NavigationFinished { tab: t, url }
                    if *t == tab && url.identity() == one.identity()
            )
        })
    });

    // ScriptDialog
    engine
        .eval(tab, ScriptWorld::Page, "alert('hi-searlx-alert');")
        .expect("eval alert");
    let dlg_events = pump_until(&mut engine, Duration::from_secs(3), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::ScriptDialog { tab: t, kind: DialogKind::Alert, message }
                    if *t == tab && message == "hi-searlx-alert"
            )
        })
    });
    assert!(dlg_events.iter().any(|e| matches!(
        e,
        EngineEvent::ScriptDialog { tab: t, kind: DialogKind::Alert, message }
            if *t == tab && message == "hi-searlx-alert"
    )));

    // PermissionRequest (Geolocation)
    engine
        .eval(
            tab,
            ScriptWorld::Page,
            "if (navigator.geolocation) { navigator.geolocation.getCurrentPosition(() => {}, () => {}); }",
        )
        .expect("eval geo");
    let perm_events = pump_until(&mut engine, Duration::from_secs(3), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::PermissionRequested { tab: t, kind: PermissionKind::Geolocation, .. }
                    if *t == tab
            )
        })
    });
    assert!(perm_events.iter().any(|e| matches!(
        e,
        EngineEvent::PermissionRequested { tab: t, kind: PermissionKind::Geolocation, .. }
            if *t == tab
    )));

    // Download signal + file destination
    let dl_url = server.url("/download-me");
    engine.navigate(tab, dl_url.clone()).expect("nav download");
    let dl_events = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::DownloadRequested { tab: t, suggested_name: Some(name), .. }
                    if *t == tab && name == "report.txt"
            )
        })
    });
    assert!(dl_events.iter().any(|e| matches!(
        e,
        EngineEvent::DownloadRequested { tab: t, suggested_name: Some(name), .. }
            if *t == tab && name == "report.txt"
    )));

    // Wait briefly for the download stream to write to disk
    let expected_file = downloads_dir.join("report.txt");
    let dl_wait = Instant::now();
    while dl_wait.elapsed() < Duration::from_secs(2) && !expected_file.exists() {
        let _ = engine.pump();
        thread::sleep(Duration::from_millis(15));
    }
    assert!(expected_file.exists(), "download file was not written");

    // WebProcessTerminated -> ProcessCrashed
    engine
        .terminate_tab_process(tab)
        .expect("terminate web process");
    let crash_events = pump_until(&mut engine, Duration::from_secs(5), |events| {
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::ProcessCrashed { tab: t, .. } if *t == tab))
    });
    assert!(crash_events.iter().any(|e| matches!(
        e,
        EngineEvent::ProcessCrashed { tab: t, reason }
            if *t == tab && !reason.is_empty()
    )));
}

#[test]
fn webkitgtk_engine_moves_across_threads_as_send_static_trait_object() {
    let temp = TestDir::new();
    let Some(engine) = try_engine_for_contract_test(temp.path()) else {
        return;
    };
    let mut boxed: Box<dyn Engine> = Box::new(engine);
    let handle = thread::spawn(move || {
        let url = PageUrl::parse("search://newtab").expect("url");
        let tab = boxed
            .create_tab(TabOptions {
                url: Some(url.clone()),
                ..TabOptions::default()
            })
            .expect("create on worker thread");
        let start = Instant::now();
        let mut committed = false;
        while start.elapsed() < Duration::from_secs(5) {
            let events = boxed.pump().expect("pump");
            if events.iter().any(|e| {
                matches!(
                    e,
                    EngineEvent::NavigationCommitted { tab: t, url: u }
                        if *t == tab && u.identity() == url.identity()
                )
            }) {
                committed = true;
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
        assert!(committed, "search://newtab never committed on moved engine");
        boxed.close_tab(tab).expect("close");
    });
    handle.join().expect("thread join");
}
