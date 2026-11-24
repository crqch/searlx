use searlx_core::address::{ParseOptions, SearchEngine, SubmitDisposition, SuggestionRow};
use searlx_core::controller::EngineEvent as CtrlEvent;
use searlx_core::data::{
    ArchivedTab, DataStore, SearchSitesFile, SessionFile, SessionTab, SiteSearchRecord, Split,
};
use searlx_core::domain::{CURRENT_SCHEMA_VERSION, PermissionDecision, PermissionKind, SpaceColor};
use searlx_core::history::ClearHistoryScope;
use searlx_core::menus_and_pages::{ConnectionState, InternalPageKind};
use searlx_core::peek_small_float::FloatTrigger;
use searlx_core::reader::ReaderTheme;
use searlx_core::session::RestoreMode;
use searlx_core::spaces_profiles::SpaceStorePolicy;
use searlx_core::{
    AppCommand, DragPayload, DropTarget, EngineDirective, EnginePermissionKind, HttpMethod,
    InternalPageData, PageUrl, ScriptWorld, Settings, SettingsPage, SplitDropSide,
};
use searlx_engine::fake::FakeCall;
use searlx_engine::{Capabilities, FakeEngine, ScriptWorld as RawScriptWorld};
use searlx_testkit::ControllerDriver;

fn full_capabilities() -> Capabilities {
    Capabilities {
        engine: "FakeWebKit".into(),
        extensions: true,
        passkeys: true,
        content_blockers: true,
        native_pip: true,
        site_isolation: true,
        custom_user_agent: true,
        print: true,
        pdf: true,
        find_while_pdf: true,
        downloads_pause: true,
        cookie_import: true,
        wayland_overlay: false,
        floating_video: true,
    }
}

#[test]
fn asleep_session_restore_materializes_only_active_tab_and_split_panes() {
    // 1. Plain 10-tab session -> only 1 engine view created on restore.
    let tmp1 = tempfile::tempdir().expect("temp1");
    let store1 = DataStore::new(tmp1.path());
    let tabs1: Vec<SessionTab> = (0..10)
        .map(|i| SessionTab {
            id: format!("tab-{i}"),
            url: format!("https://example.com/page-{i}"),
            title: format!("Page {i}"),
            ..SessionTab::default()
        })
        .collect();
    store1
        .save_session(
            None,
            &SessionFile {
                version: CURRENT_SCHEMA_VERSION,
                active: 4,
                tabs: tabs1,
                ..SessionFile::default()
            },
        )
        .expect("save session 1");

    let driver1 = ControllerDriver::open(
        tmp1.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open driver 1");

    let create_count1 = driver1
        .engine
        .calls()
        .iter()
        .filter(|c| matches!(c, FakeCall::CreateTab))
        .count();
    assert_eq!(create_count1, 1, "only active tab must materialize");
    let snap1 = driver1.snapshot();
    assert_eq!(snap1.tabs.len(), 10);
    assert!(snap1.tabs[4].engine_tab_id.is_some());
    assert!(!snap1.tabs[4].sleeping);
    for (idx, t) in snap1.tabs.iter().enumerate() {
        if idx != 4 {
            assert!(t.sleeping, "inactive tab {idx} must be asleep");
            assert_eq!(t.engine_tab_id, None, "sleeping tab has no engine TabId");
        }
    }

    // 2. 12-tab session where active tab (2) is split with tab-5 -> 2 engine tabs materialize.
    let tmp2 = tempfile::tempdir().expect("temp2");
    let store2 = DataStore::new(tmp2.path());
    let tabs2: Vec<SessionTab> = (0..12)
        .map(|i| SessionTab {
            id: format!("t-{i}"),
            url: format!("https://split.example/{i}"),
            title: format!("Split {i}"),
            ..SessionTab::default()
        })
        .collect();
    store2
        .save_session(
            None,
            &SessionFile {
                version: CURRENT_SCHEMA_VERSION,
                active: 2,
                tabs: tabs2,
                splits: vec![Split {
                    id: "sp-1".into(),
                    axis: "horizontal".into(),
                    ratio: 0.5,
                    first: "t-2".into(),
                    second: "t-5".into(),
                    ..Split::default()
                }],
                ..SessionFile::default()
            },
        )
        .expect("save session 2");

    let driver2 = ControllerDriver::open(
        tmp2.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open driver 2");
    let create_count2 = driver2
        .engine
        .calls()
        .iter()
        .filter(|c| matches!(c, FakeCall::CreateTab))
        .count();
    assert_eq!(create_count2, 2, "active split panes materialize together");
    let snap2 = driver2.snapshot();
    assert_eq!(snap2.split_panes.len(), 2);
    assert!(snap2.tabs[2].engine_tab_id.is_some());
    assert!(snap2.tabs[5].engine_tab_id.is_some());
    assert!(snap2.tabs[0].sleeping);
    assert_eq!(snap2.tabs[0].engine_tab_id, None);
}

#[test]
fn lazy_wake_sleep_put_down_pin_and_put_away_group_preserve_history() {
    let tmp = tempfile::tempdir().expect("temp");
    let store = DataStore::new(tmp.path());
    store
        .save_session(
            None,
            &SessionFile {
                version: CURRENT_SCHEMA_VERSION,
                active: 0,
                tabs: vec![
                    SessionTab {
                        id: "t-0".into(),
                        url: "https://one.example/start".into(),
                        title: "One Start".into(),
                        ..SessionTab::default()
                    },
                    SessionTab {
                        id: "t-1".into(),
                        url: "https://two.example/".into(),
                        title: "Two".into(),
                        ..SessionTab::default()
                    },
                ],
                ..SessionFile::default()
            },
        )
        .expect("save session");

    let mut driver = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open");

    let t0 = driver.snapshot().tabs[0].id;
    let t1 = driver.snapshot().tabs[1].id;
    assert!(driver.snapshot().tabs[1].sleeping);
    assert_eq!(driver.engine.tabs().len(), 1);

    // Navigate t0 through step 2 so Back/Forward stack builds.
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: t0,
            url: PageUrl::parse("https://one.example/step2").expect("u2"),
        })
        .expect("nav step2");
    assert_eq!(
        driver.snapshot().tabs[0].url.as_str(),
        "https://one.example/step2"
    );

    // Wake sleeping background tab t1 -> t1 materializes, t0 hides.
    driver
        .dispatch(AppCommand::ActivateTab { tab: t1 })
        .expect("activate t1");
    assert_eq!(driver.engine.tabs().len(), 2);
    assert!(!driver.snapshot().tabs[1].sleeping);
    assert!(driver.snapshot().tabs[1].engine_tab_id.is_some());
    assert!(
        !driver
            .engine
            .tab(driver.engine_tab_for(t0).expect("eid0"))
            .expect("ft0")
            .visible
    );

    // Put background t0 to sleep -> closes engine tab, preserves URL + history stack.
    driver
        .dispatch(AppCommand::SleepTab { tab: t0 })
        .expect("sleep t0");
    assert_eq!(driver.engine.tabs().len(), 1);
    assert!(driver.snapshot().tabs[0].sleeping);
    assert_eq!(driver.snapshot().tabs[0].engine_tab_id, None);
    assert_eq!(
        driver.snapshot().tabs[0].url.as_str(),
        "https://one.example/step2"
    );

    // Reactivate t0 and GoBack across sleep!
    driver
        .dispatch(AppCommand::ActivateTab { tab: t0 })
        .expect("wake t0");
    assert!(!driver.snapshot().tabs[0].sleeping);
    assert!(driver.snapshot().tabs[0].engine_tab_id.is_some());
    assert!(driver.snapshot().tabs[0].can_go_back);
    driver
        .dispatch(AppCommand::GoBack { tab: Some(t0) })
        .expect("go back across sleep");
    assert_eq!(
        driver.snapshot().tabs[0].url.as_str(),
        "https://one.example/start"
    );
    assert!(driver.snapshot().tabs[0].can_go_forward);

    // Pin t0 at home URL, wander away, CloseTab (⌘W) -> puts down pin (returns home, sleeps, closes webview, focuses t1).
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: t0,
            url: PageUrl::parse("https://pin.example/home").expect("pin home"),
        })
        .expect("nav pin home");
    driver
        .dispatch(AppCommand::PinTab { tab: t0 })
        .expect("pin t0");
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: t0,
            url: PageUrl::parse("https://pin.example/wandered").expect("wander"),
        })
        .expect("wander");
    driver
        .dispatch(AppCommand::CloseTab { tab: Some(t0) })
        .expect("put down pin");
    let pin_view = driver
        .snapshot()
        .tabs
        .into_iter()
        .find(|t| t.id == t0)
        .expect("pin remains");
    assert!(pin_view.sleeping);
    assert_eq!(pin_view.engine_tab_id, None);
    assert_eq!(pin_view.url.as_str(), "https://pin.example/home");
    assert_eq!(driver.snapshot().active_tab, Some(t1));

    // Group tab put-away & second close delete.
    driver
        .dispatch(AppCommand::NewTab {
            url: Some(PageUrl::parse("https://group.example/1").expect("g1")),
            activate: true,
        })
        .expect("new g1");
    let g_tab = driver.snapshot().active_tab.expect("g_tab");
    driver
        .dispatch(AppCommand::CreateTabGroup {
            name: "Work".into(),
            color: SpaceColor::Blue,
            tabs: vec![g_tab],
        })
        .expect("create group");
    driver
        .dispatch(AppCommand::CloseTab { tab: Some(g_tab) })
        .expect("put away group tab");
    let put_away = driver
        .snapshot()
        .tabs
        .into_iter()
        .find(|t| t.id == g_tab)
        .expect("put away remains");
    assert!(put_away.put_away);
    assert!(put_away.sleeping);
    assert_eq!(put_away.engine_tab_id, None);
    assert_eq!(driver.snapshot().active_tab, Some(t1));

    // Closing a put-away grouped tab removes it completely.
    driver
        .dispatch(AppCommand::CloseTab { tab: Some(g_tab) })
        .expect("second close deletes");
    assert!(!driver.snapshot().tabs.iter().any(|t| t.id == g_tab));
}

