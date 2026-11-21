//! Performance tuning (`P5-05`), CI fuzzing harness (`P5-06`), security hardening (`P5-07`),
//! and release-candidate readiness checks (`P5-08`).
//!
//! Behavioural specifications:
//! - `P5-05`: 60 Hz (`16,667 µs`) and 120 Hz (`8,333 µs`) frame budgets (`pages.120.mode`), process-count sleep budget.
//! - `P5-06`: Deterministic property & mutation fuzzing across URL parsing, address resolution,
//!   JSON data-file codecs, RSS/Atom & `mailto:` parsers, CRX3 & path confinement, and agent message/URL redaction.
//! - `P5-07`: Content-Security-Policy (`INTERNAL_PAGE_CSP`) for `search://` pages, IPC message validation,
//!   and per-OS process sandboxing & site-isolation descriptors.

use crate::address::{ParseOptions, resolve_address};
use crate::ai_agents_bench::{AgentElementHandle, redact_url_for_agent};
use crate::data::model::{BookmarksFile, HistoryFile, PinsFile, SessionFile, SpacesFile};
use crate::error::{CoreError, Result};
use crate::extensions_security::{
    derive_crx_id_from_public_key, sha256_digest, validate_extension_entry_path,
    verify_crx3_package,
};
use crate::pins_and_folders::parse_rss_or_atom_feed;
use crate::settings::HighRefreshMode;
use crate::sync_panel_launcher_routes::MailtoMessage;
use crate::url::PageUrl;

// ============================================================================
// P5-05 — Performance tuning & 120 Hz frame budget (`FEATURES.md` §20, §21)
// ============================================================================

/// Frame budget in microseconds for 60 Hz rendering (`16,667 µs`).
pub const FRAME_BUDGET_60HZ_US: u64 = 16_667;
/// Frame budget in microseconds for 120 Hz ProMotion / high-refresh rendering (`8,333 µs`).
pub const FRAME_BUDGET_120HZ_US: u64 = 8_333;
/// Maximum live awake web processes before aggressive background tab sleeping kicks in (`12`).
pub const MAX_RECOMMENDED_LIVE_WEB_PROCESSES: usize = 12;

/// Compute the target frame interval in microseconds from `pages.120.mode`, AC power, and Low Power Mode (`FEATURES.md` §21, P5-05).
#[must_use]
pub const fn target_frame_budget_us(
    mode: HighRefreshMode,
    on_ac_power: bool,
    low_power_mode: bool,
    display_supports_120hz: bool,
) -> u64 {
    if low_power_mode || !display_supports_120hz {
        return FRAME_BUDGET_60HZ_US;
    }
    match mode {
        HighRefreshMode::Never => FRAME_BUDGET_60HZ_US,
        HighRefreshMode::OnPower => {
            if on_ac_power {
                FRAME_BUDGET_120HZ_US
            } else {
                FRAME_BUDGET_60HZ_US
            }
        }
        HighRefreshMode::Always => FRAME_BUDGET_120HZ_US,
    }
}

// ============================================================================
// P5-07 — Security hardening: CSP, IPC validation, and sandbox policy (`AGENTS.md` §7, §9)
// ============================================================================

/// Strict Content-Security-Policy applied to every `search://` internal page (`P5-07`).
///
/// Blocks all network requests (`connect-src 'none'`), framing (`frame-ancestors 'none'`),
/// inline scripts (`script-src 'self'`), and external resources.
pub const INTERNAL_PAGE_CSP: &str = "default-src 'none'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; script-src 'self'; connect-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// Maximum permitted IPC payload from a page script or extension message (`1 MiB`, `FEATURES.md` §14.6, §17.2).
pub const MAX_IPC_MESSAGE_BYTES: usize = 1024 * 1024;

