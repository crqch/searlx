//! Thread-safe `Send + 'static` dispatch over GTK/WebKitGTK main-thread state.
//!
//! `webkit2gtk::WebView`, `WebContext`, and `gtk::Widget` are `!Send` and bound
//! to the GTK main thread. To satisfy `searlx_engine::Engine: Send + 'static`
//! safely without any `unsafe` code:
//! - Concrete WebKitGTK engine instances live in a `thread_local!` registry on
//!   the GTK main thread, keyed by a monotonic `u64` engine ID.
//! - If GTK was already initialized on the calling thread (`apps/searlx` GTK
//!   shell), methods called from that main thread run directly with zero
//!   thread-hopping.
//! - If GTK has not been initialized yet (headless callers or parallel
//!   `cargo test` threads), a single process-wide GTK main thread running
//!   `glib::MainLoop` is started once so multiple threads never violate GTK's
//!   single-thread initialization invariant.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::Duration;

use gtk::gdk;
use searlx_engine::{EngineError, EngineResult};

use crate::webkit_engine::InnerWebKitEngine;

static NEXT_ENGINE_ID: AtomicU64 = AtomicU64::new(1);
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static WORKER: OnceLock<Result<(), String>> = OnceLock::new();

thread_local! {
    static LOCAL_ENGINES: RefCell<BTreeMap<u64, Rc<RefCell<InnerWebKitEngine>>>> =
        const { RefCell::new(BTreeMap::new()) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DispatchMode {
    /// GTK was initialized on the caller's thread (e.g. the GTK shell main thread).
    CurrentMainThread,
    /// GTK is owned by our process-wide `"searlx-webkitgtk-main"` thread running `MainLoop`.
    DedicatedWorker,
    /// GTK was initialized on another thread externally.
    DefaultMainContext,
}

/// Validate that a display session env var exists (unless `GDK_BACKEND` is
/// explicitly overridden) and configure allowed GDK backends per ADR-0002.
fn configure_display_backends() -> Result<(), String> {
    if std::env::var_os("GDK_BACKEND").is_some() {
        return Ok(());
    }
    let has_x11 = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
    let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    match (has_x11, has_wayland) {
        (true, _) => {
            // Prefer X11/XWayland when DISPLAY is set (ADR-0002); allow Wayland
            // fallback, and exclude Broadway unless explicitly requested.
            gdk::set_allowed_backends("x11,wayland");
            Ok(())
        }
        (false, true) => {
            gdk::set_allowed_backends("wayland,x11");
            Ok(())
        }
        (false, false) => Err(
            "GTK could not initialize display (neither DISPLAY nor WAYLAND_DISPLAY is set)"
                .to_owned(),
        ),
    }
}

/// Ensure the process-wide GTK main thread is initialized and running `glib::MainLoop`.
fn ensure_worker_initialized() -> EngineResult<()> {
    let result = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let spawn_res = std::thread::Builder::new()
            .name("searlx-webkitgtk-main".to_owned())
            .spawn(move || {
                if let Err(msg) = configure_display_backends() {
                    let _ = tx.send(Err(msg));
                    return;
                }
                if let Err(err) = gtk::init() {
                    let _ = tx.send(Err(format!(
                        "GTK could not initialize display (ensure DISPLAY or WAYLAND_DISPLAY is set): {err}"
                    )));
                    return;
                }
                WORKER_STARTED.store(true, Ordering::Release);
                let _ = tx.send(Ok(()));
                let main_loop = glib::MainLoop::new(None, false);
                main_loop.run();
            });
        if let Err(err) = spawn_res {
            return Err(format!("failed to spawn GTK main thread: {err}"));
        }
        rx.recv()
            .map_err(|_| "GTK main thread exited before reporting init status".to_owned())?
    });

    match result {
        Ok(()) => Ok(()),
        Err(msg) => Err(EngineError::Backend(msg.clone())),
    }
}

/// Initialize GTK on the current thread if not initialized yet, and bind to it.
pub(crate) fn init_current_thread_for_gtk() -> EngineResult<DispatchMode> {
    if gtk::is_initialized_main_thread() {
        return Ok(DispatchMode::CurrentMainThread);
    }
    if gtk::is_initialized() {
        return Err(EngineError::Backend(
            "GTK was already initialized on a different thread".into(),
        ));
    }
    configure_display_backends().map_err(EngineError::Backend)?;
    gtk::init().map_err(|err| {
        EngineError::Backend(format!(
            "GTK could not initialize display (ensure DISPLAY or WAYLAND_DISPLAY is set): {err}"
        ))
    })?;
    Ok(DispatchMode::CurrentMainThread)
}

/// Determine the appropriate dispatch mode for a new `WebKitGtkEngine`.
pub(crate) fn select_dispatch_mode() -> EngineResult<DispatchMode> {
    if gtk::is_initialized_main_thread() {
        Ok(DispatchMode::CurrentMainThread)
    } else if gtk::is_initialized() {
        if WORKER_STARTED.load(Ordering::Acquire) {
            Ok(DispatchMode::DedicatedWorker)
        } else {
            Ok(DispatchMode::DefaultMainContext)
        }
    } else {
        ensure_worker_initialized()?;
        Ok(DispatchMode::DedicatedWorker)
    }
}

