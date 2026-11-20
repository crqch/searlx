# searlx

**searlx is Search for Linux.** A small, quiet browser: tabs down the side or across the top, one field, and the page. No toolbar, no start page, no account, no telemetry. Pages run in the system WebKitGTK engine.

A Linux port of [Search](https://github.com/driceroland/Search), the macOS browser by Office Commun. It is a new Rust implementation, not a fork or a build of the original code. Built with gpui. Early days: the workspace is being scaffolded.

- [TODO.md](TODO.md) — progress and next steps
- [AGENTS.md](AGENTS.md) — architecture and build rules
- [FEATURES.md](FEATURES.md) — behavior inventory of the original macOS app

## Build

Requires Rust and the WebKitGTK development packages. The first workspace commit lands in Phase 0; until then there is nothing to build.

```sh
cargo run -p searlx
```
