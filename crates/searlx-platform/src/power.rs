//! Power assertions and frame-rate hints.

use crate::PlatformResult;
use std::time::Duration;

/// A power assertion that keeps the machine awake until dropped.
pub trait PowerAssertion: Send + Sync {
    /// Why the machine is being kept awake.
    fn reason(&self) -> &str;
}

/// Power management.
pub trait PowerManager: Send + Sync {
    /// Keep the system awake for a page that asked for it.
    fn keep_awake(&self, reason: &str) -> PlatformResult<Box<dyn PowerAssertion>>;

    /// Tell the compositor the app is animating.
    fn set_frame_rate_hint(&self, _rate: Option<u32>) -> PlatformResult<()> {
        Ok(())
    }

    /// Whether the system is in a low-power state.
    fn low_power(&self) -> bool {
        false
    }

    /// How long the user has been idle.
    fn idle_time(&self) -> Option<Duration> {
        None
    }
}

/// A power manager for headless runs.
#[derive(Debug, Default)]
pub struct NullPowerManager;

impl PowerManager for NullPowerManager {
    fn keep_awake(&self, reason: &str) -> PlatformResult<Box<dyn PowerAssertion>> {
        Ok(Box::new(NullAssertion {
            reason: reason.to_owned(),
        }))
    }
}

#[derive(Debug)]
struct NullAssertion {
    reason: String,
}

impl PowerAssertion for NullAssertion {
    fn reason(&self) -> &str {
        &self.reason
    }
}
