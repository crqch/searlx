# Legacy fixtures

These files are **inferred**, not captured from the original app. The Swift
source and a real `~/Library/Application Support/Search` folder are not in this
checkout, so the shapes come from `FEATURES.md` §25 and the feature sections it
references.

Each fixture carries at least one `x-unknown-*` key at the top level and inside
a nested object. The tests assert those keys survive a decode/encode round trip.
When real old files are available, replace or add to these fixtures and tighten
the models; do not treat the current field names as verified.

Files:

| File | Model |
|---|---|
| `session.json` | `SessionFile` |
| `session-<uuid>.json` | `SessionFile` |
| `windows.json` | `WindowsFile` |
| `pins.json` | `PinsFile` |
| `spaces.json` | `SpacesFile` |
| `bookmarks.json` | `BookmarksFile` |
| `history.json` | `HistoryFile` |
| `hidden.json` | `HiddenFile` |
| `styles.json` | `StylesFile` |
