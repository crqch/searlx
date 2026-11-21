//! Contract checks for the fake engine, plus the shared harness used by every
//! backend's own tests.

use searlx_engine::FakeEngine;

#[test]
fn fake_engine_passes_the_lifecycle_contract() {
    let mut engine = FakeEngine::default();
    searlx_testkit::lifecycle(&mut engine).expect("fake engine must satisfy the contract");
}

#[test]
fn contract_reports_a_missing_commit() {
    let events = Vec::new();
    let url = searlx_core::PageUrl::parse("https://example.com/").expect("url");
    assert!(searlx_testkit::expect_committed(&events, searlx_core::TabId::new(1), &url).is_err());
}
