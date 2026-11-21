#!/usr/bin/env bash
# Build and ad-hoc sign a macOS searlx.app bundle (P1-13, FEATURES.md §20.1).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROFILE="${SEARLX_PROFILE:-release}"
VERSION="${SEARLX_VERSION:-0.0.0}"
BUILD_NUMBER="${SEARLX_BUILD_NUMBER:-$(date -u +%Y%m%d%H%M)}"
BUNDLE_DIR="$ROOT/build/searlx.app"

if [ "$PROFILE" = "release" ]; then
  cargo build --release -p searlx
  BIN="$ROOT/target/release/searlx"
else
  cargo build -p searlx
  BIN="$ROOT/target/debug/searlx"
fi

rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/Contents/MacOS" "$BUNDLE_DIR/Contents/Resources"
cp "$BIN" "$BUNDLE_DIR/Contents/MacOS/searlx"
chmod 755 "$BUNDLE_DIR/Contents/MacOS/searlx"

cat > "$BUNDLE_DIR/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>searlx</string>
  <key>CFBundleExecutable</key>
  <string>searlx</string>
  <key>CFBundleIdentifier</key>
  <string>com.officecommun.search</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>searlx</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${BUILD_NUMBER}</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

if [ "$(uname -s)" = "Darwin" ] && command -v codesign >/dev/null 2>&1; then
  SIGN_ID="${SEARCH_SIGN_IDENTITY:--}"
  codesign --force --deep --sign "$SIGN_ID" "$BUNDLE_DIR"
fi

echo "Staged $BUNDLE_DIR"
