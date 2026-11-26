//! Directive execution, event translation, and shell-managed `search://` / side-panel
//! WebView coordination between `searlx_core::BrowserController` and `WebKitGtkEngine`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use gtk::prelude::*;
use searlx_core::controller::EngineEvent as CtrlEvent;
use searlx_core::{
    BrowserController, EngineCapabilities, EngineDirective, EngineNewWindowDisposition,
    EnginePermissionKind, InjectAt as CoreInjectAt, PageUrl, ProfileId, ScriptWorld as CoreWorld,
    SpaceId, TabId,
};
use searlx_engine::{
    Capabilities, DataKinds, DialogKind, Engine, EngineEvent as RawEngineEvent, FindOptions,
    InjectAt, NewWindowDisposition, PermissionKind as RawPermKind, Rect, ScriptWorld, TabOptions,
};
use searlx_engine_linux::WebKitGtkEngine;
use serde::Deserialize;

use super::internal_pages::{INTERNAL_PAGE_BRIDGE_JS, InternalAction, parse_internal_action};

/// Isolated-world helper script injected into web tabs for hovered link status,
/// reading scroll progress, password-form submit detection, and Dusk light/dark probing.
const PAGE_HELPER_JS: &str = r#"(() => {
    if (window.__searlxPageHelpers) return;
    window.__searlxPageHelpers = true;

    const post = (obj) => {
        try {
            if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.searlx) {
                window.webkit.messageHandlers.searlx.postMessage(JSON.stringify(obj));
            }
        } catch (_e) {}
    };

    let lastHover = '';
    const emitHover = (href) => {
        const next = href || '';
        if (next !== lastHover) {
            lastHover = next;
            post({ kind: 'hover_link', href: next });
        }
    };
    document.addEventListener('mouseover', (ev) => {
        const a = ev.target && ev.target.closest ? ev.target.closest('a[href]') : null;
        emitHover(a ? a.href : '');
    }, true);
    document.addEventListener('mouseout', (ev) => {
        const rel = ev.relatedTarget;
        const a = rel && rel.closest ? rel.closest('a[href]') : null;
        if (!a) emitHover('');
    }, true);

    let lastProgress = -1;
    let scrollTimer = 0;
    const emitScroll = () => {
        scrollTimer = 0;
        const doc = document.documentElement;
        const body = document.body;
        const total = Math.max(doc ? doc.scrollHeight : 0, body ? body.scrollHeight : 0) - window.innerHeight;
        const ratio = total > 0 ? Math.max(0, Math.min(1, window.scrollY / total)) : 0;
        const rounded = Math.round(ratio * 100) / 100;
        if (rounded !== lastProgress) {
            lastProgress = rounded;
            post({ kind: 'scroll_progress', progress: rounded });
        }
    };
    window.addEventListener('scroll', () => {
        if (!scrollTimer) scrollTimer = setTimeout(emitScroll, 120);
    }, { passive: true });

    document.addEventListener('submit', (ev) => {
        const form = ev.target;
        if (!form || !form.querySelector) return;
        const pw = form.querySelector('input[type="password"]');
        if (!pw || !pw.value) return;
        const uf = form.querySelector('input[type="email"], input[type="text"], input[autocomplete*="user"]');
        post({
            kind: 'password_submit',
            username: uf ? (uf.value || '') : '',
            password: pw.value
        });
    }, true);

    const sampleLuminance = () => {
        try {
            const pickBg = (el) => el ? getComputedStyle(el).backgroundColor : '';
            let raw = pickBg(document.body);
            if (!raw || raw === 'transparent' || raw === 'rgba(0, 0, 0, 0)') {
                raw = pickBg(document.documentElement);
            }
            const m = raw && raw.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
            const r = m ? Number(m[1]) : 255;
            const g = m ? Number(m[2]) : 255;
            const b = m ? Number(m[3]) : 255;
            post({ kind: 'dusk_luminance', r, g, b });
        } catch (_e) {}
    };
    if (document.readyState === 'complete' || document.readyState === 'interactive') {
        setTimeout(sampleLuminance, 40);
    } else {
        window.addEventListener('DOMContentLoaded', () => setTimeout(sampleLuminance, 40), { once: true });
    }
})();"#;

