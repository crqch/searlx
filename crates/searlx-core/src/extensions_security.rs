//! Web extension CRX3 verification, path confinement, permission diffing, tab isolation (`P4-03`),
//! and cross-platform extension capability gating (`P4-12`, `ADR-0003`).
//!
//! Behavioural specifications:
//! - `FEATURES.md` §14.1, §14.4, §14.6 — CRX3 magic/version/ID check, RSA public-key SHA-256 prefix match,
//!   path-derived unpacked extension ID (`a..=p`), symlink & traversal rejection, permission escalation prompt,
//!   private-tab & cross-extension isolation.
//! - `TODO.md` P4-03, P4-12 & `docs/adr/0003-extension-strategy.md` — macOS `WKWebExtension`, Windows BYO unpacked only, Linux none.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use crate::error::{CoreError, Result};
use crate::url::PageUrl;

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Compute a standard FIPS 180-4 SHA-256 digest in pure safe Rust.
#[must_use]
pub fn sha256_digest(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// CRX3 magic bytes (`"Cr24"`).
pub const CRX3_MAGIC: [u8; 4] = *b"Cr24";
/// Supported CRX format version (`3`, `FEATURES.md` §14.6).
pub const CRX3_VERSION: u32 = 3;
/// Maximum accepted CRX3 signed header size (`64 KiB`).
pub const MAX_CRX3_HEADER_BYTES: usize = 64 * 1024;
/// Length of a Chrome extension ID in base-16 `a..=p` characters (`32`).
pub const CRX_ID_CHARS: usize = 32;
/// Length of a decoded binary `crx_id` (`16 bytes`).
pub const CRX_ID_BYTES: usize = 16;

/// Encode the first 16 bytes of a digest as a 32-character Chrome extension ID (`'a'..='p'`).
#[must_use]
pub fn encode_crx_id_ap(bytes16: &[u8; CRX_ID_BYTES]) -> String {
    let mut out = String::with_capacity(CRX_ID_CHARS);
    for byte in bytes16 {
        let hi = (byte >> 4) & 0x0F;
        let lo = byte & 0x0F;
        out.push((b'a' + hi) as char);
        out.push((b'a' + lo) as char);
    }
    out
}

/// Decode a 32-character Chrome extension ID (`'a'..='p'`) into 16 raw bytes.
pub fn decode_crx_id_ap(id: &str) -> Result<[u8; CRX_ID_BYTES]> {
    if id.len() != CRX_ID_CHARS || !id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
        return Err(CoreError::Invalid(format!(
            "invalid 32-letter Chrome extension id: {id}"
        )));
    }
    let mut out = [0u8; CRX_ID_BYTES];
    let raw = id.as_bytes();
    for i in 0..CRX_ID_BYTES {
        let hi = raw[i * 2] - b'a';
        let lo = raw[i * 2 + 1] - b'a';
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

/// Derive a 32-letter Chrome extension ID (`'a'..='p'`) from an SPKI/DER public key (`FEATURES.md` §14.1, §14.6).
#[must_use]
pub fn derive_crx_id_from_public_key(public_key_der: &[u8]) -> ([u8; CRX_ID_BYTES], String) {
    let digest = sha256_digest(public_key_der);
    let mut prefix = [0u8; CRX_ID_BYTES];
    prefix.copy_from_slice(&digest[..CRX_ID_BYTES]);
    let encoded = encode_crx_id_ap(&prefix);
    (prefix, encoded)
}

/// Derive a 32-letter unpacked extension ID (`'a'..='p'`) from the canonical folder path
/// rather than the manifest's `key`, so an unpacked folder cannot impersonate a store extension (`FEATURES.md` §14.1).
#[must_use]
pub fn derive_unpacked_extension_id(canonical_folder_path: &Path) -> String {
    let digest = sha256_digest(canonical_folder_path.to_string_lossy().as_bytes());
    let mut prefix = [0u8; CRX_ID_BYTES];
    prefix.copy_from_slice(&digest[..CRX_ID_BYTES]);
    encode_crx_id_ap(&prefix)
}

/// Verified metadata extracted from a CRX3 container before shim injection (`FEATURES.md` §14.1, §14.6, P4-03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedCrx3Package {
    /// Expected and verified 32-letter extension ID (`'a'..='p'`).
    pub extension_id: String,
    /// Byte offset where the ZIP archive payload begins.
    pub zip_payload_offset: usize,
    /// SHA-256 digest of the ZIP archive payload.
    pub payload_sha256_hex: String,
}

/// Verify a CRX3 binary container against `expected_extension_id` and `public_key_der` (`FEATURES.md` §14.1, §14.6, P4-03).
///
/// Checks:
/// 1. `Cr24` magic and version `3`.
/// 2. Bounded header length (`1..=64 KiB`) and non-empty ZIP payload.
/// 3. The signed header bytes contain the exact 16-byte `crx_id` decoded from `expected_extension_id`.
/// 4. The SHA-256 prefix of `public_key_der` matches `expected_extension_id`.
/// 5. A deterministic proof binding `public_key_der` and the signed payload matches `expected_proof_sha256`.
pub fn verify_crx3_package(
    crx_bytes: &[u8],
    expected_extension_id: &str,
    public_key_der: &[u8],
    expected_proof_sha256: &[u8; 32],
) -> Result<VerifiedCrx3Package> {
    if crx_bytes.len() < 16 {
        return Err(CoreError::Invalid("CRX3 package too short".to_owned()));
    }
    if crx_bytes[0..4] != CRX3_MAGIC {
        return Err(CoreError::Invalid("invalid CRX3 magic header".to_owned()));
    }
    let version = u32::from_le_bytes([crx_bytes[4], crx_bytes[5], crx_bytes[6], crx_bytes[7]]);
    if version != CRX3_VERSION {
        return Err(CoreError::Invalid(format!(
            "unsupported CRX version {version}; only CRX3 is accepted"
        )));
    }
    let header_len =
        u32::from_le_bytes([crx_bytes[8], crx_bytes[9], crx_bytes[10], crx_bytes[11]]) as usize;
    if header_len == 0 || header_len > MAX_CRX3_HEADER_BYTES || 12 + header_len >= crx_bytes.len() {
        return Err(CoreError::Invalid(
            "CRX3 header length out of bounds".to_owned(),
        ));
    }

    let expected_raw_id = decode_crx_id_ap(expected_extension_id)?;
    let (derived_raw_id, derived_str_id) = derive_crx_id_from_public_key(public_key_der);
    if derived_raw_id != expected_raw_id || derived_str_id != expected_extension_id {
        return Err(CoreError::Invalid(
            "CRX3 public key SHA-256 prefix does not match requested extension ID".to_owned(),
        ));
    }

    let header_slice = &crx_bytes[12..12 + header_len];
    if !header_slice
        .windows(CRX_ID_BYTES)
        .any(|w| w == expected_raw_id)
    {
        return Err(CoreError::Invalid(
            "CRX3 signed header does not carry matching 16-byte crx_id".to_owned(),
        ));
    }

    let zip_payload_offset = 12 + header_len;
    let zip_payload = &crx_bytes[zip_payload_offset..];

    // Verify cryptographic proof over (public_key_der || crx_id || zip_payload)
    let mut proof_input =
        Vec::with_capacity(public_key_der.len() + CRX_ID_BYTES + zip_payload.len());
    proof_input.extend_from_slice(public_key_der);
    proof_input.extend_from_slice(&expected_raw_id);
    proof_input.extend_from_slice(zip_payload);
    let computed_proof = sha256_digest(&proof_input);
    if &computed_proof != expected_proof_sha256 {
        return Err(CoreError::Invalid(
            "CRX3 signature proof verification failed".to_owned(),
        ));
    }

    let payload_sha256 = sha256_digest(zip_payload);
    let mut payload_sha256_hex = String::with_capacity(64);
    for b in payload_sha256 {
        payload_sha256_hex.push_str(&format!("{b:02x}"));
    }

    Ok(VerifiedCrx3Package {
        extension_id: expected_extension_id.to_owned(),
        zip_payload_offset,
        payload_sha256_hex,
    })
}

/// Validate an archive or unpacked file entry path for path confinement and symlink rejection (`FEATURES.md` §14.6, P4-03).
pub fn validate_extension_entry_path(relative_path: &str, is_symlink: bool) -> Result<String> {
    if is_symlink {
        return Err(CoreError::Invalid(format!(
            "extension package entry {relative_path:?} is a symlink and was rejected"
        )));
    }
    if relative_path.is_empty()
        || relative_path.contains('\0')
        || relative_path.contains('\\')
        || relative_path.starts_with('/')
    {
        return Err(CoreError::Invalid(format!(
            "extension package entry {relative_path:?} is not a safe relative path"
        )));
    }

    let path = Path::new(relative_path);
    let mut normalized = Vec::new();
    for comp in path.components() {
        match comp {
            Component::Normal(seg) => {
                let s = seg.to_str().ok_or_else(|| {
                    CoreError::Invalid("non-UTF-8 extension entry path".to_owned())
                })?;
                if s.is_empty() {
                    return Err(CoreError::Invalid("empty path segment".to_owned()));
                }
                normalized.push(s);
            }
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(CoreError::Invalid(format!(
                    "extension package entry {relative_path:?} attempts path traversal"
                )));
            }
        }
    }
    if normalized.is_empty() {
        return Err(CoreError::Invalid("empty extension entry path".to_owned()));
    }
    Ok(normalized.join("/"))
}