#[test]
fn multi_tab_navigation_reopen_closed_and_internal_pages_respect_private_tabs() {
    let tmp = tempfile::tempdir().expect("temp");
    let mut driver = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open");

    // Initial blank tab (`search://`) has no engine webview.
    assert_eq!(driver.engine.tabs().len(), 0);
    let initial_id = driver.snapshot().active_tab.expect("initial");
    assert_eq!(driver.snapshot().tabs[0].engine_tab_id, None);

    // Navigate initial tab to https -> materializes 1 webview.
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: initial_id,
            url: PageUrl::parse("https://rust-lang.org/").expect("rust"),
        })
        .expect("nav rust");
    assert_eq!(driver.engine.tabs().len(), 1);

    // Open a second tab, navigate, close it, and reopen with ⇧⌘T.
    driver
        .dispatch(AppCommand::NewTab {
            url: Some(PageUrl::parse("https://docs.rs/").expect("docs")),
            activate: true,
        })
        .expect("new docs");
    let docs_id = driver.snapshot().active_tab.expect("docs id");
    driver
        .dispatch(AppCommand::CloseTab { tab: Some(docs_id) })
        .expect("close docs");
    assert_eq!(driver.snapshot().tabs.len(), 1);

    driver
        .dispatch(AppCommand::ReopenClosedTab)
        .expect("reopen docs");
    assert_eq!(driver.snapshot().tabs.len(), 2);
    let reopened = driver.snapshot().active_tab.expect("reopened");
    assert_eq!(
        driver
            .snapshot()
            .tabs
            .iter()
            .find(|t| t.id == reopened)
            .expect("tab")
            .url
            .as_str(),
        "https://docs.rs/"
    );

    // Open search://settings -> creates internal tab with zero engine webview (`engine_tab_id == None`).
    driver
        .dispatch(AppCommand::OpenInternalPage {
            kind: InternalPageKind::Settings,
            detail: None,
        })
        .expect("open settings");
    let settings_tab = driver.snapshot().active_tab.expect("settings tab");
    assert!(matches!(
        driver.snapshot().active_internal_page,
        Some(InternalPageData::Settings {
            section: SettingsPage::General,
            ..
        })
    ));
    assert_eq!(
        driver
            .snapshot()
            .tabs
            .iter()
            .find(|t| t.id == settings_tab)
            .expect("settings")
            .engine_tab_id,
        None
    );

    // Open a private tab in front, then request Settings > Privacy:
    // MUST focus existing settings tab cleanly and NEVER turn the private tab into Settings!
    driver
        .dispatch(AppCommand::NewPrivateTab {
            url: Some(PageUrl::parse("https://private.example/").expect("priv")),
            activate: true,
        })
        .expect("new priv");
    let priv_id = driver.snapshot().active_tab.expect("priv id");

    driver
        .dispatch(AppCommand::OpenInternalPage {
            kind: InternalPageKind::Settings,
            detail: Some("privacy".into()),
        })
        .expect("settings privacy");

    let snap = driver.snapshot();
    assert_eq!(snap.active_tab, Some(settings_tab));
    assert!(matches!(
        snap.active_internal_page,
        Some(InternalPageData::Settings {
            section: SettingsPage::Privacy,
            ..
        })
    ));
    assert!(
        snap.tabs
            .iter()
            .find(|t| t.id == priv_id)
            .expect("priv intact")
            .private,
        "private tab must remain untouched"
    );
}

