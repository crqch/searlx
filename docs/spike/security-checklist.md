# Phase 0 security checklist

**Date:** 2026-11-21
**Scope:** the shell being built now, and the boundaries it must not cross in
Phase 1/2.
**Rules:** [AGENTS.md](../../AGENTS.md) §2, §9, §12.

This is a threat model with tests, not a wish list. Every row names a required
behaviour, how to verify it, and the task that owns it. A row is not done until
its test exists and passes on the named platform.

## 1. Private tabs and non-persistent stores

| # | Risk | Required behaviour | Verification | Owner |
|---|---|---|---|---|
| S-01 | A private tab writes history, session, favicon, or password state. | Private tabs use an ephemeral engine store and are omitted from every durable write at the store compositor. | Open a private tab, visit a unique URL, close it, restart; grep the data directory and session files for the URL. No match. | P2-14 |
| S-02 | A private tab shares cookies or cache with a normal tab. | One ephemeral data store per private session; no sharing with normal spaces. | Sign in in a normal tab; open the same site private; it must be signed out. | P2-14 |
| S-03 | A feature writes private state because it does not know the tab is private. | The tab carries a privacy flag at creation; store writes take a `Privacy` argument and reject private writes by type, not by convention. | Unit test every store writer with `Privacy::Private`; compile-time or runtime rejection. | P1-02/P2-14 |
| S-04 | A crash leaves private data in a temp file. | Ephemeral store directories are removed on close; crash recovery does not restore private tabs. | Kill the process with a private tab open; inspect temp and data dirs. | P2-19 |
| S-05 | Private downloads or favicons appear in the normal UI. | Private downloads are unlisted; favicons are memory-only. | Download in private; open the downloads panel; no entry. | P2-05/P2-14 |

## 2. Password fill and origin checks

| # | Risk | Required behaviour | Verification | Owner |
|---|---|---|---|---|
| S-06 | A password fills on a page that is not the committed origin. | Fill only when the top-level committed origin equals the saved origin; re-check immediately before filling, not when the list was offered. | Navigate during the prompt to a different origin; fill must refuse. | P2-15 |
| S-07 | An https credential fills on http. | Never fill a credential saved from https into http, and never fill after a downgrade. | Save on https, open http, assert no offer. | P2-15 |
| S-08 | A look-alike host receives a credential. | Compare canonical origins (scheme, host, port) from `PageUrl`, not display strings; punycode handling is explicit. | Unit tests for IDN, trailing dot, userinfo, default port, mixed case. | P1-01/P2-15 |
| S-09 | A password or OTP is logged, shown in a crash, or copied to the clipboard unasked. | Never log field values; redact URL query/fragment; copy only on an explicit action; clear clipboard on a timer where the platform allows. | Log capture test; grep test; manual copy check. | P2-15 |
| S-10 | A password is saved without a successful sign-in. | Offer save only after a successful navigation/credential submission signal; never save on a failed login. | Test with a failing and a succeeding form. | P2-15 |
| S-11 | The vault is readable by an extension, agent, or another profile. | Credential-store trait is app-only; no engine/extension/agent API exposes it; per-profile scope is explicit. | API review; extension contract test; agent read test. | P2-15/P4-03 |

## 3. Agent tabs

| # | Risk | Required behaviour | Verification | Owner |
|---|---|---|---|---|
| S-12 | An agent navigates to a non-web scheme or a local file. | Agent tabs allow `http`/`https` only; `file:`, `search:`, `data:`, `javascript:`, and custom schemes are rejected. | Unit test the agent navigation guard with every scheme. | P4-08 |
| S-13 | An agent downloads, opens a file picker, or grants camera/mic/location/notifications. | Agent tabs deny downloads, file pickers, media capture, geolocation, notifications, and password save. | Permission matrix test per engine. | P4-08 |
| S-14 | An agent reads a password, card, or OTP field. | Reads and screenshots redact password/card/OTP fields at the extraction boundary, before the agent sees them. | Inject a form with each field type; assert redaction. | P4-08 |
| S-15 | An agent escapes its own tabs. | Agent commands are scoped to agent-created tabs; no access to normal tabs, settings, or another agent's tabs. | Negative test with a normal tab id. | P4-08 |
| S-16 | Stop does not stop. | Stop cancels the current action and queued actions immediately; the UI shows the stopped state. | Trigger a long action, press Stop, assert no further events. | P4-08 |

## 4. Extension boundaries (macOS WKWebExtension, gated)

| # | Risk | Required behaviour | Verification | Owner |
|---|---|---|---|---|
| S-17 | An extension reads the keychain, Apple Passwords, passkeys, settings, or agent secrets. | No shim exposes those APIs. A missing API returns an error, not an empty success. | Extension contract test enumerating the API surface. | P4-03 |
| S-18 | An extension reads private tabs without permission. | Private tabs are hidden from extension tab queries unless the user grants it per extension; the default is deny. | Install a tab-query extension; open a private tab; assert it is absent. | P4-03 |
| S-19 | An extension reads another extension's pages. | Extension origins are isolated; cross-extension page access is denied. | Two extensions, one probes the other. | P4-03 |
| S-20 | A tampered CRX or unpacked path escapes the install root. | Verify CRX3 signature; canonicalize and confine paths; reject symlinks and `..`. | Fixture corpus of tampered CRX and path traversal. | P4-03 |
| S-21 | An update asks for more permissions silently. | Diff permissions on update; prompt before granting; disable the extension until answered. | Install v1, update to v2 with a new host permission, assert prompt. | P4-03 |
| S-22 | A WebView2 extension is treated as store-supported. | Label BYO/unpacked only; no store installs; capability flag present only when a real extension passes. | Manual Windows check; capability test. | P4-12 |

