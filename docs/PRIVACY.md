# Privacy Policy — searlx

`searlx` is built around a strict non-negotiable boundary ([AGENTS.md §2](AGENTS.md)):

1. **No telemetry, analytics, crash upload, or user account.**
   - `searlx` never phones home with usage metrics, crash reports, hardware identifiers, or browsing history.
   - Local process crashes are recorded in a single-line redacted local `crash.log` inside the user's own profile folder with query strings and fragments stripped via `PageUrl::redacted()`.
2. **Only user-initiated network traffic.**
   - The only network requests `searlx` makes are:
     - Web pages and live RSS/Atom pin-folder feeds the user opens,
     - Favicon fetches for visited non-private tabs,
     - Optional signed update-feed checks (`HTTPS` only, signature + SHA-256 verified),
     - Optional AI requests when the user explicitly enables a provider and asks a question (never cloud providers in private tabs).
3. **Private tabs leave zero trace.**
   - Enforced centrally by `searlx_core::privacy::StoreCompositor`: private tabs run in an ephemeral in-memory data store and write zero history visits, zero session records, zero favicons, zero password save offers, zero persistent site permissions, zero Handoff activities, and zero iCloud/Folder sync records.
4. **No secrets in logs, errors, or disk files.**
   - Passwords and AI API keys live exclusively in the OS credential store (macOS Keychain, Windows Credential Manager, Linux Secret Service) and are wrapped in redacted types (`VaultSecret`, `AiApiKey`) whose `Debug` and `Display` implementations never emit the secret.
   - Passwords saved from an `https://` origin are never offered or filled on a plain `http://` page.
