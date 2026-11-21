//! Headless harness and contract tests for engine backends.
//!
//! A backend is not "supported" because it compiles. It must pass these checks
//! on its target OS; the CI/manual matrix records where each one ran.

use searlx_core::{PageUrl, ProfileId, SpaceId};
use searlx_engine::{
    DataKinds, Engine, EngineEvent, FindOptions, InjectAt, Rect, ScriptWorld, TabOptions,
};

pub mod bench;

pub use bench::{PerformanceBudgets, PerformanceMeasurements, verify_performance_budgets};

/// A failure from a contract check.
#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    /// A backend call failed.
    #[error("engine call failed: {0}")]
    Call(#[from] searlx_engine::EngineError),

    /// The event stream did not contain what the contract requires.
    #[error("{0}")]
    Event(String),

    /// A state assertion failed.
    #[error("{0}")]
    State(String),
}

/// The contract result.
pub type ContractResult<T> = Result<T, ContractError>;

/// Run the Phase 0 lifecycle contract against an engine.
///
/// Covers create → load → commit → back → forward → close, event ordering, and
/// the requirement that closing an unknown tab is an error.
#[allow(clippy::too_many_lines)]
pub fn lifecycle<E: Engine>(engine: &mut E) -> ContractResult<()> {
    let space = SpaceId::new();
    let profile = ProfileId::new();
    let first = engine.create_tab(TabOptions {
        space,
        profile,
        ..TabOptions::default()
    })?;
    let second = engine.create_tab(TabOptions {
        space,
        profile,
        ..TabOptions::default()
    })?;
    if first == second {
        return Err(ContractError::State(
            "two tabs received the same handle".into(),
        ));
    }

    let one = page("https://example.com/one")?;
    let two = page("https://example.com/two")?;
    engine.navigate(first, one.clone())?;
    let events = engine.pump()?;
    expect_committed(&events, first, &one)?;
    expect_finished(&events, first, &one)?;

    engine.navigate(first, two.clone())?;
    engine.pump()?;
    engine.back(first)?;
    let events = engine.pump()?;
    expect_committed(&events, first, &one)?;
    engine.forward(first)?;
    let events = engine.pump()?;
    expect_committed(&events, first, &two)?;

    engine.set_bounds(
        first,
        Rect {
            x: 10.0,
            y: 20.0,
            width: 800.0,
            height: 600.0,
        },
    )?;
    engine.set_visible(first, false)?;
    engine.set_visible(first, true)?;
    engine.focus(second)?;
    engine.set_muted(second, true)?;
    engine.set_zoom(second, 1.25)?;
    engine.eval(first, ScriptWorld::Isolated, "void 0")?;
    engine.inject(first, ScriptWorld::Page, InjectAt::DocumentStart, "void 0")?;
    let found = engine.find(first, "example", FindOptions::default())?;
    if found.query != "example" {
        return Err(ContractError::State("find did not echo its query".into()));
    }
    let snapshot = engine.snapshot(first)?;
    if snapshot.png.is_empty() {
        return Err(ContractError::State("snapshot returned no bytes".into()));
    }

    let store = engine.data_store(space, profile, false)?;
    engine.clear_data(
        store,
        DataKinds {
            cookies: true,
            ..DataKinds::none()
        },
    )?;
    let private = engine.data_store(space, profile, true)?;
    if !private.ephemeral {
        return Err(ContractError::State(
            "a private store was not marked ephemeral".into(),
        ));
    }

    engine.close_tab(first)?;
    engine.close_tab(second)?;
    if engine.close_tab(first).is_ok() {
        return Err(ContractError::State(
            "closing a missing tab succeeded".into(),
        ));
    }
    Ok(())
}