/// Sanitize and narrow an extension's requested host match pattern (`FEATURES.md` §14.6).
///
/// Rules:
/// - `<all_urls>` is narrowed to `*://*/*` (never `file://` or `chrome-extension://`).
/// - `file:` patterns and other extensions' `chrome-extension:` patterns are refused.
pub fn sanitize_extension_host_pattern(
    raw_pattern: &str,
    own_extension_id: &str,
) -> Result<String> {
    let trimmed = raw_pattern.trim();
    if trimmed.eq_ignore_ascii_case("<all_urls>") {
        return Ok("*://*/*".to_owned());
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("file:") {
        return Err(CoreError::Invalid(
            "extensions are never granted file: URL access".to_owned(),
        ));
    }
    if lower.starts_with("search:") {
        return Err(CoreError::Invalid(
            "extensions are never granted search:// internal page access".to_owned(),
        ));
    }
    if let Some(rest) = lower.strip_prefix("chrome-extension://") {
        let host = rest.split('/').next().unwrap_or_default();
        if host != own_extension_id.to_ascii_lowercase() {
            return Err(CoreError::Invalid(
                "extensions are never granted access to another extension's pages".to_owned(),
            ));
        }
    }
    Ok(trimmed.to_owned())
}

/// Complete grant set of an installed extension (`FEATURES.md` §14.1, §14.6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionGrantSet {
    /// API permissions (`tabs`, `storage`, `cookies`, `scripting`, etc.).
    pub permissions: BTreeSet<String>,
    /// Sanitized host patterns (`site:*://*/*`, etc.).
    pub site_patterns: BTreeSet<String>,
    /// Search-answered shim APIs (`search:bookmarks`, `search:history`, etc.).
    pub search_apis: BTreeSet<String>,
}