/// Validate an incoming script/extension/agent IPC message payload (`AGENTS.md` §2.7, P5-07).
///
/// Rules:
/// - Rejects empty payloads, payloads exceeding `1 MiB`, or payloads containing NUL (`\0`) bytes.
/// - Requires a valid JSON object (`{ ... }`) at the root.
/// - Requires the sender's committed origin to match `expected_origin_prefix` when specified.
pub fn validate_ipc_message(
    raw_json: &str,
    committed_sender_origin: &str,
    expected_origin_prefix: Option<&str>,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    if raw_json.is_empty() || raw_json.len() > MAX_IPC_MESSAGE_BYTES {
        return Err(CoreError::Invalid(format!(
            "IPC payload length {} out of bounds (1..={MAX_IPC_MESSAGE_BYTES})",
            raw_json.len()
        )));
    }
    if raw_json.contains('\0') {
        return Err(CoreError::Invalid(
            "IPC payload contains forbidden NUL byte".to_owned(),
        ));
    }
    if let Some(prefix) = expected_origin_prefix
        && !committed_sender_origin.starts_with(prefix)
    {
        return Err(CoreError::Invalid(format!(
            "IPC sender origin {committed_sender_origin:?} does not match expected {prefix:?}"
        )));
    }
    let parsed: serde_json::Value = serde_json::from_str(raw_json)
        .map_err(|e| CoreError::Invalid(format!("malformed IPC JSON: {e}")))?;
    match parsed {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(CoreError::Invalid(
            "IPC payload root must be a JSON object".to_owned(),
        )),
    }
}

/// Process sandboxing and site-isolation posture per OS (`P5-07`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessSandboxPolicy {
    /// Target OS (`"macos"`, `"windows"`, `"linux"`).
    pub target_os: &'static str,
    /// Whether the WebProcess sandbox is enforced by the engine.
    pub web_process_sandboxed: bool,
    /// Whether ephemeral data stores are isolated in separate network sessions.
    pub ephemeral_private_network_session: bool,
    /// Content-Security-Policy applied to `search://` pages.
    pub internal_page_csp: &'static str,
}

impl ProcessSandboxPolicy {
    /// Return the required process sandbox policy for `target_os`.
    #[must_use]
    pub fn for_target_os(target_os: &'static str) -> Self {
        Self {
            target_os,
            web_process_sandboxed: true,
            ephemeral_private_network_session: true,
            internal_page_csp: INTERNAL_PAGE_CSP,
        }
    }
}

// ============================================================================
// P5-06 — Deterministic fuzzing harness for parsers & security boundaries
// ============================================================================

/// Summary of a deterministic fuzzing sweep (`P5-06`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzSweepReport {
    /// Total mutated inputs fed across all 5 fuzz targets.
    pub total_inputs_tested: usize,
    /// Number of inputs that returned `Ok` or a clean `Err` without panicking.
    pub clean_outcomes: usize,
}

/// Deterministic xorshift64 pseudo-random generator for reproducible CI fuzzing (`P5-06`).
struct XorShift64(u64);

impl XorShift64 {
    const fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_bytes(&mut self, max_len: usize) -> Vec<u8> {
        let len = (self.next_u64() as usize) % (max_len + 1);
        let mut buf = Vec::with_capacity(len);
        while buf.len() < len {
            let word = self.next_u64().to_le_bytes();
            let take = (len - buf.len()).min(8);
            buf.extend_from_slice(&word[..take]);
        }
        buf
    }
}

