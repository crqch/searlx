# Vendored `xattr` 0.2.3

This is an unmodified copy of `xattr` 0.2.3 except for one constant in
`src/sys/mod.rs`: on Linux, `ENOATTR` is defined as `libc::ENODATA` rather than
`libc::ENOATTR`. Current `libc` releases no longer export the BSD alias on
Linux, which breaks the build of `zed-async-tar` → `gpui_http_client` → `gpui`.

Upstream: https://github.com/Stebalien/xattr — MIT/Apache-2.0. Remove this patch
when `gpui` stops depending on `zed-async-tar` or `xattr` publishes a fix.