/// Isolated-world interactive Hide Elements picker (`Shift+Ctrl+H`, `FEATURES.md` §5.5).
pub const HIDE_PICKER_START_JS: &str = r#"(() => {
    if (window.__searlxHidePicker) return;
    const prevOutline = new WeakMap();
    let hovered = null;

    const synthSelector = (el) => {
        if (!el || el.nodeType !== 1) return '';
        if (el.id && /^[A-Za-z_][A-Za-z0-9_-]*$/.test(el.id)) {
            return '#' + el.id;
        }
        const stableAttrs = ['data-testid', 'data-test', 'data-qa', 'data-cy', 'aria-label', 'name', 'role'];
        const tag = el.tagName.toLowerCase();
        for (const attr of stableAttrs) {
            const v = el.getAttribute(attr);
            if (v && v.trim()) {
                return tag + '[' + attr + '="' + v.replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"]';
            }
        }
        const classes = Array.from(el.classList || []).filter((c) => /^[A-Za-z_][A-Za-z0-9_-]*$/.test(c)).slice(0, 3);
        if (classes.length) {
            return tag + '.' + classes.join('.');
        }
        const parts = [];
        let cur = el;
        while (cur && cur.nodeType === 1 && cur !== document.documentElement) {
            const t = cur.tagName.toLowerCase();
            let nth = 1;
            let sib = cur.previousElementSibling;
            while (sib) {
                if (sib.tagName === cur.tagName) nth++;
                sib = sib.previousElementSibling;
            }
            parts.unshift(t + ':nth-of-type(' + nth + ')');
            cur = cur.parentElement;
        }
        return parts.join(' > ');
    };

    const clearHover = () => {
        if (hovered) {
            hovered.style.outline = prevOutline.get(hovered) || '';
            hovered = null;
        }
    };

    const onMove = (ev) => {
        const target = ev.target;
        if (!target || target === document.documentElement || target === document.body) return;
        if (hovered !== target) {
            clearHover();
            hovered = target;
            if (!prevOutline.has(target)) prevOutline.set(target, target.style.outline || '');
            target.style.outline = '2px solid #3B74F2';
        }
    };

    const onClick = (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        const target = ev.target;
        clearHover();
        const sel = synthSelector(target);
        if (sel && window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.searlx) {
            window.webkit.messageHandlers.searlx.postMessage(JSON.stringify({ kind: 'hide_element_pick', selector: sel }));
        }
    };

    const onKey = (ev) => {
        if (ev.key === 'Escape') {
            ev.preventDefault();
            cleanup();
            if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.searlx) {
                window.webkit.messageHandlers.searlx.postMessage(JSON.stringify({ kind: 'hide_picker_cancelled' }));
            }
        }
    };

    const cleanup = () => {
        clearHover();
        document.removeEventListener('mousemove', onMove, true);
        document.removeEventListener('click', onClick, true);
        document.removeEventListener('keydown', onKey, true);
        delete window.__searlxHidePicker;
    };

    document.addEventListener('mousemove', onMove, true);
    document.addEventListener('click', onClick, true);
    document.addEventListener('keydown', onKey, true);
    window.__searlxHidePicker = cleanup;
})();"#;

pub const HIDE_PICKER_STOP_JS: &str = r#"(() => {
    if (typeof window.__searlxHidePicker === 'function') {
        window.__searlxHidePicker();
    }
})();"#;

/// Outcome of draining and translating a single `WebKitGtkEngine::pump()` batch.
#[derive(Debug, Default)]
pub struct PumpOutcome {
    pub controller_events: Vec<CtrlEvent>,
    pub internal_actions: Vec<(TabId, InternalAction)>,
    pub hovered_link: Option<(TabId, String)>,
    pub scroll_progress: Vec<(TabId, f32)>,
    pub hide_picker_cancelled: bool,
    pub side_panel_crashed: bool,
    pub side_panel_url_changed: Option<PageUrl>,
    pub snapshots: Vec<(TabId, Vec<u8>)>,
}

#[derive(Clone, Debug)]
struct InternalViewState {
    engine_tab_id: TabId,
    uri: String,
    dirty_reload: bool,
}

#[derive(Clone, Debug)]
struct SidePanelViewState {
    engine_tab_id: TabId,
    space_id: SpaceId,
    url: PageUrl,
}

#[derive(Deserialize)]
struct HelperScriptPayload {
    kind: String,
    #[serde(default)]
    href: String,
    #[serde(default)]
    progress: f32,
    #[serde(default)]
    r: u8,
    #[serde(default)]
    g: u8,
    #[serde(default)]
    b: u8,
}

/// Bridges `BrowserController` directives/events to a concrete [`WebKitGtkEngine`].
#[derive(Debug)]
pub struct EngineBridge {
    core_to_engine: BTreeMap<TabId, TabId>,
    engine_to_core: BTreeMap<TabId, TabId>,
    internal_views: BTreeMap<TabId, InternalViewState>,
    internal_engine_to_core: BTreeMap<TabId, TabId>,
    side_panel_view: Option<SidePanelViewState>,
    private_stores: BTreeSet<(SpaceId, ProfileId)>,
    last_rules_json: Option<String>,
    pub cached_snapshots: BTreeMap<TabId, Vec<u8>>,
}

impl EngineBridge {
    #[must_use]
    pub fn new() -> Self {
        Self {
            core_to_engine: BTreeMap::new(),
            engine_to_core: BTreeMap::new(),
            internal_views: BTreeMap::new(),
            internal_engine_to_core: BTreeMap::new(),
            side_panel_view: None,
            private_stores: BTreeSet::new(),
            last_rules_json: None,
            cached_snapshots: BTreeMap::new(),
        }
    }