#[test]
fn private_tab_store_isolation_leaves_zero_durable_trace_and_clears_ephemeral_store() {
    let tmp = tempfile::tempdir().expect("temp");
    let mut driver = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open");

    driver
        .dispatch(AppCommand::NewPrivateTab {
            url: Some(PageUrl::parse("https://secret.bank/login?token=xyz").expect("url")),
            activate: true,
        })
        .expect("open private");
    let priv_id = driver.snapshot().active_tab.expect("priv id");

    // 1. Favicon, download, permission, hide-element, dusk, site-css, password-submit in private tab.
    driver
        .push_core_event(CtrlEvent::FaviconChanged {
            tab: priv_id,
            url: Some(PageUrl::parse("https://secret.bank/favicon.ico").expect("fav")),
        })
        .expect("favicon");
    driver
        .push_core_event(CtrlEvent::DownloadRequested {
            tab: priv_id,
            url: PageUrl::parse("https://secret.bank/statement.pdf").expect("dl"),
            method: HttpMethod::Get,
            has_request_body: false,
            suggested_name: Some("statement.pdf".into()),
            total_bytes: Some(1024),
        })
        .expect("dl request");
    driver
        .push_core_event(CtrlEvent::PermissionRequested {
            tab: priv_id,
            origin: "https://secret.bank".into(),
            kind: EnginePermissionKind::Camera,
        })
        .expect("perm request");
    assert!(driver
        .executed_directives
        .iter()
        .any(|d| matches!(d, EngineDirective::RespondPermission { tab, allow: false, .. } if *tab == priv_id)));

    driver
        .dispatch(AppCommand::HideElement {
            selector: ".promo-banner".into(),
        })
        .expect("hide element");
    driver
        .dispatch(AppCommand::ToggleDuskForSite)
        .expect("dusk toggle");
    driver
        .push_core_event(CtrlEvent::ScriptMessage {
            tab: priv_id,
            world: ScriptWorld::Isolated,
            body: r#"{"kind":"password_submit","username":"alice","password":"pw"}"#.into(),
        })
        .expect("pw submit");
    assert!(driver.snapshot().pending_password_offer.is_none());

    // Site CSS is strictly read-only inside private tabs.
    assert!(
        driver
            .dispatch(AppCommand::UpdateSiteCss {
                site_host: Some("secret.bank".into()),
                css: "body { background: red; }".into(),
            })
            .is_err()
    );

    // Private downloads are unlisted in downloads UI yet counted in global active progress.
    let snap = driver.snapshot();
    assert!(snap.downloads.entries.is_empty());
    assert_eq!(snap.downloads.active_count, 1);
    assert_eq!(snap.downloads.total_bytes, 1024);

    // Close the last private tab -> clears ephemeral store + private hide-elements + private Dusk memory.
    driver
        .dispatch(AppCommand::CloseTab { tab: Some(priv_id) })
        .expect("close private");

    assert!(
        driver
            .executed_directives
            .iter()
            .any(|d| matches!(d, EngineDirective::ClearData { private: true, .. }))
    );
    assert!(
        driver
            .engine
            .calls()
            .iter()
            .any(|c| matches!(c, FakeCall::ClearData(handle, _) if handle.ephemeral))
    );

    // Verify ZERO durable traces anywhere.
    let comp = driver.controller.compositor();
    assert!(comp.history.visits().is_empty());
    assert!(comp.history.recently_closed().is_empty());
    assert!(comp.favicons.is_empty());
    assert!(comp.handoff_url.is_none());
    assert!(comp.icloud_exported_urls.is_empty());
    assert!(comp.permissions.is_empty());
    assert!(
        driver
            .controller
            .hide_elements()
            .before_first_paint_css("secret.bank", false)
            .is_none()
    );
    assert!(
        driver
            .controller
            .dusk()
            .durable_site_preferences()
            .is_empty()
    );
}