/// Allocate a unique process-wide engine handle ID.
pub(crate) fn allocate_engine_id() -> u64 {
    NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Run a closure on the GTK main thread, either directly if already on it or
/// synchronously dispatched via `glib::MainContext::default()`.
pub(crate) fn run_on_gtk_thread<R, F>(mode: DispatchMode, f: F) -> EngineResult<R>
where
    R: Send + 'static,
    F: FnOnce() -> EngineResult<R> + Send + 'static,
{
    if gtk::is_initialized_main_thread() {
        return f();
    }

    let can_dispatch_to_loop = match mode {
        DispatchMode::DedicatedWorker => WORKER_STARTED.load(Ordering::Acquire),
        DispatchMode::CurrentMainThread | DispatchMode::DefaultMainContext => gtk::main_level() > 0,
    };

    if !can_dispatch_to_loop {
        return Err(EngineError::Backend(
            "WebKitGtkEngine called off GTK main thread while no GTK main loop is running".into(),
        ));
    }

    let (tx, rx) = mpsc::sync_channel::<EngineResult<R>>(1);
    glib::MainContext::default().invoke(move || {
        let res = f();
        let _ = tx.send(res);
    });
    rx.recv().map_err(|_| {
        EngineError::Backend("GTK main thread dropped response before completing call".into())
    })?
}

/// Insert a new `InnerWebKitEngine` into the GTK main thread's registry.
pub(crate) fn register_engine(id: u64, engine: InnerWebKitEngine) {
    LOCAL_ENGINES.with(|map| {
        map.borrow_mut().insert(id, Rc::new(RefCell::new(engine)));
    });
}

/// Execute `f` with the per-engine `Rc<RefCell<InnerWebKitEngine>>` on the GTK
/// main thread without keeping `LOCAL_ENGINES` borrowed.
///
/// Async-waiting operations (`snapshot`, `find`, `clear_data`, `apply_content_rules`)
/// clone the needed GLib handles, drop their engine borrow, and only then wait
/// on `glib::MainContext` so parallel engine instances and re-entrant callbacks
/// cannot hit a `RefCell` borrow panic.
pub(crate) fn with_local_engine_cell<R>(
    id: u64,
    f: impl FnOnce(&Rc<RefCell<InnerWebKitEngine>>) -> EngineResult<R>,
) -> EngineResult<R> {
    let engine = LOCAL_ENGINES
        .with(|map| map.borrow().get(&id).cloned())
        .ok_or_else(|| {
            EngineError::Backend(format!("WebKitGtkEngine instance {id} is no longer active"))
        })?;
    f(&engine)
}

/// Execute `f` with mutable access to the `InnerWebKitEngine` on the GTK main thread.
pub(crate) fn with_local_engine<R>(
    id: u64,
    f: impl FnOnce(&mut InnerWebKitEngine) -> EngineResult<R>,
) -> EngineResult<R> {
    with_local_engine_cell(id, |engine_rc| {
        let mut borrow = engine_rc.try_borrow_mut().map_err(|_| {
            EngineError::Backend(format!(
                "re-entrant mutable access to WebKitGtkEngine instance {id}"
            ))
        })?;
        f(&mut borrow)
    })
}

/// Execute `f` with immutable access to the `InnerWebKitEngine` on the GTK main thread.
pub(crate) fn with_local_engine_ref<R>(
    id: u64,
    f: impl FnOnce(&InnerWebKitEngine) -> R,
) -> Option<R> {
    if !gtk::is_initialized_main_thread() {
        return None;
    }
    let engine = LOCAL_ENGINES.with(|map| map.borrow().get(&id).cloned())?;
    engine.try_borrow().ok().map(|guard| f(&guard))
}

/// Remove an engine instance from the GTK main thread's registry cleanly.
pub(crate) fn unregister_engine(id: u64, mode: DispatchMode) {
    let cleanup = move || {
        let removed = LOCAL_ENGINES.with(|map| map.borrow_mut().remove(&id));
        if let Some(engine_rc) = removed
            && let Ok(mut engine) = engine_rc.try_borrow_mut()
        {
            engine.shutdown();
        }
    };

    if gtk::is_initialized_main_thread() {
        cleanup();
    } else if matches!(mode, DispatchMode::DedicatedWorker)
        || (gtk::is_initialized() && gtk::main_level() > 0)
    {
        glib::MainContext::default().invoke(cleanup);
    }
}

/// Pump all currently pending events on `glib::MainContext::default()` without
/// blocking. Must only be called on the GTK main thread outside any `LOCAL_ENGINES`
/// borrow.
pub(crate) fn pump_pending_glib_events() {
    if !gtk::is_initialized_main_thread() {
        return;
    }
    let ctx = glib::MainContext::default();
    while ctx.pending() {
        ctx.iteration(false);
    }
}

/// Run GLib main-context iterations on the GTK main thread until `is_done()`
/// returns `true` or `timeout` elapses. Returns the final `is_done()` state.
pub(crate) fn wait_until(timeout: Duration, mut is_done: impl FnMut() -> bool) -> bool {
    if is_done() {
        return true;
    }
    if !gtk::is_initialized_main_thread() {
        return false;
    }

    let ctx = glib::MainContext::default();
    let timed_out = Arc::new(AtomicBool::new(false));
    let flag = timed_out.clone();
    let source_id = glib::timeout_add_once(timeout, move || {
        flag.store(true, Ordering::Release);
    });

    while !is_done() && !timed_out.load(Ordering::Acquire) {
        ctx.iteration(true);
    }

    if let Some(source) = ctx.find_source_by_id(&source_id) {
        source.destroy();
    }

    is_done()
}
