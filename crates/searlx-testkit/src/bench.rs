//! Automated performance budget harness for CI (`P2-21`, `C-02`).
//!
//! Measures cold session restore (30 tabs), new-tab creation, 30-tab switching
//! cycle, 1,000-match find, and address parsing against explicit budgets so a
//! regression fails CI.

use std::time::{Duration, Instant};

use searlx_core::data::{DataStore, SessionFile, SessionTab};
use searlx_core::{
    CURRENT_SCHEMA_VERSION, FindSession, PageUrl, ParseOptions, RestoreMode, TabIdAllocator,
    TabModel, resolve_address, restore_session,
};

/// Performance budget ceilings (`P2-21`).
#[derive(Clone, Copy, Debug)]
pub struct PerformanceBudgets {
    /// Maximum duration to restore a 30-tab session from disk (only active tab wakes).
    pub restore_30_tabs_max: Duration,
    /// Maximum duration to create a new tab.
    pub new_tab_max: Duration,
    /// Maximum average duration per tab switch across a 30-tab model.
    pub tab_switch_avg_max: Duration,
    /// Maximum duration for a 1,000-match find pass.
    pub find_1000_max: Duration,
    /// Maximum duration to parse 100 mixed addresses and search queries.
    pub parse_100_addresses_max: Duration,
}

impl Default for PerformanceBudgets {
    fn default() -> Self {
        Self {
            restore_30_tabs_max: Duration::from_millis(45),
            new_tab_max: Duration::from_millis(5),
            tab_switch_avg_max: Duration::from_millis(2),
            find_1000_max: Duration::from_millis(25),
            parse_100_addresses_max: Duration::from_millis(25),
        }
    }
}

/// Measured timings from a performance budget run.
#[derive(Clone, Copy, Debug)]
pub struct PerformanceMeasurements {
    /// Time to restore a 30-tab session.
    pub restore_30_tabs: Duration,
    /// Time to create a new tab.
    pub new_tab: Duration,
    /// Average time per tab switch across 30 tabs.
    pub tab_switch_avg: Duration,
    /// Time to count 1,000 matches on a large page.
    pub find_1000: Duration,
    /// Time to parse 100 addresses.
    pub parse_100_addresses: Duration,
}

/// Run the core performance budget suite and return an error if any metric
/// exceeds `budgets` (`P2-21`).
pub fn verify_performance_budgets(
    temp_dir: &std::path::Path,
    budgets: &PerformanceBudgets,
) -> Result<PerformanceMeasurements, String> {
    let store = DataStore::new(temp_dir);
    let mut tabs = Vec::with_capacity(30);
    for i in 0..30_usize {
        tabs.push(SessionTab {
            id: format!("tab-{i}"),
            url: format!("https://example.com/item-{i}"),
            title: format!("Item {i}"),
            ..SessionTab::default()
        });
    }
    store
        .save_session(
            None,
            &SessionFile {
                version: CURRENT_SCHEMA_VERSION,
                active: 0,
                tabs,
                ..SessionFile::default()
            },
        )
        .map_err(|e| e.to_string())?;

    // 1. Cold 30-tab session restore
    let mut alloc = TabIdAllocator::new();
    let t0 = Instant::now();
    let restored =
        restore_session(&store, RestoreMode::Normal, &mut alloc).map_err(|e| e.to_string())?;
    let restore_30_tabs = t0.elapsed();
    if restored.windows[0].tabs.len() != 30 {
        return Err("expected 30 restored tabs".into());
    }

    // 2. New tab + 30-tab switch cycle
    let mut model = TabModel::new(&mut alloc).map_err(|e| e.to_string())?;
    let t_new = Instant::now();
    let url = PageUrl::parse("https://example.com/first").map_err(|e| e.to_string())?;
    model
        .create_tab(&mut alloc, Some(url), "First", false, true)
        .map_err(|e| e.to_string())?;
    let new_tab = t_new.elapsed();

    for i in 2..30_usize {
        let u =
            PageUrl::parse(&format!("https://example.com/tab-{i}")).map_err(|e| e.to_string())?;
        model
            .create_tab(&mut alloc, Some(u), format!("Tab {i}"), false, false)
            .map_err(|e| e.to_string())?;
    }

    let t_switch = Instant::now();
    for idx in 0..30_usize {
        model.select_index(idx);
    }
    let tab_switch_avg = t_switch.elapsed() / 30;

    // 3. 1,000-match find
    let haystack = "searlx performance probe ".repeat(1_500);
    let mut find = FindSession::new(false);
    let t_find = Instant::now();
    find.search_in_text("searlx", &haystack);
    let find_1000 = t_find.elapsed();

    // 4. 100 address resolutions
    let opts = ParseOptions::default();
    let t_parse = Instant::now();
    for i in 0..100_usize {
        let sample = match i % 4 {
            0 => "example.com/docs",
            1 => "localhost:3000/api",
            2 => "rust browser architecture",
            _ => "[::1]:8080/health",
        };
        let _ = resolve_address(sample, &opts);
    }
    let parse_100_addresses = t_parse.elapsed();

    let measurements = PerformanceMeasurements {
        restore_30_tabs,
        new_tab,
        tab_switch_avg,
        find_1000,
        parse_100_addresses,
    };

    if restore_30_tabs > budgets.restore_30_tabs_max {
        return Err(format!(
            "30-tab restore took {restore_30_tabs:?}, above {:?}",
            budgets.restore_30_tabs_max
        ));
    }
    if new_tab > budgets.new_tab_max {
        return Err(format!(
            "new_tab took {new_tab:?}, above {:?}",
            budgets.new_tab_max
        ));
    }
    if tab_switch_avg > budgets.tab_switch_avg_max {
        return Err(format!(
            "tab_switch_avg took {tab_switch_avg:?}, above {:?}",
            budgets.tab_switch_avg_max
        ));
    }
    if find_1000 > budgets.find_1000_max {
        return Err(format!(
            "find_1000 took {find_1000:?}, above {:?}",
            budgets.find_1000_max
        ));
    }
    if parse_100_addresses > budgets.parse_100_addresses_max {
        return Err(format!(
            "parse_100_addresses took {parse_100_addresses:?}, above {:?}",
            budgets.parse_100_addresses_max
        ));
    }

    Ok(measurements)
}