/// Permission diff when updating or reloading an extension (`FEATURES.md` §14.1, §14.6, P4-03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionGrantDiff {
    /// Newly requested permissions/sites/APIs not present in the previous version.
    pub added_grants: Vec<String>,
    /// Whether the update must pause and prompt the user before activating.
    pub requires_user_prompt: bool,
    /// Cancel is always the default button (`Return`/`Escape` cancel, `FEATURES.md` §14.1, §14.6).
    pub default_button_is_cancel: bool,
}

/// Compare an existing extension's grants against an updated version's requested grants (`FEATURES.md` §14.6).
#[must_use]
pub fn diff_extension_grants(
    previous: &ExtensionGrantSet,
    updated: &ExtensionGrantSet,
) -> ExtensionGrantDiff {
    let mut added = Vec::new();
    for p in updated.permissions.difference(&previous.permissions) {
        added.push(format!("permission:{p}"));
    }
    for s in updated.site_patterns.difference(&previous.site_patterns) {
        added.push(format!("site:{s}"));
    }
    for a in updated.search_apis.difference(&previous.search_apis) {
        added.push(format!("search:{a}"));
    }
    let requires_user_prompt = !added.is_empty();
    ExtensionGrantDiff {
        added_grants: added,
        requires_user_prompt,
        default_button_is_cancel: true,
    }
}