#[test]
fn address_suggestions_lookalike_and_site_card_flow() {
    let tmp = tempfile::tempdir().expect("temp");
    let mut driver = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open");

    // Enable commands + search_suggest.
    let settings = Settings {
        commandbar: true,
        search_suggest: true,
        pages_dusk: true,
        ..Settings::default()
    };
    driver
        .dispatch(AppCommand::UpdateSettings {
            settings: Box::new(settings),
        })
        .expect("update settings");

    // 1. Invalid inputs shake without navigating.
    driver
        .dispatch(AppCommand::SubmitAddress {
            input: "v1.2.3".into(),
            disposition: SubmitDisposition::CurrentTab,
            shift_pressed: false,
        })
        .expect("submit version");
    assert!(driver.snapshot().address.shake_reason.is_some());

    driver
        .dispatch(AppCommand::SubmitAddress {
            input: "/definitely-missing-local-file-xyz.txt".into(),
            disposition: SubmitDisposition::CurrentTab,
            shift_pressed: false,
        })
        .expect("submit missing path");
    assert!(driver.snapshot().address.shake_reason.is_some());

    // 2. Cyrillic IDN lookalike preserves punycode in tab, address bar, and site card header.
    driver
        .dispatch(AppCommand::SubmitAddress {
            input: "https://\u{0430}pple.com/store".into(),
            disposition: SubmitDisposition::CurrentTab,
            shift_pressed: false,
        })
        .expect("submit lookalike");
    let snap = driver.snapshot();
    assert!(snap.address.is_lookalike);
    assert_eq!(snap.address.safe_host, "xn--pple-43d.com");
    let card = snap.site_card.expect("site card present");
    assert!(card.is_lookalike);
    assert_eq!(card.header, "xn--pple-43d.com");

    // 3. Inline completion & live engine suggestions in normal tab vs disabled in private tab.
    driver.dispatch(AppCommand::FocusAddressBar).expect("focus");
    assert_eq!(driver.snapshot().address.text, "xn--pple-43d.com/store");
    driver
        .dispatch(AppCommand::SetAddressText {
            text: "rust".into(),
        })
        .expect("type rust");
    driver
        .dispatch(AppCommand::UpdateEngineSuggestions {
            query: "rust".into(),
            suggestions: vec!["rust lang".into(), "rust cargo".into()],
        })
        .expect("engine suggestions");
    let rows = driver.snapshot().address.suggestions;
    assert!(rows.iter().any(
        |r| matches!(r, SuggestionRow::EngineSuggestion { query, .. } if query == "rust lang")
    ));

    // Private tab hides engine suggestions!
    driver
        .dispatch(AppCommand::NewPrivateTab {
            url: None,
            activate: true,
        })
        .expect("new private");
    driver
        .dispatch(AppCommand::FocusAddressBar)
        .expect("focus priv");
    driver
        .dispatch(AppCommand::SetAddressText {
            text: "rust".into(),
        })
        .expect("type priv");
    let priv_rows = driver.snapshot().address.suggestions;
    assert!(
        !priv_rows
            .iter()
            .any(|r| matches!(r, SuggestionRow::EngineSuggestion { .. }))
    );

    // Resolve URL with ParseOptions sanity check.
    assert!(SearchEngine::Google.search_url("test").is_ok());
    let _ = ParseOptions::default();
}

#[test]
fn blocking_hide_elements_site_css_dusk_reader_and_permissions() {
    let tmp = tempfile::tempdir().expect("temp");
    let mut driver = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open");

    let settings = Settings {
        sites_styles: true,
        pages_dusk: true,
        reader_twins: true,
        ..Settings::default()
    };
    driver
        .dispatch(AppCommand::UpdateSettings {
            settings: Box::new(settings),
        })
        .expect("settings");

    // 1. Navigate to YouTube -> DocumentStart injects Page + Isolated YouTube blocker scripts.
    let yt_url = PageUrl::parse("https://www.youtube.com/watch?v=1").expect("yt");
    driver
        .dispatch(AppCommand::SubmitAddress {
            input: yt_url.as_str().into(),
            disposition: SubmitDisposition::CurrentTab,
            shift_pressed: false,
        })
        .expect("nav yt");
    let yt_tab = driver.snapshot().active_tab.expect("yt tab");
    let yt_eid = driver.engine_tab_for(yt_tab).expect("yt eid");
    let injected = &driver.engine.tab(yt_eid).expect("ft").injected;
    assert!(
        injected
            .iter()
            .any(|(w, _, js)| *w == RawScriptWorld::Page && js.contains("AD_KEYS"))
    );
    assert!(
        injected
            .iter()
            .any(|(w, _, js)| *w == RawScriptWorld::Isolated && js.contains("ad-showing"))
    );

    // Site card shows YouTube blocker + DontFloatVideos + KeepAwake + permissions.
    driver
        .push_core_event(CtrlEvent::MixedContentDetected { tab: yt_tab })
        .expect("mixed");
    let card = driver.snapshot().site_card.expect("card");
    assert_eq!(card.connection, ConnectionState::MixedContent);
    assert_eq!(card.block_ads_and_trackers, Some(true));
    assert_eq!(card.dont_float_videos_here, Some(false));

    // Toggle site blocker off for YouTube.
    driver
        .dispatch(AppCommand::ToggleSiteBlocker { site_host: None })
        .expect("pause blocker");
    assert_eq!(
        driver
            .snapshot()
            .site_card
            .expect("card")
            .block_ads_and_trackers,
        Some(false)
    );

    // 2. Hide Elements + ⌘Z undo + Site CSS.
    driver
        .dispatch(AppCommand::HideElement {
            selector: "#cookie-wall".into(),
        })
        .expect("hide");
    assert!(
        driver
            .controller
            .hide_elements()
            .before_first_paint_css("youtube.com", false)
            .is_some_and(|css| css.contains("#cookie-wall"))
    );
    driver
        .dispatch(AppCommand::UndoHideElement)
        .expect("undo hide");
    assert!(
        driver
            .controller
            .hide_elements()
            .before_first_paint_css("youtube.com", false)
            .is_none()
    );

    driver
        .dispatch(AppCommand::UpdateSiteCss {
            site_host: None,
            css: "body { font-family: serif; }".into(),
        })
        .expect("site css");
    assert!(
        driver
            .engine
            .tab(yt_eid)
            .expect("ft")
            .evaluated
            .iter()
            .any(|(_, js)| js.contains("office-style"))
    );

    // 3. Dusk measured RGB -> light page applies Dusk, pause everywhere disables it.
    driver
        .push_core_event(CtrlEvent::PageRenderedLuminance {
            tab: yt_tab,
            r: 250,
            g: 250,
            b: 250,
        })
        .expect("luminance");
    assert_eq!(
        driver.snapshot().site_card.expect("card").darken_this_site,
        Some(true)
    );
    driver
        .dispatch(AppCommand::ToggleDuskPausedEverywhere)
        .expect("pause dusk");
    assert_eq!(
        driver.snapshot().site_card.expect("card").darken_this_site,
        None
    );

    // 4. Reader Mode extracts article + detects twin + reload on exit.
    let html = "<html><head><title>Article</title><link rel='alternate' type='text/markdown' href='/watch.md'></head><body><article><h1>Hello Reader</h1><p onclick=\"bad()\">Long calm prose paragraph with plenty of readable content for testing.</p></article></body></html>";
    driver
        .dispatch(AppCommand::ToggleReaderMode {
            html: Some(html.into()),
        })
        .expect("enter reader");
    let article = driver
        .snapshot()
        .active_reader_article
        .expect("reader article");
    assert!(!article.content_html.contains("onclick"));
    assert_eq!(article.markdown_twin_url.as_deref(), Some("/watch.md"));
    driver
        .dispatch(AppCommand::SetReaderTheme {
            theme: ReaderTheme::Dark,
        })
        .expect("dark reader");
    driver
        .dispatch(AppCommand::ToggleReaderMode { html: None })
        .expect("exit reader");
    assert!(driver.snapshot().active_reader_article.is_none());

    // 5. Permission decision persisted & cleared on ForgetAllPermissions.
    driver
        .dispatch(AppCommand::DecidePermission {
            tab: yt_tab,
            origin: "https://www.youtube.com".into(),
            kind: EnginePermissionKind::Camera,
            decision: PermissionDecision::Allow,
        })
        .expect("decide perm");
    assert_eq!(
        driver.controller.permissions().decisions_for_url(&yt_url)[0],
        (PermissionKind::Camera, PermissionDecision::Allow)
    );
    driver
        .dispatch(AppCommand::ForgetAllPermissions)
        .expect("forget");
    assert!(driver.controller.permissions().is_empty());
}