## 5. Update signing

| # | Risk | Required behaviour | Verification | Owner |
|---|---|---|---|---|
| S-23 | An unsigned or tampered update installs. | Verify code signature and SHA-256 before replacing the bundle; refuse on mismatch. | Feed a tampered package; assert refusal and the old version still starts. | P2-18 |
| S-24 | A downgrade or replay installs an old vulnerable build. | Monotonic version check; refuse downgrade unless the user explicitly reinstalls. | Feed an older signed package. | P2-18 |
| S-25 | A failed update bricks the install. | Stage next to the current bundle; install on next launch; keep the previous version until the new one starts. | Simulate a failed start; roll back. | P2-18 |
| S-26 | The update check identifies the user. | No account id, machine id, or browsing data in the request; user can disable it; offline launch is quiet. | Inspect the request; run offline. | P2-18 |

## 6. Imported files

| # | Risk | Required behaviour | Verification | Owner |
|---|---|---|---|---|
| S-27 | Import follows a symlink outside the chosen folder. | `O_NOFOLLOW`, regular-file check, canonical path confinement, size cap. | Fixture with symlink and oversized file. | P1-12 |
| S-28 | A live SQLite database is read while the old app writes it. | Copy before opening; never open the live DB read-write; treat the copy as hostile. | Import from a locked/copy-only fixture. | P1-12 |
| S-29 | A malformed file overwrites good data or crashes the app. | Decode into new types; on failure quarantine the source and keep the current data; never overwrite on decode failure. | Corrupt every fixture; assert quarantine and unchanged current data. | P1-12/P1-02 |
| S-30 | Import writes into the old app's folder. | Read-only source; write only into the new data directory. | Filesystem permission test. | P1-12 |

## 7. URL redaction and display

| # | Risk | Required behaviour | Verification | Owner |
|---|---|---|---|---|
| S-31 | Userinfo (`user:pass@host`) is displayed, copied, stored, or logged. | `PageUrl` strips userinfo at construction; display/copy/log all use the stripped form. | Unit tests for `https://user:pass@host/`, percent-encoded userinfo, and `@` in path. | P1-01 |
| S-32 | Query/fragment credentials leak into logs. | `redact_url()` removes values of keys that look like tokens/passwords/OTP; log only the redacted form. | Golden tests for common key names and opaque tokens. | P1-01 |
| S-33 | A look-alike host is shown as the real one. | Display uses the canonical host; punycode/look-alike warning is a UI concern; the engine does not normalize it away. | Manual matrix from FEATURES.md §4.2. | P1-07 |
| S-34 | An internal `search://` route is reachable from a page. | Internal routes are allowlisted and app-only; a page navigation to `search://` is rejected. | Unit test the route guard. | P1-01 |

## 8. Data-file quarantine

| # | Risk | Required behaviour | Verification | Owner |
|---|---|---|---|---|
| S-35 | A corrupt file is overwritten and the user loses data. | On decode failure, rename to `*.unreadable-<UTC>.json` without overwriting an earlier quarantine; keep bytes intact. | Test twice with the same stamp; assert `-1` suffix and both files exist. | P1-02 |
| S-36 | A failed write leaves a half-written file. | Write a temp sibling, then rename; remove the temp on failure. | Crash/failure injection test. | P1-02 |
| S-37 | A quarantine name escapes the data directory. | Generate the name from the file stem and a fixed stamp; reject path separators. | Unit test odd file names. | P1-02 |
| S-38 | An unreadable file blocks startup. | Return defaults for that file, surface a non-blocking notice, and continue. | Start with every file corrupt. | P1-08/P2-19 |

## 9. Logging

| # | Risk | Required behaviour | Verification | Owner |
|---|---|---|---|---|
| S-39 | Page text, form values, tokens, or full URLs with query strings reach a log. | `tracing` target `search`; log redacted URLs and counts, never content or secrets. | Log capture in tests; review every `tracing::` call. | all |
| S-40 | A crash report includes secrets. | No crash upload; local crash notes redact the same fields as logs; user sees the path before sharing. | Manual crash with a form value. | P2-19 |
| S-41 | Debug builds log more than release. | The redaction layer is unconditional; debug does not bypass it. | Compare debug/release log output for the same action. | all |
| S-42 | An error message includes a file's contents. | Errors name the path and the parse location, never the payload. | Decode every corrupt fixture and inspect the error string. | P1-02 |

## 10. Open questions

1. Which engine callback is the authoritative "credential submitted" signal on
   each backend, and can a page forge it? P2-15 must answer before saving.
2. Does WebView2 expose a reliable top-level committed origin for the fill
   check, or must the app inject a script and trust the page? P2-15.
3. Can a WKWebExtension observe private tabs through any API that the app does
   not mediate? P4-03.
4. How does the clipboard-clear timer behave on Wayland without focus? P2-15.
5. Where is the quarantine notice shown if the settings page itself is
   unreadable? P2-19.
6. Does the update feed need a per-channel key, or one key with a channel
   claim? P2-18/ADR-0008.