    /// Convert engine [`Capabilities`] to core [`EngineCapabilities`].
    #[must_use]
    pub fn convert_capabilities(caps: &Capabilities) -> EngineCapabilities {
        EngineCapabilities {
            engine: caps.engine.clone(),
            extensions: caps.extensions,
            passkeys: caps.passkeys,
            content_blockers: caps.content_blockers,
            native_pip: caps.native_pip,
            site_isolation: caps.site_isolation,
            custom_user_agent: caps.custom_user_agent,
            print: caps.print,
            pdf: caps.pdf,
            find_while_pdf: caps.find_while_pdf,
            downloads_pause: caps.downloads_pause,
            cookie_import: caps.cookie_import,
            wayland_overlay: caps.wayland_overlay,
            floating_video: caps.floating_video,
        }
    }

    /// Return the visible `gtk::Widget` for core `tab_id` (either its real web tab or
    /// its shell-managed `search://` internal WebView).
    #[must_use]
    pub fn widget_for_core_tab(
        &self,
        engine: &WebKitGtkEngine,
        core_tab: TabId,
    ) -> Option<gtk::Widget> {
        if let Some(&eid) = self.core_to_engine.get(&core_tab) {
            return engine.webview_widget(eid);
        }
        if let Some(iv) = self.internal_views.get(&core_tab) {
            return engine.webview_widget(iv.engine_tab_id);
        }
        None
    }

    /// Return the `gtk::Widget` for the companion side panel website WebView, if any.
    #[must_use]
    pub fn side_panel_widget(&self, engine: &WebKitGtkEngine) -> Option<gtk::Widget> {
        self.side_panel_view
            .as_ref()
            .and_then(|v| engine.webview_widget(v.engine_tab_id))
    }

    /// Synchronize shell-managed `search://` internal WebViews for visible tabs.
    pub fn ensure_internal_tab_view(
        &mut self,
        engine: &mut WebKitGtkEngine,
        core_tab: TabId,
        space: SpaceId,
        profile: ProfileId,
        private: bool,
        url: &PageUrl,
    ) {
        let target_uri = normalize_internal_uri(url);
        if let Some(existing) = self.internal_views.get_mut(&core_tab) {
            if existing.uri != target_uri || existing.dirty_reload {
                existing.uri = target_uri.clone();
                existing.dirty_reload = false;
                if let Ok(parsed) = PageUrl::parse(&target_uri) {
                    let _ = engine.navigate(existing.engine_tab_id, parsed);
                }
            }
            return;
        }

        // Only allocate an internal WebView if there is no controller webview bound.
        if self.core_to_engine.contains_key(&core_tab) {
            return;
        }

        if let Ok(eid) = engine.create_tab(TabOptions {
            url: None,
            space,
            profile,
            private,
            background: false,
            opener: None,
            user_agent: None,
        }) {
            let _ = engine.inject(
                eid,
                ScriptWorld::Isolated,
                InjectAt::DocumentEnd,
                INTERNAL_PAGE_BRIDGE_JS,
            );
            if let Ok(parsed) = PageUrl::parse(&target_uri) {
                let _ = engine.navigate(eid, parsed);
            }
            self.internal_engine_to_core.insert(eid, core_tab);
            self.internal_views.insert(
                core_tab,
                InternalViewState {
                    engine_tab_id: eid,
                    uri: target_uri,
                    dirty_reload: false,
                },
            );
        }
    }

    /// Mark all live `search://` internal views to refresh on next sync (e.g., after theme or data change).
    pub fn mark_internal_views_dirty(&mut self, engine: &mut WebKitGtkEngine) {
        for iv in self.internal_views.values_mut() {
            iv.dirty_reload = true;
            // Avoid blowing away active typing inside settings search input.
            if iv.uri.starts_with("search://settings") {
                continue;
            }
            let _ = engine.reload(iv.engine_tab_id, false);
            iv.dirty_reload = false;
        }
    }

    /// Reload a specific internal view explicitly when needed (e.g. navigating between settings pages).
    pub fn refresh_internal_tab(&mut self, engine: &mut WebKitGtkEngine, core_tab: TabId) {
        if let Some(iv) = self.internal_views.get_mut(&core_tab) {
            iv.dirty_reload = false;
            let _ = engine.reload(iv.engine_tab_id, false);
        }
    }

    /// Close any shell-managed `search://` internal WebView for `core_tab`.
    pub fn close_internal_view(&mut self, engine: &mut WebKitGtkEngine, core_tab: TabId) {
        if let Some(iv) = self.internal_views.remove(&core_tab) {
            self.internal_engine_to_core.remove(&iv.engine_tab_id);
            detach_widget_before_close(engine, iv.engine_tab_id);
            let _ = engine.close_tab(iv.engine_tab_id);
        }
    }