#[test]
fn splits_spaces_bookmarks_downloads_find_and_crash_recovery() {
    let tmp = tempfile::tempdir().expect("temp");
    let mut driver = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open");

    // 1. Splits: 2 panes -> 3 panes -> snap resize -> narrow stage hides other panes -> widen restores.
    driver
        .dispatch(AppCommand::NewTab {
            url: Some(PageUrl::parse("https://pane1.example/alpha").expect("p1")),
            activate: true,
        })
        .expect("p1");
    let p1 = driver.snapshot().active_tab.expect("p1 id");
    driver
        .dispatch(AppCommand::NewTab {
            url: Some(PageUrl::parse("https://pane2.example/beta").expect("p2")),
            activate: true,
        })
        .expect("p2");
    let p2 = driver.snapshot().active_tab.expect("p2 id");
    driver
        .dispatch(AppCommand::ToggleSplitView {
            target_tab: Some(p1),
            partner_tab: Some(p2),
        })
        .expect("split 2");
    assert_eq!(driver.snapshot().split_panes.len(), 2);

    // Resize near center -> snaps even 0.5/0.5.
    driver
        .dispatch(AppCommand::ResizeSplit {
            stage_width_pt: 1007.0,
            pointer_x: 505.0,
        })
        .expect("snap");
    let panes = driver.snapshot().split_panes;
    assert!((panes[0].width - panes[1].width).abs() < 1.0);

    // Narrow viewport (<507pt) shows only focused pane; widen back shows both.
    driver
        .dispatch(AppCommand::SetViewportStageWidth { width_pt: 450.0 })
        .expect("narrow");
    assert_eq!(driver.snapshot().split_panes.len(), 1);
    driver
        .dispatch(AppCommand::SetViewportStageWidth { width_pt: 1200.0 })
        .expect("widen");
    assert_eq!(driver.snapshot().split_panes.len(), 2);

    // Swap panes & find in active pane.
    driver.dispatch(AppCommand::SwapSplitPanes).expect("swap");
    driver
        .dispatch(AppCommand::FindInPage {
            query: "example".into(),
            case_sensitive: false,
            whole_words: false,
            text_hint: None,
        })
        .expect("find");
    assert!(
        driver
            .snapshot()
            .find
            .is_some_and(|f| f.total_matches >= 1 && f.active_match == 1)
    );

    // 2. Bookmarks & downloads.
    driver
        .dispatch(AppCommand::ToggleBookmark {
            parent_folder_id: None,
            title: Some("Saved Pane".into()),
        })
        .expect("bookmark");
    assert!(driver.snapshot().is_active_page_bookmarked);

    driver
        .push_core_event(CtrlEvent::DownloadRequested {
            tab: p2,
            url: PageUrl::parse("https://pane2.example/file.zip").expect("zip"),
            method: HttpMethod::Get,
            has_request_body: false,
            suggested_name: Some("file.zip".into()),
            total_bytes: Some(2048),
        })
        .expect("dl start");
    let dl_id = driver.snapshot().downloads.entries[0].id.clone();
    driver
        .dispatch(AppCommand::PauseDownload { id: dl_id.clone() })
        .expect("pause");
    driver
        .dispatch(AppCommand::ResumeDownload { id: dl_id })
        .expect("resume");

    // 3. Isolated space vs switching back + delete OwnStore space clears engine store.
    driver
        .dispatch(AppCommand::CreateSpace {
            profile: None,
            name: "Isolated Work".into(),
            color: SpaceColor::Green,
            icon: "briefcase".into(),
            store_policy: SpaceStorePolicy::OwnStore,
        })
        .expect("create space");
    let iso_space = driver
        .snapshot()
        .spaces
        .into_iter()
        .find(|s| s.name == "Isolated Work")
        .expect("iso space")
        .id;
    driver
        .dispatch(AppCommand::SwitchSpace { space: iso_space })
        .expect("switch space");
    let iso_tab = driver.snapshot().active_tab.expect("iso tab");
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: iso_tab,
            url: PageUrl::parse("https://corp.internal/dashboard?secret=42").expect("corp"),
        })
        .expect("nav corp");
    assert!(
        driver.controller.compositor().handoff_url.is_none()
            || driver
                .controller
                .compositor()
                .handoff_url
                .as_ref()
                .is_none_or(|u| u.host() != Some("corp.internal")),
        "OwnStore space never advertises Handoff"
    );

    // 4. Crash recovery: crash #1 auto-reloads, crash #2 prompts manual retry, log redacts query values!
    driver
        .push_core_event(CtrlEvent::ProcessCrashed {
            tab: iso_tab,
            reason: "segfault".into(),
        })
        .expect("crash 1");
    assert!(driver.snapshot().tabs[0].crash_retry_message.is_none());
    driver
        .push_core_event(CtrlEvent::ProcessCrashed {
            tab: iso_tab,
            reason: "segfault".into(),
        })
        .expect("crash 2");
    assert!(driver.snapshot().tabs[0].crash_retry_message.is_some());

    let crash_log = std::fs::read_to_string(tmp.path().join("crash.log")).expect("crash log");
    assert!(
        !crash_log.contains("secret=42"),
        "crash log must redact queries"
    );

    driver
        .dispatch(AppCommand::DeleteSpace { space: iso_space })
        .expect("delete space");
    assert!(driver.executed_directives.iter().any(
        |d| matches!(d, EngineDirective::ClearData { space: Some(s), .. } if *s == iso_space)
    ));

    // 5. Clear History wipes visits cleanly.
    driver
        .dispatch(AppCommand::ClearHistory {
            scope: ClearHistoryScope::All,
        })
        .expect("clear history");
    assert!(driver.controller.compositor().history.visits().is_empty());
}