/// Format a deterministic, redacted line-by-line transcript of [`EngineEvent`]s
/// for golden-file assertions (`AGENTS.md` §8).
///
/// Full URLs with query values or fragments are never written; [`PageUrl::redacted`]
/// is used everywhere.
#[must_use]
pub fn format_event_transcript(events: &[EngineEvent]) -> String {
    let mut out = String::new();
    for event in events {
        let line = match event {
            EngineEvent::NavigationStarted { tab, url } => {
                format!("tab={tab} NavigationStarted url={}", url.redacted())
            }
            EngineEvent::NavigationCommitted { tab, url } => {
                format!("tab={tab} NavigationCommitted url={}", url.redacted())
            }
            EngineEvent::NavigationFinished { tab, url } => {
                format!("tab={tab} NavigationFinished url={}", url.redacted())
            }
            EngineEvent::NavigationFailed { tab, url, reason } => {
                format!(
                    "tab={tab} NavigationFailed url={} reason={reason}",
                    url.redacted()
                )
            }
            EngineEvent::TitleChanged { tab, title } => {
                format!("tab={tab} TitleChanged title={title:?}")
            }
            EngineEvent::UrlChanged { tab, url } => {
                format!("tab={tab} UrlChanged url={}", url.redacted())
            }
            EngineEvent::FaviconChanged { tab, url } => {
                let redacted = url
                    .as_ref()
                    .map_or_else(|| "none".into(), PageUrl::redacted);
                format!("tab={tab} FaviconChanged url={redacted}")
            }
            EngineEvent::LoadingChanged { tab, loading } => {
                format!("tab={tab} LoadingChanged loading={loading}")
            }
            EngineEvent::HistoryChanged {
                tab,
                can_go_back,
                can_go_forward,
            } => {
                format!("tab={tab} HistoryChanged back={can_go_back} forward={can_go_forward}")
            }
            EngineEvent::NewWindowRequested {
                tab,
                url,
                disposition,
            } => {
                format!(
                    "tab={tab} NewWindowRequested url={} disposition={disposition:?}",
                    url.redacted()
                )
            }
            EngineEvent::DownloadRequested {
                tab,
                url,
                suggested_name,
            } => {
                format!(
                    "tab={tab} DownloadRequested url={} name={suggested_name:?}",
                    url.redacted()
                )
            }
            EngineEvent::PermissionRequested { tab, origin, kind } => {
                format!("tab={tab} PermissionRequested origin={origin} kind={kind:?}")
            }
            EngineEvent::ScriptDialog { tab, kind, message } => {
                format!("tab={tab} ScriptDialog kind={kind:?} message={message:?}")
            }
            EngineEvent::FilePickerRequested { tab, multiple } => {
                format!("tab={tab} FilePickerRequested multiple={multiple}")
            }
            EngineEvent::AuthRequired { tab, origin } => {
                format!("tab={tab} AuthRequired origin={origin}")
            }
            EngineEvent::CertificateError { tab, url, reason } => {
                format!(
                    "tab={tab} CertificateError url={} reason={reason}",
                    url.redacted()
                )
            }
            EngineEvent::MediaStateChanged {
                tab,
                audible,
                muted,
            } => {
                format!("tab={tab} MediaStateChanged audible={audible} muted={muted}")
            }
            EngineEvent::FullscreenChanged { tab, fullscreen } => {
                format!("tab={tab} FullscreenChanged fullscreen={fullscreen}")
            }
            EngineEvent::ProcessCrashed { tab, reason } => {
                format!("tab={tab} ProcessCrashed reason={reason}")
            }
            EngineEvent::ScriptMessage { tab, world, body } => {
                format!("tab={tab} ScriptMessage world={world} body={body:?}")
            }
            EngineEvent::SnapshotReady { tab, image } => {
                format!(
                    "tab={tab} SnapshotReady width={} height={} bytes={}",
                    image.width,
                    image.height,
                    image.png.len()
                )
            }
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Assert that a committed navigation for `url` happened in `tab`.
pub fn expect_committed(
    events: &[EngineEvent],
    tab: searlx_core::TabId,
    url: &PageUrl,
) -> ContractResult<()> {
    if events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::NavigationCommitted { tab: event_tab, url: event_url }
                if *event_tab == tab && event_url.identity() == url.identity()
        )
    }) {
        Ok(())
    } else {
        Err(ContractError::Event(format!(
            "no NavigationCommitted for {} in {tab}",
            url.redacted()
        )))
    }
}

/// Assert that a finished navigation for `url` happened in `tab`.
pub fn expect_finished(
    events: &[EngineEvent],
    tab: searlx_core::TabId,
    url: &PageUrl,
) -> ContractResult<()> {
    if events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::NavigationFinished { tab: event_tab, url: event_url }
                if *event_tab == tab && event_url.identity() == url.identity()
        )
    }) {
        Ok(())
    } else {
        Err(ContractError::Event(format!(
            "no NavigationFinished for {} in {tab}",
            url.redacted()
        )))
    }
}

fn page(value: &str) -> ContractResult<PageUrl> {
    PageUrl::parse(value).map_err(|error| ContractError::State(format!("bad fixture URL: {error}")))
}