    /// Ensure the companion side panel website WebView matches `(space_id, url)`.
    pub fn sync_side_panel_website(
        &mut self,
        engine: &mut WebKitGtkEngine,
        wanted: Option<(SpaceId, ProfileId, &PageUrl)>,
    ) {
        let Some((space_id, profile_id, url)) = wanted else {
            if let Some(old) = self.side_panel_view.take() {
                detach_widget_before_close(engine, old.engine_tab_id);
                let _ = engine.close_tab(old.engine_tab_id);
            }
            return;
        };

        if let Some(existing) = &mut self.side_panel_view {
            if existing.space_id == space_id {
                if existing.url.identity() != url.identity() {
                    existing.url = url.clone();
                    let _ = engine.navigate(existing.engine_tab_id, url.clone());
                }
                return;
            }
            // Space changed -> recreate WebView inside the new space's data store (`FEATURES.md` §3.8).
            let old = existing.engine_tab_id;
            detach_widget_before_close(engine, old);
            let _ = engine.close_tab(old);
            self.side_panel_view = None;
        }

        if let Ok(eid) = engine.create_tab(TabOptions {
            url: Some(url.clone()),
            space: space_id,
            profile: profile_id,
            private: false,
            background: false,
            opener: None,
            user_agent: None,
        }) {
            self.side_panel_view = Some(SidePanelViewState {
                engine_tab_id: eid,
                space_id,
                url: url.clone(),
            });
        }
    }