#[test]
fn password_vault_origin_check_save_session_roundtrip_and_lifecycle_sleep() {
    let tmp = tempfile::tempdir().expect("temp");
    let mut driver = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open");

    // 1. Save password after submit on https://app.example/login; refuse fill after origin change.
    let login_url = PageUrl::parse("https://app.example/login").expect("login");
    let t0 = driver.snapshot().active_tab.expect("t0");
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: t0,
            url: login_url,
        })
        .expect("nav login");
    driver
        .push_core_event(CtrlEvent::ScriptMessage {
            tab: t0,
            world: ScriptWorld::Isolated,
            body: r#"{"kind":"password_submit","username":"alice","password":"hunter2"}"#.into(),
        })
        .expect("pw submit");
    let offer = driver
        .snapshot()
        .pending_password_offer
        .expect("save offer");
    assert_eq!(offer.username, "alice");
    driver
        .dispatch(AppCommand::SavePasswordOffer {
            tab: t0,
            accept: true,
            never_for_host: false,
        })
        .expect("accept save");
    assert_eq!(driver.controller.password_vault().len(), 1);

    // Same origin fill succeeds and evaluates isolated fill script.
    driver
        .dispatch(AppCommand::FillCredential {
            tab: t0,
            offered_origin: "https://app.example".into(),
            username: "alice".into(),
            is_main_frame: true,
        })
        .expect("same origin fill");

    // Navigated to phishing host -> FillCredential fails loudly!
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: t0,
            url: PageUrl::parse("https://evil.example/login").expect("evil"),
        })
        .expect("nav evil");
    assert!(
        driver
            .dispatch(AppCommand::FillCredential {
                tab: t0,
                offered_origin: "https://app.example".into(),
                username: "alice".into(),
                is_main_frame: true,
            })
            .is_err()
    );

    // 2. Open a background tab, open a private tab, hide an element, SaveSession, and reopen!
    driver
        .dispatch(AppCommand::NewTab {
            url: Some(PageUrl::parse("https://idle.example/").expect("idle")),
            activate: false,
        })
        .expect("idle bg tab");
    let idle_id = driver.snapshot().tabs[1].id;
    driver
        .dispatch(AppCommand::NewPrivateTab {
            url: Some(PageUrl::parse("https://secret.example/").expect("priv")),
            activate: false,
        })
        .expect("priv bg");
    driver
        .dispatch(AppCommand::HideElement {
            selector: ".annoyance".into(),
        })
        .expect("hide");

    // Advance clock > 30 mins and TickLifecycle -> idle_id (unkept background) sleeps!
    driver
        .dispatch(AppCommand::TickLifecycle {
            now_secs: 1_700_000_000 + 2_000,
        })
        .expect("tick lifecycle");
    let after_tick = driver.snapshot();
    let idle_view = after_tick
        .tabs
        .iter()
        .find(|t| t.id == idle_id)
        .expect("idle remains");
    assert!(idle_view.sleeping);
    assert_eq!(idle_view.engine_tab_id, None);

    driver.dispatch(AppCommand::SaveSession).expect("save");
    drop(driver);

    // Reopen from same store -> 2 non-private tabs restored (t0 awake, idle_id asleep), private omitted, hidden rule survived.
    let driver2 = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("reopen");
    let snap2 = driver2.snapshot();
    assert_eq!(
        snap2.tabs.len(),
        2,
        "private tab must never survive restart"
    );
    assert!(!snap2.tabs[0].sleeping);
    assert!(snap2.tabs[1].sleeping);
    assert_eq!(snap2.tabs[1].engine_tab_id, None);
    assert!(
        driver2
            .controller
            .hide_elements()
            .before_first_paint_css("evil.example", false)
            .is_some_and(|c| c.contains(".annoyance"))
    );
}

