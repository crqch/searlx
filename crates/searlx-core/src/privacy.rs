//! Store compositor for private-tab isolation and per-origin permissions (`FEATURES.md` §12.2, §12.3, `AGENTS.md` §9, P2-14).
//!
//! Security rule (`AGENTS.md` §9):
//! > **Private tabs** use a non-persistent store and write no history, session,
//! > favicon, password offer, Handoff, iCloud, or extension state. Enforce this
//! > in the store compositor, not in each feature.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::domain::{PermissionDecision, PermissionKey, PermissionKind};
use crate::error::Result;
use crate::history::HistoryManager;
use crate::ids::TabId;
use crate::url::PageUrl;

/// Per-origin permission store (`capture.<origin>|<kind>`, `FEATURES.md` §12.3, P2-14).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionStore {
    /// Persisted decisions keyed by `capture.<origin>|<kind>`.
    decisions: BTreeMap<String, PermissionDecision>,
    /// Global `notifications.ask` setting from Settings › Privacy.
    pub notifications_globally_enabled: bool,
}

impl PermissionStore {
    /// Create a permission store with global notification prompts enabled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            decisions: BTreeMap::new(),
            notifications_globally_enabled: true,
        }
    }

    /// Evaluate a page's permission request (`FEATURES.md` §5.9, §12.3):
    /// - Private tabs are **never asked** and always receive [`PermissionDecision::Refuse`].
    /// - Agent tabs are **never asked** and always receive [`PermissionDecision::Refuse`] (`AGENTS.md` §9).
    /// - When `notifications.ask` is off globally, notification requests are refused.
    /// - Otherwise returns the stored per-origin decision or [`PermissionDecision::Ask`].
    #[must_use]
    pub fn evaluate_request(
        &self,
        url: &PageUrl,
        kind: PermissionKind,
        private_tab: bool,
        agent_tab: bool,
    ) -> PermissionDecision {
        if private_tab || agent_tab {
            return PermissionDecision::Refuse;
        }
        if kind == PermissionKind::Notifications && !self.notifications_globally_enabled {
            return PermissionDecision::Refuse;
        }
        let Ok(key) = PermissionKey::for_url(url, kind) else {
            return PermissionDecision::Refuse;
        };
        self.decisions
            .get(&key.to_storage_key())
            .copied()
            .unwrap_or(PermissionDecision::Ask)
    }

    /// Save a user permission decision for `url` and `kind`.
    ///
    /// - Private tabs and [`PermissionDecision::AllowOnce`] are never persisted (`FEATURES.md` §5.8, §12.3).
    pub fn set_decision(
        &mut self,
        url: &PageUrl,
        kind: PermissionKind,
        decision: PermissionDecision,
        private_tab: bool,
    ) -> Result<bool> {
        if private_tab || decision == PermissionDecision::AllowOnce {
            return Ok(false);
        }
        let key = PermissionKey::for_url(url, kind)?;
        if decision == PermissionDecision::Ask {
            self.decisions.remove(&key.to_storage_key());
        } else {
            self.decisions.insert(key.to_storage_key(), decision);
        }
        Ok(true)
    }

    /// Insert a raw `capture.<origin>|<kind>` entry (for loading or conflict testing).
    pub fn insert_raw_storage_key(
        &mut self,
        storage_key: &str,
        decision: PermissionDecision,
    ) -> Result<()> {
        let _validated = PermissionKey::from_storage_key(storage_key)?;
        self.decisions.insert(storage_key.to_owned(), decision);
        Ok(())
    }

    /// Check whether `url` has two disagreeing port spellings saved for any
    /// permission (e.g. `https://example.com` vs `https://example.com:443`, `FEATURES.md` §5.8).
    #[must_use]
    pub fn has_port_spelling_conflict(&self, url: &PageUrl) -> bool {
        let parsed: Vec<(PermissionKey, PermissionDecision)> = self
            .decisions
            .iter()
            .filter_map(|(k, &d)| PermissionKey::from_storage_key(k).ok().map(|pk| (pk, d)))
            .collect();

        for kind in [
            PermissionKind::Camera,
            PermissionKind::Microphone,
            PermissionKind::Location,
            PermissionKind::Notifications,
        ] {
            let Ok(target) = PermissionKey::for_url(url, kind) else {
                continue;
            };
            let canonical = target.canonical_origin();
            let matching: Vec<&(PermissionKey, PermissionDecision)> = parsed
                .iter()
                .filter(|(pk, _)| pk.kind == kind && pk.canonical_origin() == canonical)
                .collect();
            if matching.len() >= 2
                && matching
                    .iter()
                    .any(|(pk, d)| *d != matching[0].1 || pk.origin != matching[0].0.origin)
            {
                return true;
            }
        }
        false
    }

    /// `"Forget choices"` in Settings › Privacy resets every saved origin (`FEATURES.md` §12.3).
    pub fn forget_all_choices(&mut self) -> usize {
        let count = self.decisions.len();
        self.decisions.clear();
        count
    }

    /// Number of persisted origin+kind choices.
    #[must_use]
    pub fn len(&self) -> usize {
        self.decisions.len()
    }

    /// Whether no choices are persisted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.decisions.is_empty()
    }
}