    /// Execute one [`EngineDirective`] against `WebKitGtkEngine`.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn execute_directive(
        &mut self,
        engine: &mut WebKitGtkEngine,
        controller: &mut BrowserController,
        directive: EngineDirective,
        allow_webview_focus: bool,
        synthetic_events: &mut Vec<CtrlEvent>,
        external_dialogs: &mut Vec<searlx_core::ExternalDialogRequest>,
        clipboard_texts: &mut Vec<String>,
    ) {
        match directive {
            EngineDirective::CreateTab {
                tab: core_id,
                options,
            } => {
                self.close_internal_view(engine, core_id);
                let profile = controller.active_profile_id();
                if options.private {
                    self.private_stores.insert((options.space, profile));
                }
                match engine.create_tab(TabOptions {
                    url: options.url,
                    space: options.space,
                    profile,
                    private: options.private,
                    background: false,
                    opener: None,
                    user_agent: None,
                }) {
                    Ok(engine_id) => {
                        self.core_to_engine.insert(core_id, engine_id);
                        self.engine_to_core.insert(engine_id, core_id);
                        controller.bind_engine_tab(core_id, engine_id);
                        let _ = engine.inject(
                            engine_id,
                            ScriptWorld::Isolated,
                            InjectAt::DocumentStart,
                            PAGE_HELPER_JS,
                        );
                        let _ = engine.set_zoom(engine_id, options.zoom_factor);
                        let _ = engine.set_muted(engine_id, options.muted);
                    }
                    Err(err) => {
                        tracing::warn!(target: "search", error = %err, "failed to create WebKitGTK tab");
                    }
                }
            }
            EngineDirective::CloseTab { tab, engine_tab_id } => {
                self.close_internal_view(engine, tab);
                let eid = engine_tab_id.or_else(|| self.core_to_engine.get(&tab).copied());
                self.core_to_engine.remove(&tab);
                self.cached_snapshots.remove(&tab);
                if let Some(id) = eid {
                    self.engine_to_core.remove(&id);
                    detach_widget_before_close(engine, id);
                    let _ = engine.close_tab(id);
                }
            }
            EngineDirective::Navigate { tab, url } => {
                if url.is_internal() {
                    if let Some(&eid) = self.core_to_engine.get(&tab) {
                        self.core_to_engine.remove(&tab);
                        self.engine_to_core.remove(&eid);
                        detach_widget_before_close(engine, eid);
                        let _ = engine.close_tab(eid);
                    }
                } else if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.navigate(eid, url);
                }
            }
            EngineDirective::LoadHtml { tab, html, .. } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let (style_css, inner_html) = extract_reader_style_and_inner_html(&html);
                    let _ = engine.clear_user_stylesheets(eid);
                    if !style_css.is_empty() {
                        let _ = engine.apply_user_stylesheet(eid, &style_css, false, true);
                    }
                    if let Ok(encoded) = serde_json::to_string(&inner_html) {
                        let js = format!(
                            "(() => {{ \
                                document.querySelectorAll('link[rel~=\"stylesheet\"], style').forEach(e => e.remove()); \
                                document.documentElement.innerHTML = {encoded}; \
                            }})();"
                        );
                        let _ = engine.eval(eid, ScriptWorld::Isolated, &js);
                    }
                }
            }
            EngineDirective::Back { tab } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.back(eid);
                }
            }
            EngineDirective::Forward { tab } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.forward(eid);
                }
            }
            EngineDirective::Reload { tab, from_origin } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.reload(eid, from_origin);
                }
            }
            EngineDirective::Stop { tab } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.stop(eid);
                }
            }
            EngineDirective::Eval { tab, world, js } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let final_js = if js.contains("kind:'reader_source'") {
                        augment_reader_eval_js(&js)
                    } else {
                        js
                    };
                    let _ = engine.eval(eid, map_world(world), &final_js);
                }
            }
            EngineDirective::Inject { tab, world, at, js } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.inject(eid, map_world(world), map_inject_at(at), &js);
                }
            }
            EngineDirective::SetBounds { tab, rect } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.set_bounds(
                        eid,
                        Rect {
                            x: rect.x,
                            y: rect.y,
                            width: rect.width,
                            height: rect.height,
                        },
                    );
                }
            }
            EngineDirective::SetVisible { tab, visible } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.set_visible(eid, visible);
                }
            }
            EngineDirective::Focus { tab } => {
                if allow_webview_focus && let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.focus(eid);
                }
            }
            EngineDirective::Snapshot { tab, .. } => {
                if let Some(&eid) = self.core_to_engine.get(&tab)
                    && let Ok(img) = engine.snapshot(eid)
                {
                    self.cached_snapshots.insert(tab, img.png);
                    synthetic_events.push(CtrlEvent::SnapshotReady {
                        tab,
                        width: img.width,
                        height: img.height,
                    });
                }
            }
            EngineDirective::Find {
                tab,
                query,
                options,
            } => {
                if let Some(&eid) = self.core_to_engine.get(&tab)
                    && let Ok(state) = engine.find(
                        eid,
                        &query,
                        FindOptions {
                            case_sensitive: options.case_sensitive,
                            whole_words: options.whole_words,
                            backwards: options.backwards,
                            wrap: true,
                        },
                    )
                {
                    synthetic_events.push(CtrlEvent::FindResults {
                        tab,
                        total_found: state.matches,
                        active_index: state.active,
                    });
                }
            }
            EngineDirective::ClearFind { tab } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.find(eid, "", FindOptions::default());
                }
            }
            EngineDirective::SetZoom { tab, zoom } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.set_zoom(eid, zoom);
                }
            }
            EngineDirective::SetMuted { tab, muted } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.set_muted(eid, muted);
                }
            }
            EngineDirective::ClearData {
                space,
                private,
                kinds,
            } => {
                let target_kinds = DataKinds {
                    cookies: kinds.cookies,
                    cache: kinds.cache,
                    site_data: kinds.storage || kinds.indexed_db,
                    history: true,
                    favicons: true,
                };
                if private {
                    let stores = std::mem::take(&mut self.private_stores);
                    for (sp, prof) in stores {
                        if let Ok(handle) = engine.data_store(sp, prof, true) {
                            let _ = engine.clear_data(handle, target_kinds);
                        }
                    }
                } else if let Some(sp) = space {
                    let prof = controller.active_profile_id();
                    if let Ok(handle) = engine.data_store(sp, prof, false) {
                        let _ = engine.clear_data(handle, target_kinds);
                    }
                }
            }
            EngineDirective::ApplyContentRules {
                rules_json,
                shield_enabled,
                ..
            } => {
                let effective = if shield_enabled {
                    rules_json
                } else {
                    "[]".to_owned()
                };
                if self.last_rules_json.as_deref() != Some(&effective) {
                    let _ = engine.apply_content_rules("searlx-shield", &effective);
                    self.last_rules_json = Some(effective);
                }
            }
            EngineDirective::ApplyUserStylesheet { tab, css, .. } => {
                if let Some(&eid) = self.core_to_engine.get(&tab) {
                    let _ = engine.clear_user_stylesheets(eid);
                    if !css.trim().is_empty() {
                        let _ = engine.apply_user_stylesheet(eid, &css, true, true);
                    }
                }
            }
            EngineDirective::CopyToClipboard { text, .. } => {
                clipboard_texts.push(text);
            }
            EngineDirective::OpenExternalDialog { request } => {
                external_dialogs.push(request);
            }
            EngineDirective::RespondPermission { .. }
            | EngineDirective::RespondScriptDialog { .. }
            | EngineDirective::RespondAuth { .. }
            | EngineDirective::RespondCertificateError { .. } => {}
        }
    }

    /// Trigger OS print on `tab`'s active WebView.
    pub fn print_tab(&self, engine: &mut WebKitGtkEngine, core_tab: TabId) {
        if let Some(&eid) = self.core_to_engine.get(&core_tab) {
            let _ = engine.eval(eid, ScriptWorld::Page, "window.print();");
        } else if let Some(iv) = self.internal_views.get(&core_tab) {
            let _ = engine.eval(iv.engine_tab_id, ScriptWorld::Page, "window.print();");
        }
    }

    /// Toggle the interactive Hide Elements DOM picker on `core_tab`.
    pub fn set_hide_picker(&self, engine: &mut WebKitGtkEngine, core_tab: TabId, active: bool) {
        if let Some(&eid) = self.core_to_engine.get(&core_tab) {
            let js = if active {
                HIDE_PICKER_START_JS
            } else {
                HIDE_PICKER_STOP_JS
            };
            let _ = engine.eval(eid, ScriptWorld::Isolated, js);
        }
    }

    /// Classify raw [`RawEngineEvent`]s emitted by `WebKitGtkEngine::pump()`.
    pub fn classify_raw_events(&mut self, raw_events: Vec<RawEngineEvent>) -> PumpOutcome {
        let mut out = PumpOutcome::default();
        for raw in raw_events {
            // Check side-panel WebView events first.
            if let Some(sp) = &self.side_panel_view {
                let sp_eid = sp.engine_tab_id;
                match &raw {
                    RawEngineEvent::ProcessCrashed { tab, .. } if *tab == sp_eid => {
                        out.side_panel_crashed = true;
                        continue;
                    }
                    RawEngineEvent::UrlChanged { tab, url }
                    | RawEngineEvent::NavigationCommitted { tab, url }
                        if *tab == sp_eid =>
                    {
                        out.side_panel_url_changed = Some(url.clone());
                        continue;
                    }
                    _ if raw_event_tab(&raw) == Some(sp_eid) => continue,
                    _ => {}
                }
            }

            // Check shell-managed internal `search://` WebViews next.
            if let Some(raw_eid) = raw_event_tab(&raw)
                && let Some(&core_tab) = self.internal_engine_to_core.get(&raw_eid)
            {
                match raw {
                    RawEngineEvent::ScriptMessage {
                        world: ScriptWorld::Isolated,
                        body,
                        ..
                    } => {
                        let committed_uri = self
                            .internal_views
                            .get(&core_tab)
                            .map_or("search://newtab", |v| v.uri.as_str());
                        if let Ok(Some(action)) = parse_internal_action(&body, committed_uri) {
                            out.internal_actions.push((core_tab, action));
                        }
                    }
                    RawEngineEvent::NavigationStarted { url, .. } if !url.is_internal() => {
                        out.internal_actions
                            .push((core_tab, InternalAction::Navigate(url)));
                    }
                    _ => {}
                }
                continue;
            }

            // Regular controller-managed web tabs.
            if let RawEngineEvent::ScriptMessage {
                tab: raw_eid,
                world: ScriptWorld::Isolated,
                body,
            } = &raw
                && let Some(&core_tab) = self.engine_to_core.get(raw_eid)
                && let Ok(helper) = serde_json::from_str::<HelperScriptPayload>(body)
            {
                match helper.kind.as_str() {
                    "hover_link" => {
                        out.hovered_link = Some((core_tab, helper.href));
                        continue;
                    }
                    "scroll_progress" => {
                        out.scroll_progress.push((core_tab, helper.progress));
                        continue;
                    }
                    "dusk_luminance" => {
                        out.controller_events
                            .push(CtrlEvent::PageRenderedLuminance {
                                tab: core_tab,
                                r: helper.r,
                                g: helper.g,
                                b: helper.b,
                            });
                        continue;
                    }
                    "hide_picker_cancelled" => {
                        out.hide_picker_cancelled = true;
                        continue;
                    }
                    _ => {}
                }
            }

            if let RawEngineEvent::SnapshotReady {
                tab: raw_eid,
                image,
            } = &raw
                && let Some(&core_tab) = self.engine_to_core.get(raw_eid)
            {
                self.cached_snapshots.insert(core_tab, image.png.clone());
                out.snapshots.push((core_tab, image.png.clone()));
            }

            if let Some(ctrl_ev) = self.translate_engine_event(raw) {
                out.controller_events.push(ctrl_ev);
            }
        }
        out
    }

    fn translate_engine_event(&self, raw: RawEngineEvent) -> Option<CtrlEvent> {
        let map_tab = |eid: TabId| self.engine_to_core.get(&eid).copied();
        Some(match raw {
            RawEngineEvent::NavigationStarted { tab, url } => CtrlEvent::NavigationStarted {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::NavigationCommitted { tab, url } => CtrlEvent::NavigationCommitted {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::NavigationFinished { tab, url } => CtrlEvent::NavigationFinished {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::NavigationFailed { tab, url, reason } => CtrlEvent::NavigationFailed {
                tab: map_tab(tab)?,
                url,
                reason,
            },
            RawEngineEvent::TitleChanged { tab, title } => CtrlEvent::TitleChanged {
                tab: map_tab(tab)?,
                title,
            },
            RawEngineEvent::UrlChanged { tab, url } => CtrlEvent::UrlChanged {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::FaviconChanged { tab, url } => CtrlEvent::FaviconChanged {
                tab: map_tab(tab)?,
                url,
            },
            RawEngineEvent::LoadingChanged { tab, loading } => CtrlEvent::LoadingChanged {
                tab: map_tab(tab)?,
                loading,
            },
            RawEngineEvent::HistoryChanged {
                tab,
                can_go_back,
                can_go_forward,
            } => CtrlEvent::HistoryChanged {
                tab: map_tab(tab)?,
                can_go_back,
                can_go_forward,
            },
            RawEngineEvent::NewWindowRequested {
                tab,
                url,
                disposition,
            } => CtrlEvent::NewWindowRequested {
                tab: map_tab(tab)?,
                url,
                disposition: match disposition {
                    NewWindowDisposition::ForegroundTab => {
                        EngineNewWindowDisposition::ForegroundTab
                    }
                    NewWindowDisposition::BackgroundTab => {
                        EngineNewWindowDisposition::BackgroundTab
                    }
                    NewWindowDisposition::Popup => EngineNewWindowDisposition::Popup,
                    NewWindowDisposition::SmallWindow => EngineNewWindowDisposition::SmallWindow,
                },
            },
            RawEngineEvent::DownloadRequested {
                tab,
                url,
                suggested_name,
            } => CtrlEvent::DownloadRequested {
                tab: map_tab(tab)?,
                url,
                method: searlx_core::HttpMethod::Get,
                has_request_body: false,
                suggested_name,
                total_bytes: None,
            },
            RawEngineEvent::PermissionRequested { tab, origin, kind } => {
                CtrlEvent::PermissionRequested {
                    tab: map_tab(tab)?,
                    origin,
                    kind: match kind {
                        RawPermKind::Camera => EnginePermissionKind::Camera,
                        RawPermKind::Microphone => EnginePermissionKind::Microphone,
                        RawPermKind::Geolocation => EnginePermissionKind::Geolocation,
                        RawPermKind::Notifications => EnginePermissionKind::Notifications,
                        RawPermKind::ScreenCapture => EnginePermissionKind::ScreenCapture,
                        RawPermKind::ClipboardRead => EnginePermissionKind::ClipboardRead,
                    },
                }
            }
            RawEngineEvent::ScriptDialog { tab, kind, message } => CtrlEvent::ScriptDialog {
                tab: map_tab(tab)?,
                kind: match kind {
                    DialogKind::Alert => searlx_core::EngineDialogKind::Alert,
                    DialogKind::Confirm => searlx_core::EngineDialogKind::Confirm,
                    DialogKind::Prompt => searlx_core::EngineDialogKind::Prompt,
                    DialogKind::BeforeUnload => searlx_core::EngineDialogKind::BeforeUnload,
                },
                message,
            },
            RawEngineEvent::FilePickerRequested { tab, multiple } => {
                CtrlEvent::FilePickerRequested {
                    tab: map_tab(tab)?,
                    multiple,
                }
            }
            RawEngineEvent::AuthRequired { tab, origin } => CtrlEvent::AuthRequired {
                tab: map_tab(tab)?,
                origin,
            },
            RawEngineEvent::CertificateError { tab, url, reason } => CtrlEvent::CertificateError {
                tab: map_tab(tab)?,
                url,
                reason,
            },
            RawEngineEvent::MediaStateChanged {
                tab,
                audible,
                muted,
            } => CtrlEvent::MediaStateChanged {
                tab: map_tab(tab)?,
                audible,
                muted,
            },
            RawEngineEvent::FullscreenChanged { tab, fullscreen } => CtrlEvent::FullscreenChanged {
                tab: map_tab(tab)?,
                fullscreen,
            },
            RawEngineEvent::ProcessCrashed { tab, reason } => CtrlEvent::ProcessCrashed {
                tab: map_tab(tab)?,
                reason,
            },
            RawEngineEvent::ScriptMessage { tab, world, body } => CtrlEvent::ScriptMessage {
                tab: map_tab(tab)?,
                world: match world {
                    ScriptWorld::Page => CoreWorld::Page,
                    ScriptWorld::Isolated => CoreWorld::Isolated,
                },
                body,
            },
            RawEngineEvent::SnapshotReady { tab, image } => CtrlEvent::SnapshotReady {
                tab: map_tab(tab)?,
                width: image.width,
                height: image.height,
            },
        })
    }
}

fn detach_widget_before_close(engine: &WebKitGtkEngine, engine_tab_id: TabId) {
    if let Some(widget) = engine.webview_widget(engine_tab_id)
        && let Some(parent) = widget.parent()
        && let Ok(container) = parent.downcast::<gtk::Container>()
    {
        container.remove(&widget);
    }
}

const fn raw_event_tab(ev: &RawEngineEvent) -> Option<TabId> {
    match ev {
        RawEngineEvent::NavigationStarted { tab, .. }
        | RawEngineEvent::NavigationCommitted { tab, .. }
        | RawEngineEvent::NavigationFinished { tab, .. }
        | RawEngineEvent::NavigationFailed { tab, .. }
        | RawEngineEvent::TitleChanged { tab, .. }
        | RawEngineEvent::UrlChanged { tab, .. }
        | RawEngineEvent::FaviconChanged { tab, .. }
        | RawEngineEvent::LoadingChanged { tab, .. }
        | RawEngineEvent::HistoryChanged { tab, .. }
        | RawEngineEvent::NewWindowRequested { tab, .. }
        | RawEngineEvent::DownloadRequested { tab, .. }
        | RawEngineEvent::PermissionRequested { tab, .. }
        | RawEngineEvent::ScriptDialog { tab, .. }
        | RawEngineEvent::FilePickerRequested { tab, .. }
        | RawEngineEvent::AuthRequired { tab, .. }
        | RawEngineEvent::CertificateError { tab, .. }
        | RawEngineEvent::MediaStateChanged { tab, .. }
        | RawEngineEvent::FullscreenChanged { tab, .. }
        | RawEngineEvent::ProcessCrashed { tab, .. }
        | RawEngineEvent::ScriptMessage { tab, .. }
        | RawEngineEvent::SnapshotReady { tab, .. } => Some(*tab),
    }
}

fn normalize_internal_uri(url: &PageUrl) -> String {
    let raw = url.as_str();
    if raw == "search://" || raw == "search:///" {
        "search://newtab".to_owned()
    } else {
        raw.to_owned()
    }
}

fn augment_reader_eval_js(js: &str) -> String {
    format!(
        "(() => {{ const _res = {js}; if (typeof _res === 'string' && window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.searlx) {{ window.webkit.messageHandlers.searlx.postMessage(_res); }} }})();"
    )
}

/// Extract `<style id="searlx-reader-style">` CSS and inner `<html>` body for CSP-safe Reader replacement.
#[must_use]
pub fn extract_reader_style_and_inner_html(html: &str) -> (String, String) {
    let style_css = if let Some(start) = html.find("<style id=\"searlx-reader-style\">") {
        let after = &html[start + "<style id=\"searlx-reader-style\">".len()..];
        after
            .find("</style>")
            .map_or_else(String::new, |end| after[..end].to_owned())
    } else {
        String::new()
    };

    let inner = html
        .find("<html>")
        .map_or(html, |pos| &html[pos + "<html>".len()..])
        .trim_end_matches("</html>")
        .to_owned();
    (style_css, inner)
}

/// Resolve the Linux data root (`$XDG_DATA_HOME/searlx` or `~/.local/share/searlx`)
/// and WebKitGTK data root (`<data_root>/webkitgtk`).
#[must_use]
pub fn resolve_data_roots() -> (PathBuf, PathBuf) {
    let base = if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.trim().is_empty()
    {
        PathBuf::from(xdg).join("searlx")
    } else if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("searlx")
    } else {
        std::env::temp_dir().join("searlx")
    };
    let webkit = base.join("webkitgtk");
    (base, webkit)
}

/// Expand leading `~/` in a user-configured downloads folder path.
#[must_use]
pub fn expand_downloads_dir(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home).join(rest);
    }
    if trimmed == "~"
        && let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home);
    }
    PathBuf::from(trimmed)
}