#[test]
fn own_store_history_isolation_and_clear_history_scope_guarantees() {
    let tmp = tempfile::tempdir().expect("temp");
    let store = DataStore::new(tmp.path());
    store
        .save_search_sites(&SearchSitesFile {
            sites: vec![SiteSearchRecord {
                host: "crates.io".into(),
                name: "Crates".into(),
                keyword: Some("cr".into()),
                template: "https://crates.io/search?q=%s".into(),
                ..SiteSearchRecord::default()
            }],
            ..SearchSitesFile::default()
        })
        .expect("save search sites");

    store
        .save_archive(
            None,
            &searlx_core::data::ArchiveFile {
                tabs: vec![ArchivedTab {
                    id: "arch-1".into(),
                    url: "https://old.example/".into(),
                    title: "Old".into(),
                    closed_at: "2026-11-01T00:00:00Z".into(),
                    ..ArchivedTab::default()
                }],
                ..searlx_core::data::ArchiveFile::default()
            },
        )
        .expect("save archive");

    let mut driver = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open");

    // 1. Personal visit + completed download.
    let t0 = driver.snapshot().active_tab.expect("t0");
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: t0,
            url: PageUrl::parse("https://personal.example/news").expect("personal url"),
        })
        .expect("nav personal");
    driver
        .push_core_event(CtrlEvent::DownloadRequested {
            tab: t0,
            url: PageUrl::parse("https://cdn.example/installer.dmg").expect("dl url"),
            method: HttpMethod::Get,
            has_request_body: false,
            suggested_name: Some("installer.dmg".into()),
            total_bytes: Some(1024),
        })
        .expect("dl start");
    let dl_id = driver.snapshot().downloads.entries[0].id.clone();
    driver
        .push_core_event(CtrlEvent::DownloadProgress {
            download_id: dl_id,
            bytes_received: 1024,
        })
        .expect("dl finish");

    // 2. Create an OwnStore space: initially empty history (never falls back to Personal history!).
    driver
        .dispatch(AppCommand::CreateSpace {
            profile: None,
            name: "Signed Out Work".into(),
            color: SpaceColor::Blue,
            icon: "briefcase".into(),
            store_policy: SpaceStorePolicy::OwnStore,
        })
        .expect("create own store space");
    let iso_space = driver
        .snapshot()
        .spaces
        .into_iter()
        .find(|s| s.name == "Signed Out Work")
        .expect("iso space")
        .id;
    assert!(
        driver
            .controller
            .history_for_space(iso_space)
            .visits()
            .is_empty(),
        "empty OwnStore space must never expose Personal history"
    );

    driver
        .dispatch(AppCommand::SwitchSpace { space: iso_space })
        .expect("switch iso");
    let iso_tab = driver.snapshot().active_tab.expect("iso tab");
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: iso_tab,
            url: PageUrl::parse("https://isolated-work.example/portal").expect("iso url"),
        })
        .expect("nav iso");
    driver
        .push_core_event(CtrlEvent::TitleChanged {
            tab: iso_tab,
            title: "Work Portal Title".into(),
        })
        .expect("title changed");

    // OwnStore navigation and TitleChanged must NEVER leak into shared compositor history!
    assert_eq!(driver.controller.compositor().history.visits().len(), 1);
    assert_eq!(
        driver.controller.compositor().history.visits()[0]
            .url
            .as_str(),
        "https://personal.example/news"
    );
    let iso_visits = driver.controller.history_for_space(iso_space).visits();
    assert_eq!(iso_visits.len(), 1);
    assert_eq!(
        iso_visits[0].url.as_str(),
        "https://isolated-work.example/portal"
    );
    assert_eq!(iso_visits[0].title, "Work Portal Title");

    // SaveSession persists isolated history to history-space-<id>.json.
    driver
        .dispatch(AppCommand::SaveSession)
        .expect("save session");
    let space_suffix = format!("space-{iso_space}");
    let disk_iso_hist = store
        .load_history(Some(&space_suffix))
        .expect("load isolated space history");
    assert_eq!(disk_iso_hist.visits.len(), 1);
    assert_eq!(
        disk_iso_hist.visits[0].url,
        "https://isolated-work.example/portal"
    );

    // 3. ClearHistory(SignedOutSpace(iso_space)) wipes ONLY isolated space history & file,
    // leaving shared Personal history, downloads source_url, and search-sites.json intact!
    driver
        .dispatch(AppCommand::ClearHistory {
            scope: ClearHistoryScope::SignedOutSpace(iso_space.to_string()),
        })
        .expect("clear signed out space");
    assert!(
        driver
            .controller
            .history_for_space(iso_space)
            .visits()
            .is_empty()
    );
    assert!(
        store
            .load_history(Some(&space_suffix))
            .expect("reloaded isolated history")
            .visits
            .is_empty()
    );
    assert_eq!(driver.controller.compositor().history.visits().len(), 1);
    assert_eq!(
        driver.controller.downloads().listed_entries()[0]
            .source_url
            .as_ref()
            .map(|u| u.as_str()),
        Some("https://cdn.example/installer.dmg")
    );
    assert_eq!(
        store.load_downloads().expect("disk downloads").downloads[0]
            .source_url
            .as_deref(),
        Some("https://cdn.example/installer.dmg")
    );
    assert_eq!(store.load_search_sites().expect("sites").sites.len(), 1);
    assert_eq!(store.load_archive(None).expect("archive").tabs.len(), 1);

    // 4. ClearHistory(All) strips BOTH in-memory and disk download source_urls and shared artifacts,
    // and SaveSession keeps source_url stripped on disk.
    driver
        .dispatch(AppCommand::ClearHistory {
            scope: ClearHistoryScope::All,
        })
        .expect("clear all");
    assert!(driver.controller.compositor().history.visits().is_empty());
    assert_eq!(driver.controller.downloads().listed_entries().len(), 1);
    assert!(
        driver.controller.downloads().listed_entries()[0]
            .source_url
            .is_none(),
        "ClearHistory(All) must clear in-memory download source_url for search://downloads"
    );
    driver
        .dispatch(AppCommand::SaveSession)
        .expect("save after clear");
    let post_downloads = store.load_downloads().expect("downloads after clear+save");
    assert_eq!(post_downloads.downloads.len(), 1);
    assert!(post_downloads.downloads[0].source_url.is_none());
    assert!(
        store
            .load_search_sites()
            .expect("cleared sites")
            .sites
            .is_empty()
    );
}