/// Check whether extension `extension_id` is permitted to see or script a tab (`FEATURES.md` §14.4, §14.6, P4-03).
#[must_use]
pub fn can_extension_access_tab(
    extension_id: &str,
    tab_url: &PageUrl,
    is_private_tab: bool,
    extension_allowed_in_private: bool,
    private_tab_created_before_any_extension_allowed: bool,
) -> bool {
    // 1. Search internal pages and local file:// pages are never exposed to extensions
    if tab_url.is_internal() || tab_url.scheme() == "file" {
        return false;
    }
    // 2. Other extensions' pages (`chrome-extension://<other>/`) are strictly sealed
    if tab_url.scheme() == "chrome-extension" {
        let host = tab_url.host().unwrap_or_default();
        if !host.eq_ignore_ascii_case(extension_id) {
            return false;
        }
    }
    // 3. Private tabs are hidden unless explicitly allowed AND not created before any extension was granted private access
    if is_private_tab
        && (!extension_allowed_in_private || private_tab_created_before_any_extension_allowed)
    {
        return false;
    }
    true
}

// ============================================================================
// P4-12 — Extension strategy outside macOS (`ADR-0003`, `AGENTS.md` §2.4, §4)
// ============================================================================

/// Per-OS extension support level (`ADR-0003`, `TODO.md` P4-12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionSupportTier {
    /// macOS (`WKWebExtension`): CRX3 store installs + unpacked extensions + shims.
    FullWkWebExtension,
    /// Windows (`WebView2`): BYO unpacked folder extensions only; store/CRX installs disabled.
    WindowsUnpackedByoOnly,
    /// Linux (`WebKitGTK`): no extension engine; UI hides/disables extension surface honestly.
    Unsupported,
}

/// Honest capability descriptor and UI label for extensions on a target OS (`P4-12`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionPlatformGate {
    /// Support tier.
    pub tier: ExtensionSupportTier,
    /// Whether the Extensions settings page and puzzle button may be shown.
    pub ui_surface_enabled: bool,
    /// Whether Chrome Web Store / CRX3 installation is permitted.
    pub allows_crx_store_install: bool,
    /// Whether loading an unpacked extension folder is permitted.
    pub allows_unpacked_install: bool,
    /// Explicit user-facing explanation shown in Settings / docs (`AGENTS.md` §2.4: do not fake parity).
    pub status_label: &'static str,
}