const fn map_world(world: CoreWorld) -> ScriptWorld {
    match world {
        CoreWorld::Page => ScriptWorld::Page,
        CoreWorld::Isolated => ScriptWorld::Isolated,
    }
}

const fn map_inject_at(at: CoreInjectAt) -> InjectAt {
    match at {
        CoreInjectAt::DocumentStart => InjectAt::DocumentStart,
        CoreInjectAt::DocumentEnd => InjectAt::DocumentEnd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_html_extractor_splits_style_and_inner_html() {
        let doc = "<!doctype html><html><head><title>A</title><style id=\"searlx-reader-style\">body{color:#111;}</style></head><body><article><h1>A</h1></article></body></html>";
        let (css, inner) = extract_reader_style_and_inner_html(doc);
        assert_eq!(css, "body{color:#111;}");
        assert!(inner.starts_with("<head>"));
        assert!(inner.ends_with("</body>"));
    }

    #[test]
    fn normalizes_blank_search_url_and_expands_downloads_dir() {
        let blank = PageUrl::parse("search://").expect("parse");
        assert_eq!(normalize_internal_uri(&blank), "search://newtab");
        let settings = PageUrl::parse("search://settings/appearance").expect("parse");
        assert_eq!(
            normalize_internal_uri(&settings),
            "search://settings/appearance"
        );
        let expanded = expand_downloads_dir("~/Downloads");
        assert!(expanded.ends_with("Downloads"));
    }
}