#[test]
fn split_drag_order_reopen_closed_group_non_lazy_background_and_cross_space_leave_tab() {
    let tmp = tempfile::tempdir().expect("temp");
    let mut driver = ControllerDriver::open(
        tmp.path(),
        FakeEngine::new(full_capabilities()),
        RestoreMode::Normal,
    )
    .expect("open");

    // 1. Default Settings (tabs_lazy == false) materializes non-internal background tabs immediately.
    assert!(!driver.controller.settings().tabs_lazy);
    let active_t0 = driver.snapshot().active_tab.expect("t0");
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: active_t0,
            url: PageUrl::parse("https://center.example/").expect("center"),
        })
        .expect("nav t0");
    driver
        .dispatch(AppCommand::NewTab {
            url: Some(PageUrl::parse("https://left.example/").expect("left")),
            activate: false,
        })
        .expect("open bg left");
    let bg_left = driver.snapshot().tabs[1].id;
    let bg_left_snap = driver
        .snapshot()
        .tabs
        .into_iter()
        .find(|t| t.id == bg_left)
        .expect("bg_left snap");
    assert!(!bg_left_snap.sleeping);
    let bg_eid = bg_left_snap
        .engine_tab_id
        .expect("non-lazy background tab must materialize immediately");
    assert!(!driver.engine.tab(bg_eid).expect("fake tab").visible);
    assert_eq!(driver.snapshot().active_tab, Some(active_t0));

    // When tabs_lazy == true, background tab stays sleeping without engine view.
    let lazy_settings = Settings {
        tabs_lazy: true,
        float_leave: true,
        ..Settings::default()
    };
    driver
        .dispatch(AppCommand::UpdateSettings {
            settings: Box::new(lazy_settings),
        })
        .expect("enable lazy + float_leave");
    driver
        .dispatch(AppCommand::NewTab {
            url: Some(PageUrl::parse("https://right.example/").expect("right")),
            activate: false,
        })
        .expect("open lazy bg right");
    let bg_right_snap = driver
        .snapshot()
        .tabs
        .into_iter()
        .find(|t| t.url.as_str() == "https://right.example/")
        .expect("bg_right snap");
    let bg_right = bg_right_snap.id;
    assert!(bg_right_snap.sleeping);
    assert_eq!(bg_right_snap.engine_tab_id, None);

    // 2. Drag non-active bg_left onto active_t0's Left edge -> split panes [bg_left, active_t0]!
    driver
        .dispatch(AppCommand::BeginDrag {
            payload: DragPayload::Tab(bg_left),
            x: 10.0,
            y: 10.0,
        })
        .expect("begin drag left");
    driver
        .dispatch(AppCommand::UpdateDrag {
            x: 100.0,
            y: 100.0,
            target: Some(DropTarget::PageSplitEdge {
                target_tab: active_t0,
                side: SplitDropSide::Left,
            }),
        })
        .expect("update drag left");
    driver
        .dispatch(AppCommand::CommitDrag)
        .expect("commit drag left");
    let split_after_left: Vec<_> = driver
        .snapshot()
        .split_panes
        .iter()
        .map(|p| p.tab)
        .collect();
    assert_eq!(split_after_left, vec![bg_left, active_t0]);

    // Drag sleeping non-active bg_right onto active_t0's Right edge -> wakes bg_right into [bg_left, active_t0, bg_right]!
    driver
        .dispatch(AppCommand::BeginDrag {
            payload: DragPayload::Tab(bg_right),
            x: 10.0,
            y: 20.0,
        })
        .expect("begin drag right");
    driver
        .dispatch(AppCommand::UpdateDrag {
            x: 900.0,
            y: 100.0,
            target: Some(DropTarget::PageSplitEdge {
                target_tab: active_t0,
                side: SplitDropSide::Right,
            }),
        })
        .expect("update drag right");
    driver
        .dispatch(AppCommand::CommitDrag)
        .expect("commit drag right");
    let split_after_right: Vec<_> = driver
        .snapshot()
        .split_panes
        .iter()
        .map(|p| p.tab)
        .collect();
    assert_eq!(split_after_right, vec![bg_left, active_t0, bg_right]);
    assert!(
        driver
            .snapshot()
            .tabs
            .iter()
            .find(|t| t.id == bg_right)
            .expect("bg_right woke")
            .engine_tab_id
            .is_some()
    );

    // 3. Create group, navigate so history builds, CloseGroup, then ReopenClosedGroup restores real tabs + history.
    driver
        .dispatch(AppCommand::NewTab {
            url: Some(PageUrl::parse("https://grp-a.example/step1").expect("ga1")),
            activate: true,
        })
        .expect("new ga1");
    let ga = driver.snapshot().active_tab.expect("ga");
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: ga,
            url: PageUrl::parse("https://grp-a.example/step2").expect("ga2"),
        })
        .expect("nav ga2");
    driver
        .dispatch(AppCommand::NewTab {
            url: Some(PageUrl::parse("https://grp-b.example/").expect("gb")),
            activate: true,
        })
        .expect("new gb");
    let gb = driver.snapshot().active_tab.expect("gb");
    driver
        .dispatch(AppCommand::CreateTabGroup {
            name: "Research".into(),
            color: SpaceColor::Violet,
            tabs: vec![ga, gb],
        })
        .expect("create group");
    let gid = driver.snapshot().groups[0].id;
    let count_before_close = driver.snapshot().tabs.len();
    driver
        .dispatch(AppCommand::CloseGroup { group: gid })
        .expect("close group");
    assert!(driver.snapshot().groups.is_empty());
    assert_eq!(driver.snapshot().tabs.len(), count_before_close - 2);

    driver
        .dispatch(AppCommand::ReopenClosedGroup)
        .expect("reopen group");
    let after_reopen = driver.snapshot();
    assert_eq!(after_reopen.groups.len(), 1);
    let reopened_group = &after_reopen.groups[0];
    assert_eq!(reopened_group.name, "Research");
    assert_eq!(reopened_group.tabs.len(), 2);
    for &live_tid in &reopened_group.tabs {
        assert!(
            after_reopen
                .tabs
                .iter()
                .any(|t| t.id == live_tid && t.group == Some(reopened_group.id)),
            "group must reference recreated live TabId"
        );
    }
    let first_reopened = after_reopen
        .tabs
        .iter()
        .find(|t| t.id == reopened_group.tabs[0])
        .expect("first reopened");
    assert_eq!(first_reopened.url.as_str(), "https://grp-a.example/step2");
    assert!(first_reopened.can_go_back);

    // 4. Cross-space ActivateTab with playing video triggers LeaveTab floating video on outgoing space's tab.
    let space_a = after_reopen.active_space;
    driver
        .dispatch(AppCommand::CreateSpace {
            profile: None,
            name: "Space B".into(),
            color: SpaceColor::Green,
            icon: "sparkles".into(),
            store_policy: SpaceStorePolicy::SharedWithFirst,
        })
        .expect("create space b");
    let space_b = driver
        .snapshot()
        .spaces
        .into_iter()
        .find(|s| s.name == "Space B")
        .expect("space b")
        .id;
    driver
        .dispatch(AppCommand::SwitchSpace { space: space_b })
        .expect("switch space b");
    let tab_in_b = driver.snapshot().active_tab.expect("tab in b");
    driver
        .dispatch(AppCommand::NavigateTab {
            tab: tab_in_b,
            url: PageUrl::parse("https://notes.example/").expect("notes"),
        })
        .expect("nav notes");

    // Switch back to Space A, start media on its active tab, then ActivateTab into Space B!
    driver
        .dispatch(AppCommand::SwitchSpace { space: space_a })
        .expect("back to space a");
    let video_tab_a = driver.snapshot().active_tab.expect("video tab a");
    driver
        .push_core_event(CtrlEvent::MediaStateChanged {
            tab: video_tab_a,
            audible: true,
            muted: false,
        })
        .expect("media playing");
    driver
        .dispatch(AppCommand::ActivateTab { tab: tab_in_b })
        .expect("cross-space activate");
    let float = driver
        .snapshot()
        .floating_video
        .expect("cross-space leave must start floating video");
    assert_eq!(float.tab_id, video_tab_a);
    assert_eq!(float.space_id, space_a);
    assert_eq!(float.trigger, FloatTrigger::LeaveTab);
}