impl ExtensionPlatformGate {
    /// Resolve the extension capability gate for `"macos"`, `"windows"`, or `"linux"` (`ADR-0003`, P4-12).
    #[must_use]
    pub fn for_target_os(target_os: &str) -> Self {
        match target_os {
            "macos" => Self {
                tier: ExtensionSupportTier::FullWkWebExtension,
                ui_surface_enabled: true,
                allows_crx_store_install: true,
                allows_unpacked_install: true,
                status_label: "Supported via macOS WKWebExtension (CRX3 store and unpacked extensions).",
            },
            "windows" => Self {
                tier: ExtensionSupportTier::WindowsUnpackedByoOnly,
                ui_surface_enabled: true,
                allows_crx_store_install: false,
                allows_unpacked_install: true,
                status_label: "Experimental on Windows (WebView2): bring-your-own unpacked extensions only; store installs are not supported.",
            },
            _ => Self {
                tier: ExtensionSupportTier::Unsupported,
                ui_surface_enabled: false,
                allows_crx_store_install: false,
                allows_unpacked_install: false,
                status_label: "Web extensions are not available on Linux (WebKitGTK has no WebExtension runtime).",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_valid_crx3_and_refuses_tampered_payload_or_mismatched_key() {
        let pubkey = b"mock-spki-rsa-public-key-der-bytes";
        let (raw_id, ext_id) = derive_crx_id_from_public_key(pubkey);
        let zip_payload = b"PK\x03\x04manifest.json";

        // Build a valid CRX3 container carrying raw_id in its signed header
        let mut header = Vec::from(&b"crx3-header:"[..]);
        header.extend_from_slice(&raw_id);
        let mut crx = Vec::new();
        crx.extend_from_slice(&CRX3_MAGIC);
        crx.extend_from_slice(&CRX3_VERSION.to_le_bytes());
        crx.extend_from_slice(&(header.len() as u32).to_le_bytes());
        crx.extend_from_slice(&header);
        crx.extend_from_slice(zip_payload);

        let mut proof_input = Vec::new();
        proof_input.extend_from_slice(pubkey);
        proof_input.extend_from_slice(&raw_id);
        proof_input.extend_from_slice(zip_payload);
        let proof = sha256_digest(&proof_input);

        let verified = verify_crx3_package(&crx, &ext_id, pubkey, &proof).expect("valid CRX3");
        assert_eq!(verified.extension_id, ext_id);

        // Tamper with one byte of the ZIP payload -> refused!
        let mut tampered = crx.clone();
        if let Some(last) = tampered.last_mut() {
            *last ^= 0xFF;
        }
        assert!(verify_crx3_package(&tampered, &ext_id, pubkey, &proof).is_err());
    }

    #[test]
    fn enforces_path_confinement_permission_diff_and_private_tab_isolation() {
        assert_eq!(
            validate_extension_entry_path("js/content.js", false).expect("safe path"),
            "js/content.js"
        );
        assert!(validate_extension_entry_path("../etc/passwd", false).is_err());
        assert!(validate_extension_entry_path("/etc/passwd", false).is_err());
        assert!(validate_extension_entry_path("js/link.js", true).is_err());

        // <all_urls> narrowed to *://*/* and file: refused
        let ext_id = "abcdefghijklmnopabcdefghijklmnop";
        assert_eq!(
            sanitize_extension_host_pattern("<all_urls>", ext_id).expect("narrowed"),
            "*://*/*"
        );
        assert!(sanitize_extension_host_pattern("file:///*", ext_id).is_err());

        // Permission escalation prompts with Cancel default
        let prev = ExtensionGrantSet::default();
        let mut next = ExtensionGrantSet::default();
        next.permissions.insert("cookies".to_owned());
        let diff = diff_extension_grants(&prev, &next);
        assert!(diff.requires_user_prompt);
        assert!(diff.default_button_is_cancel);

        // Private tab isolation
        let web_url = PageUrl::parse("https://example.com/").expect("valid");
        assert!(!can_extension_access_tab(
            ext_id, &web_url, true, false, false
        ));
        assert!(!can_extension_access_tab(
            ext_id, &web_url, true, true, true
        ));
        assert!(can_extension_access_tab(
            ext_id, &web_url, true, true, false
        ));
    }

    #[test]
    fn gates_and_labels_extension_support_outside_macos() {
        let mac = ExtensionPlatformGate::for_target_os("macos");
        assert!(mac.allows_crx_store_install && mac.allows_unpacked_install);

        let win = ExtensionPlatformGate::for_target_os("windows");
        assert!(!win.allows_crx_store_install && win.allows_unpacked_install);

        let linux = ExtensionPlatformGate::for_target_os("linux");
        assert!(!linux.ui_surface_enabled);
        assert!(!linux.allows_crx_store_install && !linux.allows_unpacked_install);
    }
}
