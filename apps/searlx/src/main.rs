//! The searlx composition root.
//!
//! This is the only crate that wires concrete platform and engine
//! implementations together. Phase 0 ships the gpui shell on the fake engine;
//! native backends land behind it as they pass their contract tests.

use anyhow::{Context, Result};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("search=info,warn")),
        )
        .with_target(true)
        .init();

    tracing::info!(target: "search", platform = searlx_platform::platform_name(), "starting");

    searlx_ui::run().context("the UI failed to start")?;
    Ok(())
}
