//! Contract checks for the fake engine, plus the shared harness used by every
//! backend's own tests.

use searlx_core::PageUrl;
use searlx_engine::{Engine, FakeEngine, ScriptWorld, TabOptions};

#[test]
fn fake_engine_passes_the_lifecycle_contract() {
    let mut engine = FakeEngine::default();
    searlx_testkit::lifecycle(&mut engine).expect("fake engine must satisfy the contract");
}

#[test]
fn contract_reports_a_missing_commit() {
    let events = Vec::new();
    let url = PageUrl::parse("https://example.com/").expect("url");
    assert!(searlx_testkit::expect_committed(&events, searlx_core::TabId::new(1), &url).is_err());
}

#[test]
fn fake_engine_matches_golden_event_transcript() {
    let mut engine = FakeEngine::default();
    let tab = engine
        .create_tab(TabOptions::default())
        .expect("create tab");
    let first = PageUrl::parse("https://example.com/one?secret=hunter2").expect("url 1");
    let second = PageUrl::parse("https://example.com/two#private-anchor").expect("url 2");

    engine.navigate(tab, first).expect("navigate 1");
    engine.navigate(tab, second).expect("navigate 2");
    engine.back(tab).expect("back");
    engine.forward(tab).expect("forward");
    engine.set_muted(tab, true).expect("mute");
    engine.post_script_message(tab, ScriptWorld::Isolated, "{\"scrollY\":240}");

    let events = engine.pump().expect("pump");
    let transcript = searlx_testkit::format_event_transcript(&events);
    let expected = include_str!("fixtures/lifecycle.transcript");
    assert_eq!(transcript, expected);
    assert!(!transcript.contains("hunter2"));
    assert!(!transcript.contains("private-anchor"));
}
