//! Desktop notifications.

use crate::PlatformResult;

/// A notification request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    /// Bold title.
    pub title: String,
    /// Body text.
    pub body: String,
    /// Whether to play a sound.
    pub sound: bool,
}

/// The notification service.
pub trait Notifier: Send + Sync {
    /// Show a notification. Downloads never use this; see FEATURES.md §11.
    fn notify(&self, notification: &Notification) -> PlatformResult<()>;
}

/// A notifier that drops everything, for tests.
#[derive(Debug, Default)]
pub struct NullNotifier;

impl Notifier for NullNotifier {
    fn notify(&self, _notification: &Notification) -> PlatformResult<()> {
        Ok(())
    }
}
