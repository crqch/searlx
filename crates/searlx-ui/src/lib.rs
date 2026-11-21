//! gpui chrome: window, tabs, sidebar, split, panels.
//!
//! The UI talks to the app through commands and events. It never touches a
//! concrete web engine.

pub mod app;
pub mod field;
pub mod metrics;
pub mod theme;

pub use app::{Shell, run};
pub use field::{AddressField, FieldEvent};

/// A UI startup failure.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    /// The window could not be created.
    #[error("could not open the window: {0}")]
    Window(String),
}
