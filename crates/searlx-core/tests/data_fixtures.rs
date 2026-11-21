//! The P0-11 corpus: every fixture parses, unknown fields survive, and a
//! corrupt file is set aside rather than overwritten.

use std::fs;
use std::path::{Path, PathBuf};

use searlx_core::data::{
    BookmarksFile, DataError, HiddenFile, HistoryFile, PinsFile, SessionFile, SpacesFile,
    StylesFile, WindowsFile, read_json,
};
use serde_json::Value;

fn legacy(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/legacy")
        .join(name)
}

fn read_value(path: &Path) -> Value {
    let text = fs::read_to_string(path).expect("read fixture");
    serde_json::from_str(&text).expect("fixture must be JSON")
}

#[test]
fn every_fixture_parses() {
    let session: SessionFile = read_json(&legacy("session.json")).expect("session.json");
    assert_eq!(session.tabs.len(), 2);
    assert_eq!(session.groups.len(), 1);
    assert_eq!(session.splits.len(), 1);
    assert_eq!(
        session.tabs[1].scroll.as_ref().map(|scroll| scroll.y),
        Some(420.0)
    );

    let work: SessionFile = read_json(&legacy("session-7f3c2a10-3b1d-4f61-9a5e-1d2c3b4a5f60.json"))
        .expect("space session");
    assert_eq!(work.tabs.len(), 1);

    let windows: WindowsFile = read_json(&legacy("windows.json")).expect("windows.json");
    assert_eq!(windows.windows.len(), 1);
    assert_eq!(windows.windows[0].rows.len(), 2);

    let pins: PinsFile = read_json(&legacy("pins.json")).expect("pins.json");
    assert_eq!(pins.spaces.len(), 2);
    assert_eq!(pins.shared.len(), 1);
    assert_eq!(pins.folders[0].feeds[0].posts.len(), 1);

    let spaces: SpacesFile = read_json(&legacy("spaces.json")).expect("spaces.json");
    assert_eq!(spaces.spaces.len(), 2);

    let bookmarks: BookmarksFile = read_json(&legacy("bookmarks.json")).expect("bookmarks.json");
    assert_eq!(bookmarks.roots[0].children.len(), 2);

    let history: HistoryFile = read_json(&legacy("history.json")).expect("history.json");
    assert_eq!(history.visits.len(), 2);

    let hidden: HiddenFile = read_json(&legacy("hidden.json")).expect("hidden.json");
    assert_eq!(hidden.sites.len(), 2);

    let styles: StylesFile = read_json(&legacy("styles.json")).expect("styles.json");
    assert_eq!(styles.sites.len(), 2);
}

#[test]
fn unknown_fields_survive_a_round_trip() {
    let path = legacy("session.json");
    let original = read_value(&path);
    let parsed: SessionFile = read_json(&path).expect("session");
    let round = serde_json::to_value(&parsed).expect("serialise session");

    assert_eq!(round["x-unknown-top"], original["x-unknown-top"]);
    assert_eq!(
        round["tabs"][0]["x-unknown-tab"],
        original["tabs"][0]["x-unknown-tab"]
    );
    assert_eq!(
        round["tabs"][1]["scroll"]["x-unknown-scroll"],
        original["tabs"][1]["scroll"]["x-unknown-scroll"]
    );
    assert_eq!(
        round["groups"][0]["x-unknown-group"],
        original["groups"][0]["x-unknown-group"]
    );

    let path = legacy("pins.json");
    let original = read_value(&path);
    let parsed: PinsFile = read_json(&path).expect("pins");
    let round = serde_json::to_value(&parsed).expect("serialise pins");
    assert_eq!(
        round["spaces"]["space-personal"][0]["x-unknown-pin"],
        original["spaces"]["space-personal"][0]["x-unknown-pin"]
    );
    assert_eq!(
        round["folders"][0]["feeds"][0]["posts"][0]["x-unknown-post"],
        original["folders"][0]["feeds"][0]["posts"][0]["x-unknown-post"]
    );

    let path = legacy("bookmarks.json");
    let original = read_value(&path);
    let parsed: BookmarksFile = read_json(&path).expect("bookmarks");
    let round = serde_json::to_value(&parsed).expect("serialise bookmarks");
    assert_eq!(
        round["roots"][0]["children"][1]["children"][0]["x-unknown-nested"],
        original["roots"][0]["children"][1]["children"][0]["x-unknown-nested"]
    );
}

#[test]
fn corrupt_file_is_quarantined_intact() {
    let directory = tempfile::tempdir().expect("temp dir");
    let path = directory.path().join("bookmarks.json");
    let corrupt = b"{ \"roots\": [";
    fs::write(&path, corrupt).expect("write corrupt file");

    let error = read_json::<BookmarksFile>(&path).expect_err("corrupt file must fail");
    let DataError::Decode { quarantined_to, .. } = error else {
        panic!("expected a decode error, got {error:?}");
    };
    let quarantined_to = quarantined_to.expect("file must be quarantined");
    assert!(!path.exists());
    assert!(quarantined_to.exists());
    assert!(
        quarantined_to
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".unreadable-")),
        "quarantine name was {:?}",
        quarantined_to.file_name()
    );
    assert_eq!(fs::read(&quarantined_to).expect("read quarantine"), corrupt);
}

#[test]
fn missing_file_is_not_quarantined() {
    let directory = tempfile::tempdir().expect("temp dir");
    let path = directory.path().join("session.json");

    let error = read_json::<SessionFile>(&path).expect_err("missing file must fail");
    assert!(matches!(error, DataError::Read { .. }));
    assert!(!path.exists());
    let entries = fs::read_dir(directory.path())
        .expect("list directory")
        .count();
    assert_eq!(entries, 0);
}
