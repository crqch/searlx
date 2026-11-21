//! The searlx composition root.
//!
//! This is the only crate that wires concrete platform and engine
//! implementations together. Phase 0 ships the gpui shell on the fake engine;
//! native backends land behind it as they pass their contract tests.

use anyhow::{Context, Result};
use searlx_core::ai_agents_bench::BenchWorldConfig;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("search=info,warn")),
        )
        .with_target(true)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let probe_arg = args
        .iter()
        .any(|a| a == "--probe" || a.starts_with("--probe="));
    let probe_env = std::env::var("SEARCH_PROBE")
        .ok()
        .filter(|v| !v.trim().is_empty() && v.trim() != "0");

    if probe_arg || probe_env.is_some() {
        let world_raw = probe_env.as_deref().unwrap_or("test");
        let temp_dir = std::env::temp_dir();
        let mut world = BenchWorldConfig::from_probe_env(world_raw, &temp_dir)
            .context("invalid probe world")?;
        let status = world
            .execute_verb("probe", None, false, false)
            .context("headless probe execution failed")?;
        tracing::info!(
            target: "search",
            platform = searlx_platform::platform_name(),
            world = %world.world_name,
            status = %status,
            "headless probe completed"
        );
        return Ok(());
    }

    tracing::info!(target: "search", platform = searlx_platform::platform_name(), "starting");

    searlx_ui::run().context("the UI failed to start")?;
    Ok(())
}
