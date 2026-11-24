//! gpui chrome: window, tabs, sidebar, split, panels.
//!
//! The UI talks to the app through commands and events. It never touches a
//! concrete web engine.

pub mod a11y;
pub mod app;
pub mod field;
pub mod metrics;
pub mod theme;

pub use a11y::{
    A11yNode, A11yRole, AccessibilityTree, FocusController, FocusTarget, build_chrome_a11y_tree,
};
pub use app::{AddressOverlayMode, Shell, ShellModel, run};
pub use field::{AddressField, FieldEvent};

/// A UI startup failure.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    /// The window could not be created.
    #[error("could not open the window: {0}")]
    Window(String),
    /// The browser controller could not be initialized or failed to dispatch.
    #[error("browser controller error: {0}")]
    Controller(String),
}
