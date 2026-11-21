//! Packaging skeleton and runtime prerequisite checks (`FEATURES.md` §18.1, §20.1, P1-13).
//!
//! - On macOS, generates the `.app` bundle's `Info.plist` declaring `http`/`https`
//!   URL handlers and safe document types (`HTML`, `XHTML`, `PDF`, `PNG`, `JPEG`,
//!   `GIF`, `HEIC`, `WebP`, `TIFF`, `Plain Text`, `JSON` — never `SVG`, which
//!   carries a script execution risk per `FEATURES.md` §18.1).
//! - On Windows, checks whether the Microsoft Edge WebView2 Evergreen Runtime
//!   is present and produces a clear, actionable message if it is missing.

use std::path::{Path, PathBuf};

/// Status of the Windows WebView2 Evergreen Runtime check (`P1-13`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebView2Status {
    /// WebView2 Evergreen Runtime is installed with the given version string.
    Available {
        /// Detected version (e.g. `"131.0.2903.70"`).
        version: String,
    },
    /// WebView2 Evergreen Runtime was not found; `user_message` explains how to install it.
    Missing {
        /// Clear user-facing message shown when WebView2 is absent.
        user_message: String,
    },
    /// Running on a non-Windows host where WebView2 does not apply.
    NotApplicable,
}

/// Official WebView2 Evergreen Runtime client GUID in the Windows registry.
pub const WEBVIEW2_CLIENT_GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// User-facing message displayed on Windows when the WebView2 Evergreen Runtime is missing (`P1-13`).
pub const WEBVIEW2_MISSING_MESSAGE: &str = "searlx requires the Microsoft Edge WebView2 Evergreen Runtime, which was not found on this Windows installation.\n\nInstall the Evergreen Bootstrapper or Standalone Installer from:\nhttps://developer.microsoft.com/microsoft-edge/webview2/";

/// Evaluate a probed WebView2 version string (`pv` registry value) into a [`WebView2Status`].
#[must_use]
pub fn evaluate_webview2_version(probed_version: Option<&str>) -> WebView2Status {
    match probed_version.map(str::trim) {
        Some(v) if !v.is_empty() && v != "0.0.0.0" => WebView2Status::Available {
            version: v.to_owned(),
        },
        _ => WebView2Status::Missing {
            user_message: WEBVIEW2_MISSING_MESSAGE.to_owned(),
        },
    }
}

/// Check WebView2 availability on the current host.
#[must_use]
pub fn check_webview2_runtime() -> WebView2Status {
    #[cfg(target_os = "windows")]
    {
        // Check standard Evergreen installation paths when registry bindings are not linked.
        let candidates = [
            std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
            std::env::var_os("ProgramFiles").map(PathBuf::from),
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        ];
        for base in candidates.into_iter().flatten() {
            let dir = base
                .join("Microsoft")
                .join("EdgeWebView")
                .join("Application");
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if let Some(version) = name.to_str()
                        && version.chars().next().is_some_and(|c| c.is_ascii_digit())
                        && entry.path().join("msedgewebview2.exe").exists()
                    {
                        return evaluate_webview2_version(Some(version));
                    }
                }
            }
        }
        evaluate_webview2_version(None)
    }
    #[cfg(not(target_os = "windows"))]
    {
        WebView2Status::NotApplicable
    }
}

/// Allowed file extensions for `CFBundleDocumentTypes` (`FEATURES.md` §18.1).
/// `svg` is deliberately excluded because SVG files can execute script.
pub const ALLOWED_DOCUMENT_EXTENSIONS: &[&str] = &[
    "html",
    "htm",
    "xhtml",
    "xht",
    "pdf",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "heic",
    "webp",
    "tif",
    "tiff",
    "txt",
    "json",
    "webarchive",
];

