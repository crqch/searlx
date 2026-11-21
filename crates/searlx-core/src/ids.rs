//! Stable identifiers.
//!
//! Persistent things (spaces, profiles, pins, groups, collections, windows) use
//! UUIDs so they survive renames and synchronisation. `TabId` is a runtime
//! handle and only needs to be unique within one app run; it is still passed
//! across the engine boundary as an opaque value.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident, $label:literal) => {
        #[doc = concat!("Identifier for a ", $label, ".")]
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Create a fresh identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wrap an existing UUID (for imports and fixtures).
            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// The underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }

            /// Parse a UUID string.
            ///
            /// # Errors
            /// Returns the UUID parse error when the string is not a UUID.
            pub fn parse(value: &str) -> Result<Self, uuid::Error> {
                Uuid::parse_str(value).map(Self)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

uuid_id!(SpaceId, "space");
uuid_id!(ProfileId, "profile");
uuid_id!(PinId, "pin");
uuid_id!(GroupId, "tab group");
uuid_id!(CollectionId, "collection");
uuid_id!(WindowId, "window");

/// A runtime tab handle.
///
/// Tab identifiers are unique for the lifetime of one app process. They are
/// never written to disk; sessions store tab snapshots and assign fresh IDs on
/// restore.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TabId(u64);

impl TabId {
    /// Create a tab handle from a raw value.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Allocate monotonically increasing tab handles.
#[derive(Debug, Default)]
pub struct TabIdAllocator {
    next: u64,
}

impl TabIdAllocator {
    /// Create an allocator starting at one.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 1 }
    }

    /// Allocate the next handle.
    pub fn allocate(&mut self) -> TabId {
        let id = TabId(self.next);
        self.next = self.next.saturating_add(1);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_ids_are_unique_and_monotonic() {
        let mut allocator = TabIdAllocator::new();
        let a = allocator.allocate();
        let b = allocator.allocate();
        assert_ne!(a, b);
        assert!(b > a);
    }

    #[test]
    fn persistent_ids_round_trip() {
        let id = SpaceId::new();
        let json = serde_json::to_string(&id).expect("serialize");
        let back: SpaceId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, back);
    }
}