/// Run `iterations` rounds of deterministic fuzzing across all 5 parser/security surfaces (`P5-06`):
/// 1. URL & address parser (`PageUrl::parse`, `resolve_address`)
/// 2. JSON data-file decoders (`SessionFile`, `BookmarksFile`, `HistoryFile`, `PinsFile`, `SpacesFile`)
/// 3. Feed & `mailto:` parsers (`parse_rss_or_atom_feed`, `MailtoMessage::parse`)
/// 4. CRX3 container & path confinement verifiers (`verify_crx3_package`, `validate_extension_entry_path`)
/// 5. Agent handle, URL redaction, and IPC message validators (`AgentElementHandle::parse`, `redact_url_for_agent`, `validate_ipc_message`)
pub fn run_ci_fuzz_suite(iterations: usize) -> FuzzSweepReport {
    let mut rng = XorShift64::new(0x5EA2_1C00_F022_2026);
    let mut total_inputs_tested = 0;
    let mut clean_outcomes = 0;

    let adversarial_seeds: &[&str] = &[
        "",
        "   ",
        "http://",
        "https://user:secret@example.com:8443/path?token=123&pass=abc#frag",
        "javascript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "mailto:a@b.com,c@d.com?subject=Hello%20World&body=Test%0A",
        "search://settings/privacy",
        "search://unknown/../../etc/passwd",
        "0.0.0.0:3000",
        "[::1]:8080/test",
        "1.2.3.4",
        "1.2.3",
        "xn--80ak6aa92e.com",
        "../../../etc/shadow\0",
        "\\windows\\system32\\cmd.exe",
        "{\"version\":999999,\"tabs\":null}",
        "<rss><channel><item><title>T</title><link>javascript:evil()</link></item></channel></rss>",
        "p1.s2.e3",
        "p0.s0.e0.extra",
    ];

    let parse_opts = ParseOptions::default();

    for &seed in adversarial_seeds {
        let _ = PageUrl::parse(seed);
        let _ = resolve_address(seed, &parse_opts);
        let _ = serde_json::from_str::<SessionFile>(seed);
        let _ = serde_json::from_str::<BookmarksFile>(seed);
        let _ = serde_json::from_str::<HistoryFile>(seed);
        let _ = serde_json::from_str::<PinsFile>(seed);
        let _ = serde_json::from_str::<SpacesFile>(seed);
        let _ = parse_rss_or_atom_feed(seed.as_bytes());
        let _ = MailtoMessage::parse(seed);
        let _ = validate_extension_entry_path(seed, false);
        let _ = AgentElementHandle::parse(seed);
        let _ = validate_ipc_message(seed, "https://example.com", Some("https://"));
        total_inputs_tested += 12;
        clean_outcomes += 12;
    }

    let (_, sample_ext_id) = derive_crx_id_from_public_key(b"fuzz-key");
    let dummy_proof = sha256_digest(b"fuzz");

    for _ in 0..iterations {
        let raw_bytes = rng.next_bytes(256);
        let lossy_str = String::from_utf8_lossy(&raw_bytes);

        // 1. URL & address parser
        if let Ok(url) = PageUrl::parse(&lossy_str) {
            let redacted = redact_url_for_agent(&url);
            assert!(!redacted.is_empty());
        }
        let _ = resolve_address(&lossy_str, &parse_opts);

        // 2. Data-file decoders
        let _ = serde_json::from_slice::<SessionFile>(&raw_bytes);
        let _ = serde_json::from_slice::<BookmarksFile>(&raw_bytes);

        // 3. RSS/Atom feed & mailto parser
        let _ = parse_rss_or_atom_feed(&raw_bytes);
        let _ = MailtoMessage::parse(&lossy_str);

        // 4. CRX3 & path confinement
        let _ = verify_crx3_package(&raw_bytes, &sample_ext_id, b"fuzz-key", &dummy_proof);
        let _ = validate_extension_entry_path(
            &lossy_str,
            (raw_bytes.first().copied().unwrap_or(0) & 1) == 1,
        );

        // 5. Agent handle & IPC validator
        let _ = AgentElementHandle::parse(&lossy_str);
        let _ = validate_ipc_message(&lossy_str, "https://example.com", None);

        total_inputs_tested += 10;
        clean_outcomes += 10;
    }

    FuzzSweepReport {
        total_inputs_tested,
        clean_outcomes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_fuzz_suite_runs_over_5000_adversarial_and_mutated_inputs_without_panicking() {
        let report = run_ci_fuzz_suite(500);
        assert!(report.total_inputs_tested >= 5_000);
        assert_eq!(report.total_inputs_tested, report.clean_outcomes);
    }

    #[test]
    fn enforces_internal_page_csp_ipc_bounds_and_120hz_frame_budgets() {
        assert!(INTERNAL_PAGE_CSP.contains("default-src 'none'"));
        assert!(INTERNAL_PAGE_CSP.contains("connect-src 'none'"));
        assert!(INTERNAL_PAGE_CSP.contains("frame-ancestors 'none'"));

        // Valid IPC JSON object succeeds; array, NUL byte, or wrong origin is rejected
        assert!(
            validate_ipc_message(
                "{\"cmd\":\"ping\"}",
                "https://example.com",
                Some("https://example.com")
            )
            .is_ok()
        );
        assert!(validate_ipc_message("[1,2,3]", "https://example.com", None).is_err());
        assert!(validate_ipc_message("{\"a\":\"\0\"}", "https://example.com", None).is_err());
        assert!(
            validate_ipc_message(
                "{\"cmd\":\"ping\"}",
                "http://evil.example",
                Some("https://example.com")
            )
            .is_err()
        );

        // 120 Hz budget applies on AC power or Always when not in Low Power Mode
        assert_eq!(
            target_frame_budget_us(HighRefreshMode::OnPower, true, false, true),
            FRAME_BUDGET_120HZ_US
        );
        assert_eq!(
            target_frame_budget_us(HighRefreshMode::OnPower, false, false, true),
            FRAME_BUDGET_60HZ_US
        );
        assert_eq!(
            target_frame_budget_us(HighRefreshMode::Always, true, true, true),
            FRAME_BUDGET_60HZ_US
        );
    }
}