/// Generate the `Contents/Info.plist` XML for a macOS `searlx.app` bundle (`P1-13`, `FEATURES.md` §18.1, §20.1).
#[must_use]
pub fn macos_info_plist(version: &str, build_number: &str, dev_build: bool) -> String {
    let bundle_name = if dev_build { "searlx Dev" } else { "searlx" };
    let bundle_id = if dev_build {
        "com.officecommun.search.dev"
    } else {
        "com.officecommun.search"
    };
    let ext_entries: String = ALLOWED_DOCUMENT_EXTENSIONS
        .iter()
        .map(|ext| format!("        <string>{ext}</string>\n"))
        .collect();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>{bundle_name}</string>
  <key>CFBundleExecutable</key>
  <string>searlx</string>
  <key>CFBundleIdentifier</key>
  <string>{bundle_id}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>{bundle_name}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>{version}</string>
  <key>CFBundleVersion</key>
  <string>{build_number}</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key>
      <string>Web site URL</string>
      <key>CFBundleURLSchemes</key>
      <array>
        <string>http</string>
        <string>https</string>
      </array>
    </dict>
  </array>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key>
      <string>Web and Document Files</string>
      <key>CFBundleTypeRole</key>
      <string>Viewer</string>
      <key>LSHandlerRank</key>
      <string>Alternate</string>
      <key>CFBundleTypeExtensions</key>
      <array>
{ext_entries}      </array>
    </dict>
  </array>
</dict>
</plist>
"#
    )
}

/// Stage a macOS `.app` bundle directory layout (`Contents/Info.plist`,
/// `Contents/MacOS/searlx`, `Contents/Resources`) at `bundle_dir` (`P1-13`).
pub fn stage_macos_app_bundle(
    bundle_dir: &Path,
    binary_source: &Path,
    version: &str,
    build_number: &str,
    dev_build: bool,
) -> std::io::Result<PathBuf> {
    let contents = bundle_dir.join("Contents");
    let macos_dir = contents.join("MacOS");
    let resources_dir = contents.join("Resources");
    std::fs::create_dir_all(&macos_dir)?;
    std::fs::create_dir_all(&resources_dir)?;

    let plist_path = contents.join("Info.plist");
    std::fs::write(
        &plist_path,
        macos_info_plist(version, build_number, dev_build),
    )?;

    let dest_binary = macos_dir.join("searlx");
    std::fs::copy(binary_source, &dest_binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&dest_binary)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dest_binary, perms)?;
    }

    Ok(dest_binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webview2_check_reports_clear_message_when_missing() {
        let missing = evaluate_webview2_version(None);
        assert!(matches!(
            missing,
            WebView2Status::Missing { ref user_message } if user_message.contains("WebView2 Evergreen Runtime")
        ));

        let zero = evaluate_webview2_version(Some("0.0.0.0"));
        assert!(matches!(zero, WebView2Status::Missing { .. }));

        let present = evaluate_webview2_version(Some("131.0.2903.70"));
        assert_eq!(
            present,
            WebView2Status::Available {
                version: "131.0.2903.70".into()
            }
        );
    }

    #[test]
    fn macos_info_plist_includes_xhtml_and_excludes_svg() {
        let plist = macos_info_plist("0.1.0", "20261121", false);
        assert!(plist.contains("<string>http</string>"));
        assert!(plist.contains("<string>https</string>"));
        assert!(plist.contains("<string>xhtml</string>"));
        assert!(plist.contains("<string>pdf</string>"));
        assert!(
            !plist.contains("<string>svg</string>"),
            "SVG must never be registered as a document type"
        );
    }

    #[test]
    fn stages_macos_app_bundle_layout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fake_bin = temp.path().join("searlx-bin");
        std::fs::write(&fake_bin, b"#!/bin/sh\nexit 0\n").expect("write fake binary");

        let bundle = temp.path().join("searlx.app");
        let exe = stage_macos_app_bundle(&bundle, &fake_bin, "0.1.0", "1", false)
            .expect("stage app bundle");
        assert!(exe.exists());
        assert!(bundle.join("Contents/Info.plist").exists());
        assert!(bundle.join("Contents/Resources").is_dir());
    }
}