/// Central store compositor enforcing private-tab and agent-tab isolation in one
/// boundary (`AGENTS.md` §9, P2-14).
///
/// All durable side effects (history, session, favicons, password save offers,
/// permissions, Handoff, iCloud tabs, extension tab visibility) route through
/// this compositor. When a tab is registered as `private` (or `agent`), every
/// durable write is blocked at the compositor gate.
#[derive(Clone, Debug, Default)]
pub struct StoreCompositor {
    private_tabs: BTreeSet<TabId>,
    agent_tabs: BTreeSet<TabId>,
    pub history: HistoryManager,
    pub permissions: PermissionStore,
    pub favicons: BTreeMap<String, PageUrl>,
    pub handoff_url: Option<PageUrl>,
    pub icloud_exported_urls: Vec<PageUrl>,
    pub password_offers: Vec<(TabId, String, String)>,
}

impl StoreCompositor {
    /// Create a new store compositor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            private_tabs: BTreeSet::new(),
            agent_tabs: BTreeSet::new(),
            history: HistoryManager::new(),
            permissions: PermissionStore::new(),
            favicons: BTreeMap::new(),
            handoff_url: None,
            icloud_exported_urls: Vec::new(),
            password_offers: Vec::new(),
        }
    }

    /// Register a tab's privacy and agent classification.
    pub fn register_tab(&mut self, tab: TabId, private: bool, agent: bool) {
        if private {
            self.private_tabs.insert(tab);
        } else {
            self.private_tabs.remove(&tab);
        }
        if agent {
            self.agent_tabs.insert(tab);
        } else {
            self.agent_tabs.remove(&tab);
        }
    }

    /// Unregister a closed tab and return `true` if the last private tab just
    /// closed (signalling the engine to destroy the ephemeral data store).
    pub fn unregister_tab(&mut self, tab: TabId) -> bool {
        let was_private = self.private_tabs.remove(&tab);
        self.agent_tabs.remove(&tab);
        self.password_offers.retain(|(t, _, _)| *t != tab);
        was_private && self.private_tabs.is_empty()
    }

    /// Whether `tab` is a private tab.
    #[must_use]
    pub fn is_private(&self, tab: TabId) -> bool {
        self.private_tabs.contains(&tab)
    }

    /// Whether `tab` is a coding-agent tab.
    #[must_use]
    pub fn is_agent(&self, tab: TabId) -> bool {
        self.agent_tabs.contains(&tab)
    }

    /// Whether `tab` is permitted to write any durable state.
    #[must_use]
    pub fn allows_durable_writes(&self, tab: TabId) -> bool {
        !self.is_private(tab) && !self.is_agent(tab)
    }

    /// Record a committed navigation through the compositor gate.
    pub fn on_navigation_committed(
        &mut self,
        tab: TabId,
        url: PageUrl,
        title: &str,
        timestamp_iso: &str,
        space: Option<String>,
    ) {
        if !self.allows_durable_writes(tab) {
            return;
        }
        self.history
            .record_visit(url.clone(), title, timestamp_iso, false, space);
        if matches!(url.scheme(), "http" | "https") {
            self.handoff_url = Some(url.clone());
            if !self
                .icloud_exported_urls
                .iter()
                .any(|u| u.identity() == url.identity())
            {
                self.icloud_exported_urls.push(url);
            }
        }
    }

    /// Record a favicon update through the compositor gate (`AGENTS.md` §9).
    pub fn on_favicon_changed(&mut self, tab: TabId, page_url: &PageUrl, favicon_url: PageUrl) {
        if !self.allows_durable_writes(tab) {
            return;
        }
        self.favicons.insert(page_url.identity(), favicon_url);
    }

    /// Offer to save a password after sign-in through the compositor gate (`AGENTS.md` §9).
    pub fn on_password_save_candidate(
        &mut self,
        tab: TabId,
        origin: impl Into<String>,
        username: impl Into<String>,
    ) -> bool {
        if !self.allows_durable_writes(tab) {
            return false;
        }
        self.password_offers
            .push((tab, origin.into(), username.into()));
        true
    }

    /// Evaluate a permission request through the compositor gate.
    #[must_use]
    pub fn evaluate_permission(
        &self,
        tab: TabId,
        url: &PageUrl,
        kind: PermissionKind,
    ) -> PermissionDecision {
        self.permissions
            .evaluate_request(url, kind, self.is_private(tab), self.is_agent(tab))
    }

    /// Persist a permission choice through the compositor gate.
    pub fn save_permission(
        &mut self,
        tab: TabId,
        url: &PageUrl,
        kind: PermissionKind,
        decision: PermissionDecision,
    ) -> Result<bool> {
        let disallowed = !self.allows_durable_writes(tab);
        self.permissions
            .set_decision(url, kind, decision, disallowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_tab_leaves_zero_trace_across_all_subsystems() {
        let mut comp = StoreCompositor::new();
        let priv_tab = TabId::new(10);
        let norm_tab = TabId::new(11);
        comp.register_tab(priv_tab, true, false);
        comp.register_tab(norm_tab, false, false);

        let secret_url = PageUrl::parse("https://bank.example.com/account").expect("url");
        let icon_url = PageUrl::parse("https://bank.example.com/favicon.ico").expect("icon");

        comp.on_navigation_committed(
            priv_tab,
            secret_url.clone(),
            "Secret Bank",
            "2026-11-21T09:50:00Z",
            None,
        );
        comp.on_favicon_changed(priv_tab, &secret_url, icon_url.clone());
        assert!(!comp.on_password_save_candidate(priv_tab, "https://bank.example.com", "alice"));
        assert_eq!(
            comp.evaluate_permission(priv_tab, &secret_url, PermissionKind::Camera),
            PermissionDecision::Refuse
        );
        assert!(
            !comp
                .save_permission(
                    priv_tab,
                    &secret_url,
                    PermissionKind::Camera,
                    PermissionDecision::Allow
                )
                .expect("save perm")
        );

        let last_private_closed = comp.unregister_tab(priv_tab);
        assert!(last_private_closed);
        assert!(comp.history.visits().is_empty());
        assert!(comp.favicons.is_empty());
        assert!(comp.handoff_url.is_none());
        assert!(comp.icloud_exported_urls.is_empty());
        assert!(comp.password_offers.is_empty());
        assert!(comp.permissions.is_empty());

        // Normal tab persists permissions and forget_all_choices clears them.
        assert!(
            comp.save_permission(
                norm_tab,
                &secret_url,
                PermissionKind::Camera,
                PermissionDecision::Allow
            )
            .expect("normal save perm")
        );
        assert_eq!(
            comp.evaluate_permission(norm_tab, &secret_url, PermissionKind::Camera),
            PermissionDecision::Allow
        );
        assert_eq!(comp.permissions.forget_all_choices(), 1);
        assert_eq!(
            comp.evaluate_permission(norm_tab, &secret_url, PermissionKind::Camera),
            PermissionDecision::Ask
        );
    }
}
