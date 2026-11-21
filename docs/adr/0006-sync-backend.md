# ADR-0006 — Sync backend

- **Status:** proposed
- **Date:** 2026-11-21
- **Related:** P3-14, P4-10

## Context

The original app syncs by writing JSON files to a user-chosen folder (for
example a cloud-drive folder) and by optional iCloud on macOS. A hosted sync
service would require an account, a server, and a privacy story the product does
not want.

## Decision

- **Folder sync is the portable baseline.** `Search Sync/<scope>/<device>.json`
  with merge rules, tombstones, and 365-day deletion as specified in
  FEATURES.md §20.
- **iCloud is an optional macOS layer** over the same model. It is gated, can be
  turned off, and never syncs private tabs, shared pins, password data, or
  history that the user excluded.
- **No account and no hosted server** in the default product. A future hosted
  service needs a new ADR and an explicit opt-in.
- Sync conflicts merge; a delete is a tombstone, not the absence of a record.
- Pin folders and live feeds stay local unless a later ADR says otherwise.

## Consequences

- The user supplies the transport (Dropbox, Syncthing, a network folder).
- Two devices can converge without the project operating infrastructure.
- iCloud and folder sync are mutually exclusive for the same scope.
- Sync files are hostile input: size caps, schema validation, quarantine, and
  no path traversal.

## Verification

- Two machines with the same folder converge (P3-14).
- A deleted item does not resurrect after a merge.
- A corrupt sync file is quarantined and does not erase local data.
