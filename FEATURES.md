# Search — complete feature inventory

**Source:** `main` at `9cc5f07`, 20 November 2026.
**Scope:** every user-facing behaviour found in the Swift source — including gestures, hidden affordances, defaults, and macOS-only mechanisms. This is a rewrite reference, not marketing copy.
**Size:** 107,058 lines of Swift across 183 files, plus ~29,795 lines of tests/harnesses. The README's "42,000 lines" is stale.

**Legend**

| Mark | Meaning |
|---|---|
| ● | On by default / core behaviour |
| ○ | Optional, off by default |
| ◐ | Always on; no user switch |
| ⌘ | macOS / Apple-framework only |
| ⚙ | Settings key named; see §24 |
| † | Dormant or held back in this build |
| ⚠ | Depends on a WebKit/engine capability a rewrite must replace |

---

## 1. Product shape and principles

Search is a one-window-per-profile browser with no toolbar, no start page, no suggestion sidebar, no account, and no telemetry. The page is the ground; the chrome is a row of titles and a single field. Tabs sit down either side (the default for new installs) or across the top. A click on the active tab or its address shows a menu-like **site card** instead of a permanent address bar.

Core principles visible in the code:

- **The page is the interface.** Chrome hides until needed: the field rises over the page (`⌘L`), the sidebar folds (`⌘S`), the address bar is optional.
- **One field, addresses only.** Typing words searches; everything else must resolve to a URL. Invalid input shakes rather than silently searching.
- **Nothing leaves the Mac** except the pages requested, their icons, an optional daily update check, optional AI requests, optional iCloud/folder sync, and optional Handoff.
- **One engine per OS.** WebKit is used on macOS by design; there is no bundled Chromium. This is the single largest constraint for a cross-platform rewrite.
- **Everything is lazy.** Tabs restore asleep, web views are built on first use, sleeping tabs release their process, favicons are cached, the block list is compiled once.
- **Settings are mostly opt-in.** New behaviours ship off unless the change is invisible or a fix.

---

## 2. Window and chrome

### 2.1 Window

- One SwiftUI scene window, plus AppKit windows for any additional window or profile. A window belongs to one profile for life. `⌘N` opens another window with its own tabs in the front window's space.
- Hidden title bar; the empty part of the tab row is the window drag region; double-clicking it zooms/fills the screen. Traffic lights are drawn by macOS in the top-left; when the window is inactive they are drawn by hand in a "resting" grey (`Design.swift`).
- Minimum window 640×420; default 1180×780; frame saved per window and restored (`windows.json`; the first window uses the standard `NSWindow Frame search` default).
- Window state per window: frame, current space, tab rows for every space, merged-profile state, folded sidebar. `⇧⌘T` reopens closed windows as well as tabs.
- Full screen: page can fill the screen; the window can stay usable on its own desktop (`fullscreen.keepsWindow`); `Esc` can require two presses within 2 s (`fullscreen.escapeTwice`); traffic lights hidden; card edges optional in full screen (`layout.cardsFullScreen`).
- Glass window (○): the desktop behind the window shows through a blur, with the space's colour/gradient, transparency, and grain; frosted (Big Sur blur) or Liquid Glass (macOS 26); never in full screen or with a folded column. (`Glass.swift`, `window.glass*`.)
- Page as a card (● with sidebar): 8 pt gap, 10 pt radius, `layout.cards`; can be edge-to-edge in full screen.
- Side panel: a website beside the page (Claude, ChatGPT, Gemini, Perplexity, or any page); resizable 300–640 pt, default 380 (`sidepanel.width`), remembered. One panel per window; it sits on the side away from the tab column; `⌘W` does not close it (use the header x or View → Side Panel). (`SidePanel.swift`.)
- Bookmarks bar (○): a row above the page (`bookmarks.bar`), folders open in menus.
- Address line (○): always-visible address at the top of the column or before the top tabs; Small 220 pt or Medium 340 pt; click to edit in place, or use the site-card button. (`AddressLine.swift`.)
- Status line (●): hovered link's destination at the bottom of the page; off is possible (`links.show`).
- Downloads button: only while downloading unless `downloads.button` is on.
- Sidebar doors: spaces, extensions, bookmarks and downloads at the foot; can move up beside back/forward/reload (`sidebar.doorsUp`).
- "What's playing" player: a tab making sound in the background shows above the sidebar foot, with pause/resume and go-to (`sidebar.player`).
- Agent row: while a coding agent is connected, a Stop row on the column/strip foot.
- Quit confirmation (○): `⌘Q` asks first (`quit.confirm`). Imports are allowed 5 s to clean up before termination.
- Crash log: one line per uncaught exception in `crash.log`; never sent anywhere.

### 2.2 Chrome layouts

| Mode | Description | Key |
|---|---|---|
| Top tabs | Titles across the window, pill slides between them; short bar option | default for old installs; `sidebar=false` |
| Left column | Titles down the left; resizable 176–440 pt, default 232 pt | default for new installs; `sidebar=true`, `sidebar.position=left` |
| Right column | Same, on the right | `sidebar.position=right` |
| Icons only | Narrow fixed column of favicons/letters, title in tooltip; nothing grows on hover | `sidebar.icons` |
| Folded | Page takes the whole window; column slides out over the page when the pointer reaches the edge; traffic lights return with it | `⌘S`, `folded` per window |
| Auto-hidden | Column folded permanently, comes out on edge dwell | `sidebar.hides` |
| Fold-to-icons | `⌘S` toggles icons instead of hiding | `sidebar.foldIcons` |
| Roomier | Taller rows and 14 pt titles | `sidebar.roomy` |
| Doors up | Footer buttons in the top row when they fit | `sidebar.doorsUp` |

Folded-sidebar edge behaviour: 6 pt edge zone; up to 48 pt past the window edge counts; 65 ms grace before it retracts; 150 ms dwell before a permanently folded column appears; in full screen the edge starts 60 pt below the top. Pointer drift while renaming a tab keeps the column out.

---

## 3. Tabs

### 3.1 Row anatomy

- Pinned **squares** (default) in a grid above the tabs; optionally **pinned rows** with titles, grouped into folders (see §7).
- Named **groups** with a heading, fold mark, optional colour, member tabs; groups can be folded.
- **Loose tabs** in the rest of the row.
- A **split item** represents 2–4 pages side by side as one place in the row; it shows the pages' icons/favicons stacked and the focused page's title.
- The **live pill**: a grey (or Liquid Glass) shape that slides from the tab you left to the one you picked; it does not blink.
- **Reading fill**: a grey progress fill grows along the live tab as you scroll the page (`tabs.reading`).
- **Tab glyphs**: a letter (default) or the site's favicon (`glyph`). Favicons are fetched from the page's declared icons, cached as small PNGs with light/dark variants and page-level identity.
- **Shortcut numbers**: while `⌘` is held, each tab shows the digit that selects it (`tabs.shortcuts`).
- **Sleeping tabs**: title/icon can dim (`tabs.sleep.dim`); a sleeping tab shows its last picture while it reloads.
- **Audio**: a speaker icon on tabs making sound; click to mute/unmute; muted state persists in the session and in the tab.
- **Other badges**: recording indicator (extension screen capture), translation sign, agent dot, blocked-popup sign, close cross on hover, pin "minus" (put down) instead of "x".
- The **plus** appears only when the pointer is in the row (or always while `⌘T` field is up); a click makes a new tab.
- Space dot and profile badge appear when those features are on; the space name fades in over the row briefly after a switch.
- **Overflow**: the top row scrolls horizontally; the sidebar scrolls vertically. The live tab is scrolled into view when selected; a space switch scrolls to that space's last tab.

### 3.2 Tab semantics

- **One tab per page.** Switching tabs moves the existing `WKWebView` in and out of the window; the page does not reload, lose scroll, or forget typed input.
- **New tab** (`⌘T`): reuses an existing blank tab if one exists (but never one in a group), otherwise inserts at the configured place. If an extension overrides the new-tab page, Search asks once and can use it.
- **New private tab** (`⇧⌘N`): a separate ephemeral `WKWebsiteDataStore`; never reuses a normal blank tab; never restores; nothing is written to history, favicons, cookies, or session; never offered notifications; some features are disabled (passkeys, Apple Passwords, extension private access unless granted).
- **New tab position**: Next (default), Top, End (`tabs.newPlace`). Links opened in front from a page can go immediately after their opener (`tabs.frontBeside`).
- **Lazy background tabs** (`tabs.lazy`): a link opened behind the page waits to load until you go to it; `⇧⌘-click` still goes there at once.
- **Close** (`⌘W`):
  - A pinned tab is **put down**, not closed: its place/letter stay, the page is released, and it wakes at its pinned page if `pinsGoHome`/`pinsPeek`.
  - A tab in a group is **put away**: its row stays in the group, the page is released, and a click reopens it; closing a put-away row deletes it.
  - Closing the last non-pin with pins beside it leaves a new tab if `tabs.newAfterLast`.
  - Closing the last tab in a window with no others makes a fresh blank tab or closes the window if it is blank.
  - Close goes back to the previously viewed tab if `tabs.closeGoesBack`; otherwise the neighbour.
  - `⌘Z` reopens the just-closed tab if `tabs.undoReopens` and closing it was the last action.
- **Reopen** (`⇧⌘T`): tabs, groups, and windows; `⌘⇧T` restores a tab's Back/Forward and scroll position. Up to a dozen recent closes in the History menu; archived closures for 30 days/500 entries.
- **Duplicate** (`⌘D`): opens the active address in a new tab beside it.
- **Rename**: inline in the row; a named tab is "kept" (never auto-closes).
- **Pin** (`⌘` number or menu): a letter/site icon, fixed first place, wakes at home; can be a square or a row; can live in every space; can be in folders.
- **Sleep now**: right-click → Put to Sleep; disabled when the tab is on screen, playing, on a call, downloading, asking a question, or holding typed text.
- **Move**: to another space (`Move to Space`), another window (`Move to Window`/`New Window`), a new small window (`Open in Small Window`), the side panel, or Split View.
- **Multi-select**: `⌘`-click toggles, `⇧`-click selects the visible run from the anchor. Batch actions: close, move, open in Split View (up to 4), copy addresses, pin, drag.
- **Middle click** on a tab closes it; middle click on a page link opens it in a background tab (or foreground with `⇧`). Mouse side buttons over the tab row switch spaces.

### 3.3 Dragging

- **Reorder** within the row; groups make room; the carried tab lifts under the pointer.
- **Drag out of the window** (12 pt slack) drops into another window of the same profile or makes a new window at the drop point. Pins can't leave. Dragging a tab from the row shows a small floating **TabLift** window (180×30, shadow, no key).
- **Drag onto a page's edge** splits it: a band of `max(120 pt, 24% of the page)` on either side; the split is previewed after 6 pt of travel and 90 ms of stillness; leaving the band by 20 pt cancels; 300 ms parting animation. Dragging onto a split adds a page (up to 4) or puts the carried tab into it.
- **Drag across the pin/tab boundary**: a tab dropped on the pin grid becomes a pin; a pin dropped among tabs becomes a tab; pin rows can be dropped into groups and folders. A 5 pt margin counts as the grid; 10 pt beside the column still counts.
- **Peek**: a modified click on a link (Shift by default; Option-Shift or Option-Command can be chosen) opens the page in a panel over the current one. Escape, a click beside it, or its cross closes it; `⌘↩` keeps it as a tab; `⌥⌘↩` keeps it beside the page in Split View. With `links.peek.stays`, it hides with its tab and comes back; hidden peeks pause video/audio and mute camera/mic.
- **Pin drag**: dragging a pin over another pin reorders; a pin dragged onto the tabs becomes a loose tab; a tab dragged onto pins becomes a pin at the nearest grid slot.

### 3.4 Tab switcher (`⌃Tab`)

- A grid of page previews, most recently used first, with the current tab first; **5 columns, up to 10 cards**. The panel appears after a **0.15 s** hold, so a quick press can go back and return without drawing it.
- Split pairs are one card with both previews; **moons** beside the grid show small windows (up to 9, even columns, at most 3 per column). Moon cards scale with page length on a log₂ curve; 1 screen is smallest, 16+ screens full column.
- Held `⌃` keeps the switcher up; `⇧` walks backwards; arrow keys move in the ring (left/right wrap; up/down stay in columns, ±5). Hover only picks after the pointer has moved **>2 pt** since the panel appeared, so a resting cursor doesn't steal the keyboard pick.
- While held, `⌃W` closes the picked tab, `⌃R` reloads, `⌃M` mutes, `⌃O` keeps a small window into the row.
- Release selects the highlighted tab; a fresh press of the key while up puts the switcher away. Any other key cancels; Esc cancels.
- Preview images are captured **120 ms** after selection settles, staggered **40 ms** apart; the tab just left is captured after **0.4 s** so switching isn't stalled. Cached per tab and discarded when the tab closes or sleeps without a picture.
- With `tabs.switcherBehindFirst`, a link just opened behind the page comes immediately after the current page; off, the previously used tab keeps that slot.
- With `tabs.switcherLeavesOutPins`, pinned tabs are excluded from the grid.

### 3.5 Tab lifecycle

- **Sleep** (● `tabs.sleep`): after Normal 30 min, Later 2 h, or Sooner 10 min away (`tabs.sleep.pace`). In Low Power Mode with `motion.light`, at most 10 min. Memory pressure shortens to 5 min (warning) or 0 (critical). Exceptions: on screen, pinned in the current space, playing sound, on a call, recording, downloading, video floating, a question waiting, holding typed text, kept awake per site, blank, still loading. A pin in another space sleeps unless its site may send notifications.
- **Auto-close** (`tabs.close.after`): Never (default), after 12 h, 1 day, 1 week, or 1 month. Counts from when you last left/let go of the tab, including across quits. Exceptions: on screen, pinned, named, grouped, private, blank, busy (playing, call, download, question, typed text), and a split pair closes whole or not at all. Closed tabs go to the archive.
- **Archive**: last 500/30 days; History → Archived Tabs…; searchable; click reopens in the current space.
- **Crash/reload**: a page process ending shows a reload affordance; reload loops are suppressed after repeated crashes. A never-arriving page shows an error with the reason and a retry.
- **Twins** (`tabs.mirror`, ○): every window of a profile shows the same open tabs in each space; each window has its own page instance; closing a window leaves tabs in the others; private tabs stay local.
- **Tab kept state**: a tab is "kept" if pinned, named, grouped, or explicitly kept awake. This protects it from auto-close.

### 3.6 Small interaction details worth preserving

- **Selection timing differs by layout.** A tab in the side column opens as the mouse button goes **down** (so the page is under the hand before a drag begins); a tab in the top strip waits for the release, because the same press may become a drag. Clicking a selected tab in a multi-selection waits for release.
- **Pinned grid**: up to 4 squares per row, evenly divided (5 = 3+2, 7 = 4+3, 9 = 3+3+3, 10 = 4+3+3); a single row keeps at least 3 places; cells become wide-and-short rather than larger than the classic 34 pt square; minimum 20 pt wide.
- **Tab give**: when `tabs.give` is on, a pressed tab scales to 0.97; it returns on release. The press is cancelled if the pointer travels more than 5 pt (so the drag wins).
- **Close cross** appears on hover (and on the active tab), as a small circle; a pinned tab shows a minus (put down), a grouped open tab shows a minus, and a put-away grouped row shows nothing until hovered.
- **Live pill** slides; it never blinks out and in. Liquid Glass adds a pane that stretches/overshoots and bends the titles it passes, but only for titled column/top tabs; pins and split pairs keep grey.
- **Empty strip**: double-click makes a new tab; the plus appears only when the pointer is in the row.
- **Space dot** turns over like a page during a space swipe; the space's name appears over the row briefly after the switch.
- **Page covers**: while a tab wakes from sleep, resizes, or enters full screen, the last picture covers the area until WebKit has drawn the new frame; the cover fades in 0.2 s.
- **Split stage overlays**: each pane has its own cover, "trouble" message, address field, find bar, site card, and permission card, positioned from the stage's frames; a click on a pane focuses it.
- **Peek panel** slides in from the side over the page; the page underneath keeps its scroll; the panel has a cross and a keep button. `⌥` on the keep button keeps it beside the page in Split View.
- **Tab lift window**: a borderless, non-activating, click-through `NSPanel` with the tab's icon/title and a shadow; it follows the pointer while dragging a tab out of the row.
- **Traffic lights** are macOS's; when the window is inactive the app draws them in a resting grey so they don't shout. In full screen they are hidden and the column starts at the top.
- **The field shakes** and says nothing when input isn't a place; it never silently searches a malformed address.

### 3.7 Split View

- **2–4 pages** share one place in the row. The persisted format supports vertical stacking, but the UI only creates horizontal side-by-side panes.
- **Create**: `⌥⌘N` on a single page makes a blank page to its right and focuses its field; on a pair it detaches the split; on 2–4 selected tabs it builds that split. `⌥`-click a tab opens it beside the current page (or removes it from the split). Tab menu → Open in Split View / Add to Split View / Open N Tabs in Split View. Drag a tab to a page edge. Drag a half out of a pair takes only that half.
- **Layout**: one AppKit stage; gutter 7 pt; minimum pane 250 pt. If the window can't fit every pane at 250, only the focused pane is shown but the split is kept and returns when there is room.
- **Shares**: two pages clamp 20–80%; three/four pages minimum 10% each, normalised; adding a page makes all shares even; removing one redistributes its share.
- **Divider**: 4 pt wide, 28→40 pt tall, 0.1 s hover dwell, `resizeLeftRight` cursor; drag follows the pointer and commits on release; snaps to even within 15 pt but needs 20 pt to unsnap; double-click evens adjacent panes; right-click: Swap Pages, Even Out, Separate, Close Both/All.
- **Focus**: click a pane focuses it without stealing the click from WebKit; focused pane owns the keys/find bar; `⌃⌘←`/`⌃⌘→` move between panes; no default key for "focus other".
- **Swap**: reverses pane order, shares, and the row order of the pages.
- **Separate**: removes the whole split, leaving every page as a normal tab. Remove from Split View on a 3–4-page split removes one page.
- **Close**: closing one page leaves the partner; the split disappears when one remains. Close Both/All is one undo batch; `⇧⌘T` restores the same pair.
- **Persistence**: splits are written even when the feature is off, by tab index, with axis, sizes, and focused page; restore validates ≤4 pages and no missing/duplicate pages.
- **Row/ switcher representation**: one row item, with pages stacked in the column, side by side across the top, or stacked marks in icons-only; one card in `⌃Tab` with both previews.
- **Motion**: adding a pane fades in 0.3 s; removing uses page snapshots over 0.32 s; the leaving page shrinks 1% and fades 0.12 s; Reduce Motion dissolves or cuts.

### 3.8 Side panel

- Sits on the side **away from the tab column** (right when tabs are across the top), as a card when the page is a card, otherwise separated by a hairline.
- Three content kinds: an extension page (optionally bound to one tab), a website (signed in as the current space), or Downloads.
- Offered sites: Claude (`claude.ai/new`), ChatGPT (`chatgpt.com`), Gemini (`gemini.google.com/app`), Perplexity (`perplexity.ai`); only http/https. "This Page" opens the current tab's site in the panel.
- Width 380 default, 300–640; edge drag writes on release; double-click resets.
- Extension panel: header with icon/name/close; main-frame navigation limited to the extension's origin; external links open as tabs only on a real click/form in the last second, once per click; no camera/mic; screen capture denied; downloads cancel. Pressing the same extension button again closes it.
- Website panel: site line (host, "not secure" for http), open-in-tab, close; navigation limited to http/https/about; windows become tabs; no camera/mic/screen; crashes reload at most twice a minute then give up. Switching space reloads the panel into the new space's store.
- Private page beside an extension panel the extension isn't allowed to see: the panel is held back and its room goes to the page; it returns when the page is visible again.
- Downloads panel: the same list as the Downloads tab, compact.
- No dedicated keyboard shortcut; View → Side Panel or the extension button; `⌘W` does not close it.

---

## 4. Address field and navigation

### 4.1 Field modes

| Mode | Opened by | Behaviour |
|---|---|---|
| Blank-tab field | New tab | Field is the whole page; typing is the tab's draft; Return goes there |
| Over-page field | `⌘L` | Dims the page, field in the middle, current address selected; Escape/click outside puts it away |
| Floating new tab | `⌘T` (○ `newtab.floating`) | Field over the current page; no tab is made until Return; Escape/`⌘W`/outside returns to the page; draft waits for the next `⌘T` |
| Tab switcher | `⌘K` | Searches open tabs; Enter switches; a typed address still opens a new tab; most recent selected |
| Inline address line | Click the optional address line | Edits in place; suggestions hang under the line; Escape closes |
| Launcher | Global hotkey (○) | Field over any app; Return opens in Search's front window or a small window; Escape returns to the app |
| Site card field | Click the active tab | The site card shows under the address while editing |

### 4.2 Parsing rules (`Address.swift`)

- Whitespace → not an address → search.
- `http://`, `https://`, `file://`, `about:`, `data:`, `search://` are accepted as written; other schemes are refused (no blank tab).
- `/path` and `~/path` open a local file if it exists.
- Anything with `@` is treated as an email address and refused.
- `[::1]:port` → `http://`; `localhost`, `*.localhost`, `*.local`, `127.0.0.1`, `0.0.0.0`, and private IPv4 ranges → `http://`; everything else gets `https://`.
- A host must have a letter TLD (≥2 letters); dotted version numbers are not hosts.
- `0.0.0.0` is rewritten to `localhost` because WebKit won't load it.
- The displayed address strips the scheme only when typing it back would restore exactly the same URL.
- `www.` is hidden in display labels; the registrable domain is emphasised.
- IDN/look-alike names: Unicode that can pass for Latin is shown as punycode (`xn--…`); the registrable domain is never truncated.
- Plain `http` shows "Not secure" on the tab; file pages show their file name.

### 4.3 Suggestions

The list under the field can contain, in roughly this order:

- Open tabs matching the text (up to 6; no duplicates of the current tab).
- History, scored by frecency and prefix/substring fit, with page titles and site trails.
- Bookmarks, including folders ("slips").
- Built-in known sites (Google, Gmail, Drive, Calendar, Maps, YouTube, GitHub, Figma, Notion, Linear, Slack, Discord, X, LinkedIn, Instagram, Reddit, Hacker News, Stack Overflow, Claude, ChatGPT, Dribbble, Behance, Awwwards, Mobbin, SiteInspire, Are.na, Pinterest, Framer, Webflow, Apple Developer, Swift, npm, Supabase, Stripe, Shopify, Cloudflare, Netlify).
- Search-engine suggestions (○ `search.suggest`, never in private tabs, never for addresses/files/emails; 120 ms debounce, 0.8 s patience, up to 6, max 80 characters).
- Past site searches for the site chip.
- Internal pages/commands: Settings, New Tab, New Private Tab, New Space, Bookmarks, History, Downloads, Passwords, Toggle Sidebar; every menu command and every Settings page (`commandbar`, ○).
- Tabs on other devices via iCloud (`icloud.sync`).
- A final "search these words" row.

Keyboard: `↑`/`↓` walk (walking off the top clears the selection); `Return` takes the picked row, a whole-word address command, or the grey completion, then the typed destination; `Tab`/`⇧Tab` walk the suggestion list, and with a site offer `Tab` locks the site chip — **Tab does not accept the completion** (the `acceptEnding()` helper exists but has no call site); `⌘Return` opens in a new tab behind; `⇧⌘Return` opens in a new tab in front; `⌫` cancels the completion and edits normally; `⌥⌫` drops the completion and deletes a word; `⌫` on an empty field removes the site chip. A click on a row takes it without disturbing the keyboard's pick. `⌘←`/`⌘→` go back/forward when the caret is not in an editor.

### 4.4 Site search and keywords

- **Tab-to-search** (◐): typing the start of a known site's name offers "Search Reddit/YouTube/etc."; `Tab` locks the site as a chip; words after it search that site. 16 built-in sites, plus sites learned from OpenSearch descriptions you have visited. `SiteSearch.builtIn` includes Google, Reddit, YouTube (`yt`), X (`twitter`), ChatGPT (`gpt`, `openai`), Claude, Perplexity, Wikipedia (`wiki`, language-aware), GitHub (`gh`), Stack Overflow (`so`), MDN, Amazon (regional), Google Maps (`maps`), IMDb, Spotify, Figma Community.
- **Keywords** (`search.keywords`, ○): `"yt cats"` goes to YouTube search; any keyword with a single `%s` in the path/query/fragment; validated so the words can't change the destination host.
- **Shift-Return** (`search.shift`, ○): searches exactly what is typed with a second engine.
- **Search engines** (`search.engine`): Google (default), DuckDuckGo, Bing, Ecosia, Startpage, Kagi, Brave, Qwant, Custom. Each has a template; custom accepts `{searchTerms}` or `%s`.

### 4.5 Navigation

- Back/forward: buttons, `⌘[`/`⌘]`, two-finger horizontal swipe, mouse side buttons over the page, and **hold** the back/forward button for a menu of every page that way with favicons.
- **Swipe-back hold gesture** (● `swipe.history`): a two-finger horizontal swipe from the page edge pulls in a disc. Past 70 pt it arms; a quick 30 pt flick within 0.25 s also goes. **Keep the fingers down while armed for 0.45 s** and the disc becomes a list of up to **8 pages that way** (back: nearest at the bottom; forward: nearest at the top). Moving up/down 22 pt per step picks a row with a light haptic; releasing opens it. Letting go before arming slips the disc away. A page that can scroll sideways, or has taken the wheel event, keeps the gesture; the page is asked per event and gets the final word, including after the fingers lift.
- **Space swipe**: two fingers sideways over the tabs (column) or up/down over the top bar; 50 pt threshold; 0.4 s rest after a switch; 0.2 s anti-back-stroke; past the last space creates a new one in place; the space's rows follow the fingers and its icon turns over. A mouse wheel notch over the tabs switches one space; the mouse's back/forward buttons over the tabs go one space back/forward.
- **Reload** `⌘R`; **Reload from Origin** `⌥⌘R` (checks every cached resource with the site); **Stop** while loading (`⌘.`).
- Back from the first page in a small window closes it; in a tab it does nothing.
- Internal pages keep their own Back/Forward trail, separate from WebKit's.
- Scroll position and form state are restored by WebKit on back/forward; a sleeping/restored tab keeps scroll, history, and a picture.
- Blocked pop-ups: a page may open a window only after a recent click/key. Refused windows are remembered (last 5) and shown by a sign beside the title, each openable in a tab, plus "Always Allow" for the site's own pages. `tabs.popupWindows` can let sized pop-ups become real small windows.
- New windows from a page: sign-in pop-ups stay attached to their opener; sized pop-ups can be small windows; everything else becomes a tab.

---

## 5. Page features

### 5.1 Find on page (`⌘F`)

- Floating pill in the top corner; "3 of 17" count; previous/next; close.
- Match case and whole words options; whole words unavailable on PDFs (native WebKit find fallback).
- Counts at most 1,000 matches at a time; "more" indicated; results computed in an isolated world so a page can't tamper with `Range`, `RegExp`, or `getSelection`.
- `Return` finds next, `⇧Return` previous; `⌘F` on the Settings tab searches settings instead.
- Narrow panes hide the arrows and use Return/Shift-Return.

### 5.2 Reading mode (`⇧⌘R`)

- Article extraction heuristic: the largest block of prose, penalised for links (navigation/related/comments are link-heavy).
- If the site publishes a Markdown copy (`<link rel="alternate" type="text/markdown">`, an `Accept: text/markdown` response, or a `.md` URL), that is rendered instead (`Reader.twin`, ○ `reader.twins`).
- Clean typography: 38 em column, serif 18/1.72, headings, images/videos/iframes, tables, code, blockquotes, lists, figures, details, wide tables scroll.
- Light by default; `reader.follows` (○) makes it follow the window; a corner switch chooses light/dark/system for the next article.
- The page's own stylesheets are removed for the reader (Tailwind resets otherwise break lists), Search's own injected sheets stay; no page script runs in the reader.
- Leaving reloads the original page.

### 5.3 Translate (`Translate Page` in the page menu, ◐)

- On-device only, using Apple's Translation framework; models downloaded per language; no network.
- Reads text, title, alt, placeholder, aria-label in the page, open shadow roots, and every frame; skips code, fields, editable areas, `translate="no"`, `.notranslate`.
- Sends visible text first in slices; inline links/bold are marked `[1]`, `[2]`; each block is language-detected so quotes/foreign passages keep their language; repeated strings translated once.
- New content follows via MutationObserver until Show Original, navigation, or sleep.
- Target language follows the Mac by default (`translate.target`); pages can be set to always translate (`translate.always`); private tabs can translate but remember nothing; no translation from a private tab's "always" menu.
- The translation sign appears beside the tab title.

### 5.4 Capture (View → Capture…, ○ `pages.capture`)

- A light overlay over the page with three modes: drag an area; click an element (outlined and labelled with its name and W×H); **Whole Page**.
- The overlay is removed before WebKit draws, so it is never in the picture. Area/element uses `takeSnapshot` with the computed rect; Whole Page uses `createPDF` then rasterises the first PDF page, avoiding WebKit's white off-screen area on long pages.
- Limits: 30,000 pt tallest (top cropped beyond), ~64,000,000 pixels (≈256 MB as WebKit draws it), minimum 2×2 pt; oversized shots are scaled down. Pinch zoom is converted from CSS pixels to view points; whole-page shots ignore pinch and use the page's own size.
- Output: PNG + TIFF on the **local** clipboard (`currentHostOnly`) and a PNG in Downloads named `Capture YYYY-MM-DD at HH.MM.SS.png` (deliberately not the site name, so a private page isn't given away).
- Private-tab captures mark the clipboard `org.nspasteboard.TransientType`; nothing is uploaded. Test runs use a temp folder and a private pasteboard.

### 5.5 Hide Elements (`⇧⌘H`)

- A page-side picker: hover outlines the element, click removes it; Escape cancels; the same key/menu toggles pointing off. Only one pointing mode at a time (Capture is stopped if it is up).
- The selector list is stored per site (host without `www`), injected as a stylesheet **before first paint**, so hidden content never flashes. Each selector becomes its own `selector { display:none !important }` rule, so one bad selector can't kill the list.
- Selector strategy: unique `#id`, then stable `data-testid`/`data-test`/`data-qa`/`data-cy`/`aria-label`/`name`/`role`, then stable classes, then an `nth-of-type` path.
- Every hide announces "Hidden — ⌘Z puts it back"; `⌘Z` while pointing restores the last one.
- Review list (`⇧⌘U`) shows what is hidden on the site; hovering a row outlines and scrolls to the element; Restore one, Restore all, Hide something…
- `hidden.json`; a private tab keeps selectors only in memory for that page and writes nothing.

### 5.6 Site style / Boosts (○ `sites.styles`)

- View → Edit Site Style… opens a floating CSS editor for the current site; typing applies live after a 250 ms debounce; Done saves; Clear empties; the editor's size is kept (`sitestyle.width/height`, first 420×220, min 340×120, double-click resets).
- Applied to every page of that site as a `<style id="office-style">` at document start, appended last so equal-specificity rules win without `!important`; the live page gets the same sheet.
- CSS only; no user JavaScript. Stored by host in `styles.json`; turning the switch off removes every style but keeps them all. Not editable from a private tab, but a style written elsewhere is still worn there.

### 5.7 Darken pages (`pages.dusk`, ○)

- For sites with no dark mode, and only while the frame is dark. A 48 px snapshot first checks whether the page is light; dark pages get no script/observer/sheet at all. A site is "wanted" if switched on by hand or measured light this session.
- Each rule that sets a colour gets a dark twin using relative OKLCH colours; hue/chroma kept, lightness moved by role (text/ground/line); shorthands, shadows, masks, legacy `bgcolor`, and open shadow roots handled; images untouched. Until twins are ready, or on older WebKit, a root filter `invert(1) hue-rotate(180deg) contrast(.85)` is used and pictures/video/canvas/frames are reversed; fullscreen disables the filter.
- Budget: 2,000 sheets, 8,000,000 held characters, 48,000,000 characters made; past any, colours give up for good and the page stays on the filter.
- `⇧⌘D` toggles the current site (`dusk.sites`); `⌥⇧⌘D` pauses everywhere until pressed again or quit (not persisted). Private tabs keep the choice only for that page; no measurement is stored.

### 5.8 Site card

Click the active tab (or right-click → Site Information…), or the button at the end of the always-visible address line. It is a non-key, borderless, glass panel that never takes the keyboard; it flips above the field when there is no room below, hides on typing something different, and closes on outside click/app deactivation.

- Header: host without `www` (or `File`/scheme for non-web).
- **Connection row** → secure / mixed content / not secure / untrusted certificate; "Show Certificate…" or "Show Certificate (Not Valid)…"; Back.
- **Copy Address** (shows the current shortcut).
- **Print…**.
- **Zoom**: −, percentage (click resets to Actual Size), +; tooltips carry `⌘-`, `⌘0`, `⌘+`.
- **Block Ads and Trackers** (non-private, http/https, Shield on and not in trouble; changes reload the page).
- **Site Permissions** (http/https only): Camera, Microphone, Location, Notifications; Ask/Allow/Refuse; notifications Allow disabled when site notifications are off; "Permissions aren't saved in a private tab"; "Notifications are off in Settings › Privacy"; "Saved two ways for this site. Pick one to keep." when two port spellings disagree; Back.
- **Play Sound by Itself** (non-private, http/https, only when the global "Videos wait for a click" is off; takes effect from the next page; shows "from the next page").
- **Don't Float Videos Here** (known video site, float-on-leave/away on, non-private).
- **Darken This Site** (Dusk on, not paused, frame dark, page has a host; shows the current `⇧⌘D` key).
- **Keep This Site Awake** (http/https; prevents sleep/auto-close even under memory pressure; still closable; private-tab version is only for that tab).
- Not in the card: mute (tab speaker / ⇧⌘M), site CSS (View menu), reader (⇧⌘R). "Play Sound by Itself" is autoplay, not muting.

### 5.9 Other page features

- **Zoom**: per-site remembered (`zoom.<host>`); pages 40–300%, PDFs 20–600%, steps of 5% in the settings list; `⌘+`, `⌘−`, `⌘0`; "Actual Size" title shows the current percentage; pinch magnification is separate and `⌘0` resets both. A site at the default keeps no entry; private tabs keep nothing.
- **Web Inspector** (`⌥⌘I`), **JavaScript Console** (`⌥⌘J`), **Inspect Element** (`⌥⌘C`); optional Chrome colours and forced-dark inspector (`inspector.chrome`, `inspector.dark`).
- **Print** (`⌘P`), **Save Page As…** (Web Archive), **Export as PDF…**, **Share…** (macOS share sheet).
- **Autoplay**: media waits for a click (`media.click`, ○); per-site "Play Sound by Itself" overrides.
- **Media keys**: a page that plays becomes Now Playing; a paused/forgotten tab gives the keys back to Music/Spotify (`MediaKeys.swift`).
- **Full screen video**: the page gets WebKit's own full-screen window; leaving can require two Esc presses; the window can stay usable on another desktop.
- **Picture-in-picture**: Search's own floating window (default) or macOS PiP (`float.style`); see §22.
- **JavaScript dialogs**: `alert`, `confirm`, `prompt` as sheets over the page (or a card in a split pane); a background tab's questions are held until it comes forward; closing a tab dismisses them safely.
- **File inputs**: a native open panel, named for the site; refused in an agent tab; never shown in a test run.
- **HTTP auth**: basic/digest/NTLM sheet with name/password; retry message; per-space realm.
- **Certificate errors**: this Mac's loopback is trusted; other invalid certificates ask once per site per launch, with "Go Back" default; never for subresources.
- **Downloads** from links, `Content-Disposition`, `window.open`, etc.: see §11.
- **Permissions**: camera, microphone, location, notifications, screen share; a card over the page with Allow/Always allow/Don't allow; choice stored per origin under `capture.*`; a private tab is never asked and is refused; "Forget choices" clears.
- **Notifications**: posted as Mac notifications with the site's icon; click returns to the tab; dismissal/click goes back to the service worker.
- **Screen sharing**: a page's request opens macOS's own picker; screen or window choice; recording indicator on the tab and in the menu bar.
- **Popups**: see §4.5.
- **Context menus**: WebKit's base page/link/selection/media menu, modified as follows. WebKit's generic "Open Link" is removed; "Open Link in New Window" becomes **Open Link in New Tab** (opens behind), with an ⌥ alternate **Open Link in New Tab and Go to It**; **Open Link in Split View** is added when Split View is on. **Copy Clean Link** is added under Copy Link and, when the page URL has trackers, under Reload. A non-linked image gets Search's own menu: Open Image in New Tab, Copy Image, Download Image, Copy Image Address. **Translate Page to <language> / Show Original** is added when available. **Search with <engine>** replaces Web Search for selected text. **Inspect Element** is routed through Search. Extension `contextMenus` items are WebKit's and are not duplicated.
- **Text substitutions**: Edit → Substitutions (Text Replacement, Smart Quotes, Smart Dashes) and Spelling and Grammar → Correct Spelling Automatically, applied inside pages; Text Replacement follows the Mac, quotes/dashes default off.

### 5.10 Injected page scripts and message handlers

All Search scripts except the few noted run in the isolated `WKContentWorld` named "Search", which the page cannot see into (no variables, no `window.webkit`). Handler names are removed from both worlds before registering because a tab opened by a link inherits its opener's controller and double registration crashes.

| Handler | Script | Where / when | Purpose |
|---|---|---|---|
| `officeScroll` | `ScrollRelay` | document end, main frame | scroll progress + "is this sideways scroll the page's?" answer |
| `officeVeil` | `VeilRelay` / picker | document start, main | Hide Elements picking, peek/restore, failure |
| `officeForms` | `FormRelay` | document end, main | sign-in detection, focus rect, submit/name, fill, unsaved, fullscreen, typing |
| `officeImages` | `ImageRelay` | document start, all frames | image menu / linked-image fallback |
| `officeMedia` | `MediaKeys` | document start, all frames | played/playing state, which frame played |
| `officeStore` | `StoreRelay` | document end, main (15.4+) | Chrome Web Store "Add to Search" |
| `officeIcon` | `IconRelay` | document end, main | favicon changed after load |
| `officePasskeys` | `PasskeyRelay` | document start, all frames (page + bridge) | WebAuthn ceremonies |
| `link` | `HoveredLink` | document start, all frames, default client | hovered link address |
| `officeMiddle` | `MiddleRelay` | document start, main | middle-click / ⌘-click / file link |
| `officeDusk` | `DuskRelay` | document start, all frames | measured dark/light, darkening state |
| `officeTranslate` | `TranslateRelay` | document end, all frames | frame present, page text changed |
| `officePopupGesture` | `PopupGestureRelay` | document start, all frames | trusted gesture window for popups |

Other injected scripts: `KeptScroll` (manual scroll restoration), `Veiling.picker` + style, `SiteStyles.script`, `AutoScroll`, `Swipe.watch`, `ImageRelay.keepMenu` (only when an extension has `contextMenus`), `Isolate.watch` (floating video), `LiveRate` (page world), `ExtensionExternal` (page world), `FormRelay.withoutPasskeys` or the passkey page patch, `PageTranslator.announce`, `Dusk.script`, `YouTubeAds.page` + `YouTubeAds.skipper`, `PageFind` (on demand), `Snap` (on demand), `Reader` (guess in page world; twin fetch/show in Search world), and `SiteSearch.lookup`. The list is compared by object identity and only replaced when changed, avoiding a re-send on every link.

---

---

## 6. Spaces and profiles

### 6.1 Spaces (`spaces`, ○)

A space is a separate set of tabs in the same window, with its own sign-ins and (optionally) its own downloads folder. The first space is `Personal`; it uses the WebKit store and session that existed before spaces.

- **What a space owns**: its tab row, its parked tabs, its split pairs and tab groups, its pin list and pin folders, its page data store (cookies/localStorage/cache) only when it has its own, its own history when it has its own store, and an optional downloads folder. Bookmarks are **per profile**, not per space, even for a signed-out space. History is per profile for shared-store spaces, and a signed-out space gets its own history file.
- **What is shared**: settings, extensions (one `WKWebExtensionController` for the app), passwords (keychain), the downloads *list* (the folder can be per-space), favicons, bookmarks.
- **Create**: Space menu → New Space… (name; optionally share sign-ins with the first space; optional icon/colour). Creating past the last space by swipe also creates one in place.
- **Switch**: `⌃1`–`⌃9` (or `⌘1`–`⌘9` with `spaces.commandNumbers`), click the space dot, the Space menu, two-finger swipe over the tabs, mouse wheel notch over the tabs, mouse back/forward buttons over the tabs.
- **Swipe details**: axis locked at gesture start; 50 pt threshold; 0.4 s rest; 0.2 s back-stroke guard; past the last space creates a new space; rows follow the fingers; the icon flips like a page; with profiles in one window, a swipe past the first/last space continues into the next/previous profile.
- **Rename / move / delete**: menu. Delete erases its cookies/sign-ins, closes its tabs, and — if it was a signed-out space — retires its own history, despite the confirmation copy saying "History and bookmarks stay". Bookmarks are per profile and do stay. Store deletion is asynchronous and retried after 3 s/15 s and at launch (`spaces.erasing`), because WebKit may leave data behind.
- **Appearance**: 6 colours (blue, green, orange, red, violet, none), 24 SF Symbols icons in four groups (work, thinking/AI, leisure, life), a 2–3 colour gradient with angle and grain (`SpaceGradient`), strength Subtle/Rich/Vivid (7/11%, 18/22%, 30/40% wash in light/dark). Six gradient presets (Sunrise, Lagoon, Orchard, Dusk, Glacier, Ember), 14 swatches, eight directions. The wash tints the chrome (column/strip, bookmarks bar, ground around the card) but never the page. On glass, the colour/gradient is worn by the glass itself. Gradient colours are mixed in OKLab and held to a lightness band so tab greys still read.
- **Shared pins**: pins marked "Keep in Every Space" appear in every space of the profile, after that space's own pins.
- **Space routes** (`spaces.links`, ○): map a site to a space. Links from other apps and new-tab links from pages in other spaces open there with that space's sign-ins. Subdomains included; typed addresses, links followed in their own tab, and private tabs are not routed. The tab menu can add/remove a route for the current site.
- **Ask which space** (`links.ask`, ○): for external links, a panel lists every space; a number key picks one; Return picks the marked one; "Always" saves a route.

### 6.2 Profiles (`profiles`, ○)

A profile is a separate browser inside the app: its own spaces, tabs, cookies/sign-ins/site storage, history, bookmarks, and archive. There is always `Personal`; everything before profiles lives there.

- **Window ownership**: a window belongs to one profile for life. Its column shows only that profile's spaces, history suggestions, and tabs.
- **Create / rename / delete**: Settings → Spaces & Pins → Profiles, or File → New Window → Profile → New Profile…; delete asks and removes its data files and stores.
- **Profiles in one window** (`profiles.oneWindow`, ○): choosing another profile's space brings that profile's window into this one's frame, full-screen state, and fold state; the previous window is kept loaded and hidden ("tucked"), out of Mission Control/`⌘``. Closing the visible one closes the merged group; `⇧⌘T` reopens it. Turning the option off untucks all windows.
- **Shared**: settings, extensions, passwords, downloads, favicon cache.
- **Not shared**: history, bookmarks, spaces, pins, tabs, cookies, saved sessions, archive.

---

## 7. Pins, pin folders, and live folders

### 7.1 Pins

A pin is a page kept in the first part of the row. It can be a **square** (letter or site icon, default) or a **row** (title + icon, with pinned rows enabled).

- **Pin / unpin**: tab menu, Tabs menu, `⌘`-click a pin (with `pins.commandNewTab`), drag across the boundary.
- **Pin identity**: each pin has a stable `pinID` shared across windows and (optionally) spaces; a pin is the same in every window.
- **Home**: the address it was pinned at. `pinsGoHome` makes it wake at home when closed with `⌘W` or reopened after quit; `pinsPeek` makes links to other sites open in a peek so the pin stays put. Both are halves of "Pins keep their site" (default on).
- **Double-click a square** → go home (or edit its letter if already home). A row has "Back to Pinned Page" in its menu instead.
- **⌘-click a pin**: if it wandered, the wandered page becomes a loose tab and the pin gets a fresh tab at home; if at home, a new tab at that address opens.
- **Letter editing**: click "Change Letter"; a single character (or a glyph) shows on the square.
- **Replace Pinned Page with Current URL**: rehome a pin to where it is now.
- **Delete**: unpins and closes in one action (menu).
- **Pin numbers**: `⌘1`–`⌘8` target the first eight of the shortcut row (pins first, then group headings, then loose tabs); `⌘9` is always the **last** item. A folded group counts once; a group's sleeping tabs don't count. Pressing a pin's number again returns to the tab you were on before it (`pinNumberGoesBack`, ◐).
- **Pin peek**: a plain click on a link to another site inside a pin opens it in a peek; links within the site stay in the pin.
- **Pinned rows** (`pins.list`, ○): squares and rows; a "Clear" line above loose tabs.
- **Shared pins** (`pins.shared`, ◐ with spaces): "Keep in Every Space" / "Only in This Space".

### 7.2 Pin folders (`pins.folders`, ○, requires pinned rows)

- Folders among **listed rows of the space's own**, nested up to 3 levels. Squares, shared pins, and private tabs cannot enter a folder.
- Click a folder heading to fold/open; menu: Rename, Expand/Collapse, New Folder Inside, New Live Folder Inside, Move Folder To, Delete. Delete lifts child folders/pins to the parent; no pins are closed. Names max 200 characters.
- Drag a pin onto a folder; drag a folder by its heading to nest/move; a line or highlighted heading shows the landing place; a folder cannot be moved into itself/descendant or into a live folder.
- A folder is not a group: its pins remain pins, and it is Arc's pinned-list model rather than tab groups. Shared pins are never in folders.
- Imported Arc folders become pin folders (and Arc's folder pages are pins in them); same-name folders are reused, depth capped at 3.

### 7.3 Live folders (`pins.folders` + feed)

- A pin folder filled from an RSS or Atom feed (e.g. a blog, GitHub `releases.atom`/`commits.atom`); the feed's latest 12 posts, each a line that opens in a new tab.
- Fetched when created, 5 s after launch, every 30 min while running, and on Refresh. A private, cookieless, cacheless session; gives up past a timeout or 2 MB. Failure keeps the old posts and says so in the heading's tooltip.
- Never a pin, never drag-and-dropped; stored in `pin-folders.json` with its feed URL and posts.

---

## 8. Tab groups

- Named sections of ordinary tabs in one space; a group has a name, folded state, and optional colour (`TabGroup`).
- **Create**: tab menu → Move to Group → New Group; right-click a group → Rename Group.
- **Move tabs**: drag tabs into/out of groups; tab menu → Move to Group; group menu → Ungroup Tabs; a tab opened from a grouped tab joins its group.
- **Fold/unfold**: click the heading or its fold mark; group menu → Collapse/Expand Group; View → Fold All Groups `⌥⌘[` / Unfold All Groups `⌥⌘]`.
- **Colour**: group menu → Colour… (same palette as spaces).
- **Close Group**: closes the group and its tabs; `⇧⌘T` brings the whole group back. Empty groups disappear.
- **Move Group to Space**: the whole group moves, still a group, into another space.
- **Group numbering for extensions**: Chrome's `tabGroups` id is derived from the group UUID.
- Groups are kept in the session even when the feature is off, so turning it on later loses nothing.

---

## 9. Bookmarks

- **Add This Page** (`⇧⌘B`) or the Bookmarks menu; if the page is already bookmarked, the menu item becomes Edit Bookmark…
- **Structure**: a tree of sites and folders, arbitrary nesting; a folder has a title and children; the empty top level is "Top Level".
- **Manage**: Bookmarks page / panel; add folder, new folder inside, rename, edit address, remove (with count of what goes), move to folder, drag/drop reorder and nest; a folder opened shows its first child's place.
- **Open**: click opens in the current tab; menu/⌘-click options open in a new tab, new window, private tab, Split View, or a new tab group. A folder can "Open All in New Tabs", "Open All in New Window", or "Open All in Tab Group".
- **Bookmarks bar** (`bookmarks.bar`, ○): a row above the page; folders open as menus; context menu on empty space: New Folder, Show Bookmarks, Hide Bar. Items: Open in New Tab/Window/Split/Private, Rename, Edit Address, Copy Link, Remove; folders: Open All…, Rename, New Folder Inside, Remove.
- **Suggestions**: bookmarks appear in the address field, scored alongside history; folders are offered as "slips".
- **Menu bar**: the Bookmarks menu lists the tree lazily (AppKit builds it as it opens, to avoid a 230 ms launch cost with 1,500 imported bookmarks).
- **File**: `bookmarks.json` (per profile: `bookmarks-<profile>.json`); imported bookmarks are marked "Imported from <browser>"; duplicate URLs are merged rather than duplicated. There is **no bookmark export writer** — only readers/importers; the only File-menu export is PDF.
- **Search/page**: bookmarks page has a 600 pt panel, 440 pt list, search-all-words, folder paths, "Show in Folder", new folder, bring-in, file import, total count. The bookmark button dropdown shows an outline, Add/Edit This Bookmark, and Manage Bookmarks…; if the top level is one folder, it opens expanded.

---

## 10. History, archive, and recently closed

- **Recording**: every committed web page visit (not internal pages, not private tabs) is recorded with URL identity, title, visit count, and last visit. History is per profile; spaces share their profile's history. The address field's completions, frecency, and site trails come from it.
- **Identity**: fragments ignored; scheme/host normalised; path/query spelling preserved; `www.` handled for labels.
- **Panel**: search everywhere you've been (single contiguous substring over address/title); grouped by day ("Today", "Yesterday", then dates) with times; per-profile; remove a row; Clear History (all, or a specific signed-out space's own); Clear Browsing Data. **No date-range deletion.**
- **Caps**: 2,000 places on disk, 2,000 in memory with 250 slack, visit counts capped, low-value old entries pruned by frecency; saves coalesce for 1.5 s; corrupt files are quarantined.
- **Clear**: History clears all/one scope, the archive, link-chooser choices, favicons, learned site searches, and download source addresses. Cookies/sign-ins signs out of every store of the profile. Cache clears HTTP/memory/offline/fetch caches only; service workers stay.
- **Suggestions**: history contributes visited pages, host trails, and site-search memory; a known site not yet visited is also offered.
- **Recently Visited**: the History menu shows the latest visits with favicons.
- **Recently Closed**: `⇧⌘T` and History menu list up to 10 closed tabs/windows; a reopened tab keeps its Back/Forward/scroll if it was recent; a closed group returns as a group.
- **Archived Tabs**: tabs Search closed automatically, newest first, 500/30 days, searchable; click reopens in the current space; never includes private tabs or tabs you closed yourself. `archive.json` per profile.
- **Clear Browsing Data**: history, cookies/sign-ins, cache; "Forget choices" for site permissions; private data never touches any of it.
- **iCloud history?** No. History, passwords, cookies, and sign-ins never sync.

---

## 11. Downloads

- **Start**: normal link downloads, `Content-Disposition`, `window.open` downloads, "Download Linked File", "Download Image", extension downloads.
- **Destination**: the folder in Settings (default `~/Downloads`) or a space's own folder; "Ask where to save each file" (`downloads.ask`, ○) shows a save sheet per file.
- **List**: Downloads tab / floating card / side panel (`downloads.open`: tab, popover, panel); the button appears only while downloading unless `downloads.button`.
- **Progress**: bytes/total, percentage, indeterminate when unknown; Dock progress? (the app updates its Dock tile in the standard way).
- **Controls**: pause only for replayable GET/HEAD with no body/body stream; resume needs the in-memory resume data + cookie store; retry from the start for failed replayable GET/HEAD; cancel; remove from list. POST/form downloads cannot pause or retry. Pause data, request, store, destination and partial-file identity are **in-memory only** — quitting loses paused downloads.
- **Naming**: duplicate names get " 2", " 3", etc.; a retry renumbers afresh. An extension-requested subfolder is sanitized against traversal; the page's suggested name keeps only its last path component.
- **Open / reveal**: click a finished row opens the file; "Show in Finder" reveals it; the list remembers downloads in `downloads.json` (memory of files, not the bytes), capped at 50.
- **Private downloads**: use the private tab's ephemeral store; no cookies leak to or from normal tabs; the file still lands in the chosen folder, but it is **not listed** in Downloads and gets no source address; it still counts for the global progress ring.
- **Notifications**: **no Notification Center notifications for downloads**; only the in-app announcement, Finder/Dock progress, and the Dock's Downloads stack notification. Failures show the reason; the row's menu can copy the address. Clear list leaves files on disk; clearing History strips source addresses but leaves rows.
- **Failure states**: unknown error, server refuses pause, file changed while stopped, destination panel cancelled (no error).
- **Download button**: at the sidebar foot or top doors; shows progress and pulses on start; "Open Downloads" from its button, `⇧⌘J`, or History → Downloads; button can be always visible (`downloads.button`).

---

## 12. Privacy, blocking, and permissions

### 12.1 Ad and tracker blocking (● `shield`)

- A `WKContentRuleList` compiled once at launch and enforced inside WebKit's networking, before any request or stylesheet.
- **Network rules**: ~50 third-party ad/analytics/tracking domains (DoubleClick, Google Ads/Analytics/Tag Manager, Amazon Ads, AppNexus, Trade Desk, Criteo, Taboola, Outbrain, Rubicon, PubMatic, OpenX, Casale, Smart AdServer, Sharethrough, Index Exchange, Bidswitch, 33Across, Teads, Moat, AdRoll, comScore, Quantcast, Chartbeat, Hotjar, Mouseflow, FullStory, Clarity, Mixpanel, Amplitude, Segment, Branch, AppsFlyer, Adjust, TikTok Analytics, Facebook Connect, Twitter Ads).
- **Cosmetic rules**: a deliberately short list of ad containers/iframes and generic slots (`.adsbygoogle`, GPT/Taboola IDs, `.ad-slot`, `.ad-leaderboard`, etc.), plus a few site-specific selectors (AS/El País `.ad`, Guardian top banner, Independent billboard, CNN header).
- **YouTube ads**: two scripts. One in the page world removes `adPlacements`, `adSlots`, `playerAds`, `adBreakHeartbeatParams` from YouTube's player responses (only responses shaped like YouTube's). A fallback in Search's world mutes and 16×-plays while `ad-showing`, restoring sound/speed afterwards; it never seeks. Only on YouTube hosts, only when Shield is on and not paused for the site.
- **Per-site pause**: site card → Block Ads and Trackers off; kept in `shield.paused`.
- **First-load wait**: the first page loads wait up to 1 s for the list to compile; after that no wait.
- **No counter, no shield icon**: the only UI is the site card.

### 12.2 Tracking prevention and privacy signals

- **Prevent cross-site tracking** (●): WebKit's resource-load statistics, as Safari's switch. Off (`sites.keep=true`) also sets the cookie policy to `always`, so framed sites keep cookies.
- **Global Privacy Control** (●): `Sec-GPC: 1` on every request and `navigator.globalPrivacyControl`; WebKit feature on macOS 27+; private tabs always.
- **Remove click trackers** (●): WebKit strips `gclid`, `fbclid`, and its system click-ID list before load; `utm_*` campaign tags stay; private tabs always. This is separate from Copy Clean Link's list, which may differ.
- **Private tabs**: ephemeral store, no history, no favicons, no password offers, no Apple Passwords, no notifications, no Handoff, no iCloud sync, no session record, no extension private access unless granted, no per-site permission persistence, no `Always` routes. They keep tracking prevention/GPC/click-ID stripping on.
- **Clear**: history, cookies/sign-ins, cache. "Forget choices" for permissions.
- **Proxy** (`Proxy.swift`): HTTP or SOCKS5 proxy for the whole browser, set in Settings → Privacy; WebKit configuration through the proxy; no PAC.
- **Certificate errors**: see §5.9.
- **No telemetry / analytics / crash reporting**; only the update check and requested pages leave the Mac.

### 12.3 Permissions

- Camera, microphone, location, notifications, screen sharing; per origin, stored under `capture.<origin>|<kind>`; defaults ask.
- A card over the page names the site and the request; "Allow", "Allow once" (where applicable), "Always allow", "Don't allow"; a private tab is never asked and gets a refusal.
- Site card → Site Permissions shows and changes each; notifications off globally disables the notification request.
- `Forget choices` in Settings resets every origin.
- Location uses CoreLocation, asks per site, and can use "Always allow"; the first request triggers the macOS location prompt.
- Screen sharing opens macOS's own picker and shows a recording indicator on the tab and in the menu bar.

---

## 13. Passwords, passkeys, and forms

### 13.1 Passwords (● save/fill)

- **Storage**: macOS keychain, `kSecClassInternetPassword`, label `Search` (or `Search (<world>)` in tests); `kSecAttrServer` = bare host, `kSecAttrAccount` = username (empty stored as no attribute), password in `kSecValueData`; authentication type HTML form; protocol HTTPS, or **HTTP when saved from a plain-http page** (that is how the `clear` flag is stored); last-used time in the comment; accessible when unlocked; **not** synchronizable. Nothing on disk, nothing logged. Updates are scoped by server + account + label, so another app's item is never overwritten.
- **Saving**: after a submitted sign-in, Search waits for evidence it worked (a new document, or 1.5 s after the password fields disappear) and offers Save/Update. It will not save from a private tab, an agent tab, an extension page, a site in `Vault.never`, or a password just filled from Apple Passwords. Same account + same password only updates the last-used time; a different password offers Update. The offer expires after 45 s; a two-step flow's name is remembered in memory for 5 min. "Never here" is the permanent per-site refusal; "Not now" dismisses only that offer. There is no separate "once per site" suppression beyond this.
- **Filling**: nothing fills automatically. Clicking a judged sign-in box hangs the site's accounts under it (passkeys first, then this host's keychain accounts, this host's Apple accounts, other accounts of the same registrable domain, Apple codes, Connect/Offer, Contacts; max 5 keychain + 5 Apple + 8 passkeys + 4 contact rows). On http, only `clear` accounts are offered. The list ignores clicks for its first 0.5 s; ↓/↑ + Return choose, ⇧Return fills only the focused box, Escape closes; IME composition wins. Filling uses the native value setter and fires input/change; the page must still be the same scheme/host, and the origin is rechecked.
- **List/page**: Passwords tab (620 pt) shows site, username, last used; search; add manually. Show requires Touch ID (`LAContext.deviceOwnerAuthentication`), masks as ten dots regardless of length, auto-hides after 15 s. Copy also requires Touch ID, uses a host-only/concealed/transient clipboard and clears after 90 s. Single Remove is immediate and **not** Touch-ID-gated; Remove All (only the rows currently shown) asks then authenticates. If the Mac has no authentication policy, `prove` allows — a deliberate fallback.
- **Import**: from Chromium/Firefox browsers or CSV (see §15); keychain keys may need the login password once. **No password export exists** (an error message points to a nonexistent export UI).
- **Private tabs**: saved Vault passwords can still be offered (the tab has its own origin), but Search never offers to save from one (`!tab.shy`), never uses Apple Passwords there, and keeps private exceptions in memory only. Contact fill is also available in private tabs.
- **Security**: page scripts never receive saved passwords; fill is triggered by a real choice; secrets are read one item at a time only when needed; no secret is written to defaults, logs, or error text.

### 13.2 Passkeys (● when entitled)

- Search carries out the WebAuthn ceremony itself (not WebKit) because a conditional-mediation request wedged open by a crash can wedge `AuthenticationServicesAgent` for the whole Mac. Requires `com.apple.developer.web-browser.public-key-credential` and a matching provisioning profile; an ad-hoc build has none and hides the offer. The first ceremony triggers macOS's one-time browser passkey authorization. `passkeys` defaults on only when entitled; switching between signed/unsigned builds resets the choice (`passkeys.for`, `passkeys.entitled`).
- Conditional mediation: the page-world patch turns `navigator.credentials.get` into a waiting request; platform credentials for the RP are listed under the field (max 8); choosing one opens the Mac's sheet for that credential only. Only the main frame or a same-site frame; origin + committed page URL + scheme/host/port checked; RP ID must fit the host and not be a public suffix; challenge required; page must be the same view/origin; only the active/key window (except conditional waiting).
- Create/get: ES256 default; platform and security-key authenticators; resident-key/attestation/exclusions/transports; PRF on macOS 15; user verification; security keys from macOS 14.4. **Conditional create is not supported** (`NotAllowedError`). Credentials come from iCloud Keychain and installed password managers via AuthenticationServices; Search stores no passkeys.
- Private tabs: passkeys have **no private-tab check** — they work under the same origin/focus rules. This differs from Apple Passwords.
- Fallback when unavailable/off: `PublicKeyCredential` remains present, but conditional/UV capabilities report false and public-key create/get reject with `NotAllowedError`, so sites fall back to passwords instead of wedging. An extension's own WebAuthn patch is detected and left alone.
- The page-world patch is installed in every frame; responses go through Search's isolated world; results are sealed to a per-request ECDH key for PRF; cancellation maps to `AbortSignal`.

### 13.3 Apple Passwords app (○ `passwords.apple`, unofficial)

- Runs Apple's Chrome-extension helper as its Chrome extension id, over native messaging. Pairing is SRP-6a (3072-bit group, SHA-256) with the helper's six-digit code; messages are AES-GCM under the shared key. The key and pairing live in memory only and need a new code every launch. The helper is only run from fixed system paths and must be Apple-signed (`com.apple.PasswordManagerBrowserExtensionHelper`, `anchor apple`). It is kept alive by a tab-event ping every 240 s and on wake.
- Offers account names under a sign-in box on https pages of ordinary tabs, never private, never http, never cross-site frames; only the page's registrable site; max 5 names and max 5 codes. Password/code requests wait up to 120 s for Touch ID; values are filled into the page and never kept. Codes exclude SMS-delivered entries. The helper's "Passwords not saved" marker is not an account.
- "Use your Apple passwords?" is offered once per site, only when Apple Passwords is off, Search has no account for the site, the helper exists and the page is https/non-private; it retires after two ignored appearances. Clicking turns the switch on and starts pairing; ✕ is permanent.
- Settings shows Connect/Disconnect, a six-digit code field that submits at six digits, and the pairing card under the sign-in field (Escape cancels). It says "Unofficial" and "Never in private tabs".

### 13.4 Forms and contact fill

- **Contact card** (`contacts.fill`, ○, off): only your own "Make This My Card" card, read off-main-thread, refreshed on Contacts changes. Fields: name, given/additional/family, organization, email, tel, tel-national, street-address, address-line1/2, city, state, postal-code, country/country-name. Max 4 rows; label/title-cased. Choosing fills only visible, empty, enabled boxes in the same shipping/billing group, up to 40 boxes; selects only from defaults; postal codes can truncate to five characters, everything else is skipped rather than cut; street addresses become comma-separated for single-line inputs; filled boxes flash accent at 30 ms stagger. Works in private tabs; writes nothing. macOS Contacts permission is asked when the switch is turned on.
- **Text replacements / smart quotes / smart dashes**: see §5.9. Text Replacement follows macOS; smart quotes/dashes default off.
- **One-time codes**: only from Apple Passwords. Code boxes are detected by `autocomplete=one-time-code`, a row of 4–8 one-character cells, or OTP/2FA/TOTP/passcode/verification words; promo/coupon/postal/area/country/referral/captcha/backup/card/license/IBAN/CVV are excluded. A guessed code box gets only this host's codes; an explicit code box can get the site's other hosts' codes. Single boxes are set; a row of cells is filled only when the code length matches.
- **Credit cards**: not supported. No storage, no autofill; card fields are excluded from username/password and contact detection; agent reads redact them.
- **Autofill dropdowns**: account list positioned from the field's rect, 240–360 pt wide, below or above as room allows, scrolls; footer names the source; arrow keys + Return; Escape; click; only one list at a time; 0.5 s click guard.
- **Frame scope**: the form script is main-frame only, so sign-in/contact filling does not operate in subframes.

---

## 14. Extensions

### 14.1 Engine and install

- Built on **`WKWebExtension`** (Safari 18.4+/macOS 15.4+): the same WebExtension engine Safari uses, reading Chrome's `manifest.json`. One `WKWebExtensionController` for the app (not per profile), so each extension's worker runs once; extension data lives in WebKit's extension store. Extensions are loaded 400 ms apart (loading all at once makes WebKit fail some workers permanently).
- **Chrome Web Store (CRX3)**: paste a store link or a bare 32-letter id, or open the store page and press "Add to Search" (Search injects that button over the store's own). Download is from `clients2.google.com/service/update2/crx`. The CRX3 signed header must carry a `crx_id` equal to the requested id, and an RSA proof whose SHA-256 key prefix matches it must verify over the signed payload. Unpacking rejects any symlink. After signature verification Search **rewrites the package to add its shim** (prefixes the worker, injects HTML, adds content-script entries, writes `.search-shim`/`.search-added`; it only adds, except DNR rules WebKit would misread, which it deletes).
- **Load unpacked**: choose the folder containing `manifest.json`; the folder id is the first 16 bytes of SHA-256 of the real path (a–p), not the manifest's `key`, so a folder can't impersonate a store id. Reload re-copies the source only if it still resolves to the same id. Developer mode is implicit.
- **Import from another browser**: only Chrome Web Store ids, installed fresh from the store one at a time with the normal permission prompt; already-installed ids are skipped. There is **no Safari `.appex` or Safari extension import**.
- **Updates**: checked hourly but rate-limited to once per 20 hours. New version → verified CRX → unpacked → loaded → all grants compared. Any new permission/site pattern/Search-answered API triggers a prompt (Cancel is default; Return/Escape cancel). Approved updates swap atomically (`renamex_np(RENAME_SWAP)`), unload the old context, save metadata, and reload if enabled.
- **Background/service worker**: WebKit manages it. Search clears stale extension service-worker registrations at launch when the shim version or extension version list changed (only if at least one extension is installed) and recovers a failed worker after 2 s, at most once per 60 s.

### 14.2 UI

- A puzzle button appears when at least one extension is installed; pinned extensions are inline 26×26 buttons with badges; all running extensions appear in the puzzle menu with Pin/Unpin, Options…, Reload, Runs on, Remove.
- **Popup**: Search's own `NSPopover`, not WebKit's, because some popups stall under WebKit's. It follows Chrome's 25–800 × 25–600 auto-size algorithm; a click on its own button closes it; `window.close()` works; a link opening a new window becomes a tab and closes the popup.
- **Side panel**: `chrome.sidePanel` is implemented; per-tab or global; open/close/setPanelBehavior/getPanelBehavior; action click can open it; if it can't be created, the page opens as a normal tab.
- **Options page**: opens as a foreground tab. **New-tab override**: an extension asks once; only one extension can own new tabs; Settings offers Search's own or one asker; "Stop Showing in New Tabs" reverts.
- **Commands**: manifest `commands` become WebKit commands and are adopted into the shortcut store unless reserved; users can rebind them.
- **Settings › Extensions**: icon/name/version/source, "couldn't start" reason, "can't do" limitation, Runs on, ellipsis menu (pin, new-tab page, options, reload, private tabs, remove), enable switch, per-site list. **Remove from the Settings row has no confirmation**, while Remove from the context menu does.
- **Site access**: All sites it asks for (default) / Only sites I name / On click. Per-host "Always on" and "Run on This Site"; a click grant lasts until the tab leaves the site; private tabs need explicit "Allow in Private Tabs". Extensions that need every site to fill sign-ins are warned about, never forced back.

### 14.3 Capability layers

**WebKit provides** `runtime`, `tabs`, `windows`, `storage`, `alarms`, `contextMenus`/`menus`, `action`/`browserAction`, `scripting`, `webNavigation`, `webRequest`, `declarativeNetRequest`, `permissions`, `cookies`, `extension`, `i18n`, and messaging/ports.

**Search's shim** (`ExtensionShims.swift`, ~6,800 lines of JS + Swift) fills or patches:

- Whole namespaces WebKit lacks: `bookmarks`, `history`, `downloads`, `browsingData`, `sessions`, `topSites`, `readingList` (empty), `sidePanel`, `offscreen`, `tabGroups`, `identity`, `management`, `notifications`, `tts`, `idle`, `power`, `privacy`, `contentSettings` (stubs), `proxy` (stored, not applied), `system`, `omnibox` (stub), `declarativeContent` (stub), `desktopCapture`, `tabCapture`/`pageCapture`/`debugger`/`gcm`/`instanceID` (refused), `userScripts`.
- Patches on WebKit namespaces: `runtime` update/suspend/restart/getContexts/onInstalled reasons, `extension` getURL/getViews, `tabs` enums/zoom/group/ungroup/discard/highlight/private marker, `windows` bounds event, `storage` managed/null-prototype/popup `onChanged`/framed localStorage mirror, `scripting` ExecutionWorld/StyleOrigin, `action` popup routing, `webNavigation` enums/events, `webRequest` filter sanitation, `declarativeNetRequest` constants/header-rule matching, `permissions`, `alarms` callback form, `cookies`, `commands`.
- **Native messaging**: host manifests from Chrome/Chromium/Edge/Brave/Arc/system/Vivaldi/Opera `NativeMessagingHosts/*.json`; exact `chrome-extension://<id>/` origin required; executable check; Chrome framing (4-byte length + JSON, 1 MB cap); one-shot and long-lived ports; 2 s SIGTERM→SIGKILL; orphan cleanup; 25 s worker keepalive.
- **OAuth**: `identity.launchWebAuthFlow` opens a tab; only `https://<extension-id>.chromiumapp.org/…` is intercepted as the answer; closing/10 min declines; the callback page is never loaded. Non-interactive flow uses a hidden 800×600 web view with no popups/dialogs/downloads/app launch and reduced permissions, 60 s cap. `identity.getAuthToken` is unsupported.
- **Offscreen documents**: one per extension, extension-owned page only, 30 s load deadline; only recording-reason documents may get mic/camera; main-frame navigation limited to the extension's origin; downloads cancel; crash closes it; `runtime.getContexts` includes it.
- **WebSocket**: worker WebSockets are proxied through URLSession because WebKit's worker WebSocket blocks the worker's main thread; ephemeral session, no cookies/cache/credentials; Chrome-like Origin/UA; text/binary relayed; close codes preserved.
- **Credentialed fetch**: extension-page/worker `fetch` to a granted http(s) URL is sent natively so SameSite=Lax/Strict cookies go, as Chrome does; redirects rechecked; cookies from the relevant window's store; no private store; DNR header rules applied; forbidden headers protected; streaming responses relayed.
- **Page scripts blocked by CSP**: an extension's own `<script src="chrome-extension://…">` blocked by a page's CSP is recovered for the top frame only, within `web_accessible_resources`, with `scripting` or a matching content script, single-use token, no user gesture, evaluated in the `WebExtension-<id>` world.
- **External website messaging**: `externally_connectable.matches` only; broad wildcards and Google domains filtered; exposed as `window.chrome.runtime.sendMessage/connect`.

### 14.4 What extensions see

- All ordinary tabs (subject to site access), their URLs/titles/favicons, the active tab, windows, groups, bookmarks, history, downloads, cookies (with permission), storage.
- **Private tabs** are hidden unless explicitly allowed; an allowed extension still runs in the private tab's ephemeral store. Private tabs created **before** any extension was allowed never gain extensions, even after toggling; the UI advises reopening them.
- **Other extensions' pages** are sealed: no web view, URL, title, favicon, or snapshot.
- **Agent tabs** are ordinary web tabs with `agent=true`; an extension with site access sees them as normal tabs. Search does not hide agent tabs from extensions. Search's own page-reader redaction applies to agent tooling, not extension content scripts.
- **Search's keychain / Apple Passwords / passkeys** have no `chrome.*` API. A content script granted page access can still read DOM values, as in Chrome. An extension can suppress Search's save offer via `chrome.privacy.services.passwordSavingEnabled=false`.
- Extension pages are served under `chrome-extension://<id>/`; Search's `search://` pages are not exposed.

### 14.5 Known gaps (critical for a rewrite)

- **`webRequestBlocking` cannot block or rewrite requests** — WebKit has no such API. uBlock Origin's MV2 in-page filters work; no network request is blocked. This is surfaced in Settings and at install.
- `tabCapture` entirely unsupported; `pageCapture.saveAsMHTML` unsupported; `debugger` unsupported.
- `management` can only see itself; `readingList` is empty; downloads lack pause/resume/cancel/removeFile/getFileIcon and real events except synthetic blob completion; `identity.getAuthToken` unavailable.
- `contentSettings` always reports "allow"; `proxy` and most `privacy` settings are stored but not applied (except `passwordSavingEnabled`); `tabGroups.move` unsupported and extension-visible groups are always grey.
- No Safari app extensions / `.appex`, no Safari gallery.
- Extensions are the single largest engine-specific subsystem (~14,500 lines across 18 files) and have no equivalent on WebView2/WebKitGTK. Any cross-platform port needs a capability matrix per engine or a bundled Chromium (CEF).

### 14.6 Security

- CRX3 only; signed payload and `crx_id` checked; RSA proof must match the id; symlinks rejected. Unpacked folders strip outside symlinks; id is path-derived.
- The shim rewrite happens **after** signature verification and only adds (except misread DNR rules, which are deleted).
- Permission set includes WebKit permissions, site patterns (`site:`), and Search-answered API names (`search:`); an update asking for more is prompted with Cancel default. Reload of an unpacked extension follows the same check.
- No extension is ever granted another extension's pages, all extension pages, or `file:` URLs; `<all_urls>` is narrowed to `*://*/*`.
- Extension windows cannot be private; popups limited to http(s)/the extension's own page/about:blank; `javascript:`/`file:` navigation refused.
- Native hosts must list the exact origin; messages capped 1 MB. External page messaging only for listed origins.
- Capture is opt-in per extension, one-time stream token 60 s, macOS picker, recording pill.
- Extension fetch is granted-URL only; redirects rechecked; no private-tab cookies; forbidden headers protected. Extension WebSockets are ephemeral.

---

## 15. Imports

### 15.1 Direct reads from this Mac

| Family | Browsers | What is read |
|---|---|---|
| Chromium | Dia, Chrome (stable/Beta/Dev/Canary), Chromium, Arc, Brave, Edge, Vivaldi, Opera, Opera GX, Helium, Comet, Thorium, Naver Whale, Island, Sidekick | Bookmarks, history, passwords (keychain "Safe Storage" key), extensions, profiles, open tabs from session files; Arc's sidebar (spaces, pins, folders, favourites); `Login Data` + `Login Data For Account` |
| Chromium without passwords | ego lite, Yandex, ChatGPT Atlas | Bookmarks, history, tabs, extensions only |
| Mozilla | Firefox, Zen, Floorp, LibreWolf, Waterfox, Mullvad Browser | Bookmarks, history, passwords (`logins.json` + `key4.db`, only with no primary password), open tabs/session, icons; Zen spaces/pins/Essentials mapped to Search spaces/pins |
| WebKit | Safari, Orion / Orion RC | Bookmarks and history; Safari files only if macOS already allows them — Search **never asks for Full Disk Access**, so Safari is omitted with export instructions otherwise |

- **Profiles**: root plus subfolders containing `Login Data`, `Bookmarks`, `History`, or `Extensions`; symlink-resolved and confined to the browser root. Chromium `Local State` supplies profile display names and last-used; Guest/System excluded. A user-chosen data folder is trusted only for the process, never persisted.
- **Chromium passwords**: `Login Data` and `Login Data For Account` are copied with `-wal`/`-shm` before reading; the key comes from the login keychain (one prompt per browser); PBKDF2-HMAC-SHA1, salt `saltysalt`, 1003 rounds, 16 bytes; `v10` + AES-128-CBC + 16-space IV + PKCS#7; a 32-byte hash prefix is stripped. `blacklisted_by_user` becomes `Vault.never`; skipped/undecrypted counts are reported.
- **Mozilla passwords**: `key4.db` + `logins.json`; PBES2 (PBKDF2-SHA256 + AES-256-CBC) and legacy PBE-SHA1-3DES; base64 DER fields; a primary password means no attempt — the UI tells the user to export CSV.
- **Extensions**: only Chrome Web Store ids; `location == 1` or `from_webstore == true`; default/OEM installs, themes, and non-Google update URLs rejected; fallback to `Extensions/` folder names. Installed fresh with confirmation.
- **Open tabs**: Chromium session files, Gecko `sessionstore-backups` (`recovery.jsonlz4`/`sessionstore.jsonlz4`), up to 300 tabs; popup/private/taskbar windows skipped; restored asleep.
- **Bookmarks**: merge, not replace. If Search has none, the imported tree becomes the top level; otherwise it merges into a folder named for the browser, or at top if that browser owns the top. Only the same URL at the same level is skipped; user edits are never deleted/reordered.
- **History**: merged by URL identity; counts use `max`, last-visit uses the later date, missing titles filled; imported into the profile's history.
- **Import records**: `import.records` stores per-browser counts and bookmark IDs, so "Bring in from X again" can show what was brought and "Replace" can withdraw exactly the prior imported bookmark IDs.

### 15.2 File imports

- One chooser accepts Netscape HTML bookmarks, CSV passwords, JSON Safari history, and Safari's "Export Browsing Data" ZIP. The ZIP is extracted with `ditto` into a per-process scratch folder, cancelled with terminate→SIGKILL, and swept at launch. Safari's export is detected by its history file, not filenames; after import Search warns that the export holds passwords in the clear.
- HTML parser: loose Netscape format, any case, omitted closing tags, `PERSONAL_TOOLBAR_FOLDER` goes to the top, folders to 64 deep, only http/https.
- CSV: headers `url/login_uri/website/site`, `username/login_username/user/email`, `password/login_password`, case-insensitive; quoted fields, doubled quotes, commas/newlines inside fields; rows without a valid host or password are skipped; http-origin rows are marked `clear`.
- File imports run async with progress/cancel (≤10 updates/sec, 64 KB chunks); JSON reads are `O_NOFOLLOW`, regular-file checked, 64 MB cap; a failed import leaves nothing half-written.
- Import panel defaults: passwords/bookmarks/history on; extensions off; open tabs/groups off; Arc/Zen spaces and pins on; Replace bookmarks off; when opened from Settings › Extensions, only extensions on and the first browser with store extensions selected. Counts are computed before asking for any keychain key.
- `ImportFile.Control` tracks cancellation; quitting waits up to 5 s for workers to remove temp files.

### 15.3 Session/space import

- Arc and Zen sidebars become Search spaces and pins: spaces mapped by name or created; profiles created if needed; favourites/Essentials become shared pins; pinned pages become rows when pinned rows are on, otherwise asleep tabs; folders become pin folders. Arc's session duplicates its pins, so importing Arc spaces suppresses the "open tabs" switch automatically.
- Zen has three historical storage forms: `zen-sessions.jsonlz4` + backups (160 MB unpack cap), `zen_workspaces`/`zen_pins` tables in `places.sqlite`, and old session + `zen-workspaces.json`. Max 5000 pins, 200 spaces, 64 folder depth; split views skipped; live folders not brought.
- A generic session import (JSON) can bring tabs, groups, pins and splits; groups made by name and folded state; tabs restored asleep.
- Imported tabs are ordinary tabs; nothing is auto-selected; the import doesn't overwrite existing pins or rows.

---

## 16. Sync and Handoff

### 16.1 Folder sync (○ `sync.folder`)

- A folder the user chooses (iCloud Drive, Dropbox, a network share); `<folder>/Search Sync/<scope>/<device>.json`, one file per Mac per scope (`app`, `profile-<uuid>`), written only by that Mac. A per-device UUID lives in `sync.device`; this Mac's last-agreed map is `sync-<scope>.json` in its own folder.
- Syncs: spaces (name/colour/icon/order/gradient/strength/sharesSignIns — not cookies/tabs/downloads folder/profile), pins (per-space and shared; letter/home/name/listed — **not** pin folder), bookmarks (tree by id and child order), and 17 settings: look, glyph, engine, custom engine, engine suggestions, keywords, spaces, pinned rows, pins-go-home, tab lifetime, new-tab place, tab shortcut numbers, shield, click IDs, GPC, bookmarks bar, quit confirmation.
- Never syncs: history, passwords, cookies/site data, open unpinned tabs, tab groups, private windows/tabs, extensions, downloads folder, window sizes, pin folders, profiles/profile names.
- Merge: per-item `v`/tombstone, `t` stamp, `d` device; larger stamp wins, then device id, then tombstone, then larger text; deletions kept 365 days; unknown/foreign items carried untouched. A "whole kind vanished at once" guard restores from the folder rather than accepting a bad reset. Ties on join go to the folder's existing values.
- Timing: 1 s after turn-on, 2 s after a change (debounced), every 30 s, on app activation, and when a scope folder changes. A missing folder is not treated as mass deletion. iCloud Drive placeholder files are requested and skipped until available.
- Each profile syncs separately; another profile syncs only with a profile of the **same UUID**; separately created profiles stay apart.
- No account, no server; the user is responsible for the shared folder.
- Not compatible with iCloud sync at the same time.

### 16.2 iCloud sync (○ `icloud.sync`)

- CloudKit container `iCloud.com.officecommun.search`, private database, one zone `Search`, one record type `Item`.
- Records: `device.<uuid>` (one device's open tabs), `pins.<space>`, `spaces`, `bookmarks`, `settings`; JSON in `encryptedValues["data"]`, `at` timestamp; latest write wins per record. Only Personal syncs.
- What syncs: spaces (without downloads folder/profile; `sharesSignIns` preserved for existing spaces), each space's **own** pins (not every-space/shared pins), bookmarks, the settings `engine`, `custom`, `look` only, and one open-tabs list per device (every ordinary window's Personal tabs + parked tabs; no pins, private, bench, internal/non-http(s); deduplicated by URL). History, passwords, cookies/sign-ins, shared pins, shared settings, extensions, downloads never.
- Merge: first fetch must succeed before any write; spaces → pins → bookmarks → settings; pins/bookmarks unioned (iCloud first, ours not already present by id/URL); spaces unioned by id; iCloud's settings win. Latest write wins per record; identical canonical JSON is not echoed; unknown fields ride along; a deleted space is deleted locally as Delete Space does.
- Turning iCloud sync off deletes only this device's `device.<uuid>` record; pins/spaces/bookmarks/settings stay in iCloud. "Delete Search's data from iCloud" deletes the zone for every device, turns sync off locally, and says why; local data stays.
- Other devices' open tabs appear in History → "Tabs on Your Other Devices" and in address suggestions (max 2, min 2 typed chars).
- Requires the iCloud entitlement/provisioning profile; a build without it can't enable this. Mutually exclusive with folder sync.

### 16.3 Handoff (○ `handoff`)

- Offers the page in front to the user's iPhone/iPad/other Macs via `NSUserActivityTypeBrowsingWeb`; comes up in their app switcher/Dock.
- Never a private tab, a space signed out of the others, or a non-web page.
- Pages handed off from another device open in Search when it is the default browser.
- Off unless enabled because the address leaves the Mac (to Apple's devices only).

---

## 17. AI, coding agents, and the bench

### 17.1 AI add-on († dormant in this build; `AI.shipped = false`)

The code is complete and compiles into the app, but `AI.shipped = false` (`AI.swift:13-20`) removes the Settings page, View-menu items, welcome/What's New lines, and engine downloads. `prefs.ai` reads as false even if a test build left it true. Re-enabling is one flag.

**Providers**

| Provider | Kind | Base | Wire | Default model |
|---|---|---|---|---|
| On this Mac (`thisMac`) | local engine | never networked | custom JSON lines | Qwen3 1.7B Q4_K_M |
| Anthropic | cloud | `api.anthropic.com/v1/` | Messages | `claude-haiku-4-5` |
| OpenAI | cloud | `api.openai.com/v1/` | chat completions | `gpt-5.4-mini` |
| Google Gemini | cloud | `generativelanguage.googleapis.com/v1beta/` | `generateContent` | `gemini-3.8-flash` |
| OpenRouter | cloud | `openrouter.ai/api/v1/` | OpenAI-compatible | `google/gemini-3.8-flash` |
| Ollama | local app | `127.0.0.1:11434/v1/` | OpenAI-compatible | list from app |
| LM Studio | local app | `127.0.0.1:1234/v1/` | OpenAI-compatible | list from app |

- Local = thisMac, Ollama, LM Studio. Cloud keys are kept in the data-protection keychain, service `com.officecommun.search.ai`, account `<provider>@<host>`, `WhenUnlockedThisDeviceOnly`, not synchronizable. A source-built/unsigned copy has no access group and keeps no key (Settings says so).
- **On this Mac**: model pinned as Qwen3 1.7B Q4_K_M (1,107,409,472 bytes, SHA-256 pinned), stored under `AI/Models/<sha256>.gguf`, excluded from backups. Engine downloaded from the signed appcast, Developer-ID-checked for `com.officecommun.search.ai-engine`, must have only the App Sandbox entitlement; started suspended with stdin/stdout/stderr and the model as fd 3, validated as a running process, then resumed. 5 idle minutes → stop. First question can take up to 120 s while GPU code compiles. Apple Silicon only; Intel warns.
- **Requests**: one ephemeral URLSession, no cookies/cache/credential storage, redirects never followed, HTTPS + exact host allowlist (loopback HTTP only for local apps), 60 s request / 300 s resource timeout, proxy stamped. SSE streaming supports all three wire formats; 2048-token cap. OpenRouter asks for zero-data-retention providers. Keys are never in URLs, defaults, logs, or error text.
- **OpenRouter sign-in**: OAuth PKCE, callback `officecommun.com/search/ai/callback`, exact tab/host/path/main-frame checks, code <1024 chars, <10 min, verifier only in memory. The callback page is never loaded.
- **Page context**: read in Search's isolated world; main frame only; hidden/aria-hidden/inert/screen-reader-only/transparent/off-screen text excluded; article detection; nav/footer/aside/clutter removed; DOM walk capped 40,000 nodes; title 300 chars; text 24,000 chars (first 2/3 + last 1/3); visible links ≤400; URL stripped of query/fragment/user/password and capped 300; random `page-<16 hex>` fence; prompt labels the page as untrusted data; detects page text addressed to an AI and warns; post-answer "strays" check flags addresses/emails/phones that weren't in the page.
- **Answers**: plain text only (no markdown/links/images), 16,000-char cap, footer "AI — may be wrong · where it ran · model", "Not on the page: …" warning, conversation discarded on close. First use of a cloud provider shows a disclosure card (`ai.told.<provider>`).
- **Private tabs**: only on-device providers (thisMac/Ollama/LM Studio); cloud is refused with a message.
- **Settings**: Use AI on pages; provider picker; thisMac Download/Remove/Cancel/Try Again + progress; local model picker + Look Again; cloud key Save/Forget; OpenRouter Sign In…; per-provider model field; Forget every key.

### 17.2 Coding agents (○ `agents`; "Let coding agents use Search")

- **Socket**: `Application Support/Search/Agent/agent.sock`; folder 0700, owned by the user, not a symlink; socket 0600; peer uid via `getpeereid`; non-blocking; `SO_NOSIGPIPE`. No network listener. One session; another gets `busy`. Client must say `hello` within 5 s. The launcher is identified from the peer PID/parent and the client's audit token, not just PID.
- **Consent**: the window shows "<client> wants to use Search" and "From <program> on this Mac". Only a fresh mouse click on the actual control answers; keyboard cannot allow. "Let it see my open tabs" optional. "Always allow" only when the launcher is allowlisted and safely signed. Don't Allow/Stop silences new questions for 10 s. The question is answered only in the key/front window.
- **Trust**: remembers the launcher program's designated requirement, not the client's name. Only Claude Code (`com.anthropic.claude-code`, team `Q6L2SF6YDW`) is allowlisted; Codex is not because its CLI is unsigned. Grant 30 days, last-use 7 days. Stored in the data-protection keychain (`com.officecommun.search.agents.trusted`). Stop suspends the entry; ticking Always again resumes it. Auto-admitted agents never get tab visibility.
- **Session**: idle at 8 s (tab dot removed); let-go at 300 s (connection ends); 64 MiB outbox cap; 1 MiB/32 request burst cap; ~5 req/s after a burst of 8; answers time out at `min(60, timeout+15)`; page settle ≤30 s; closing the session's window ends it; Stop is rechecked before every page action.
- **Tabs**: agent tabs are opened at the end, never selected, never in your session/history; marked `tab.agent` and `tab.agentWorking`; pages opened from them are adopted. Private tabs are invisible (`stale_page`). Your tabs can be listed/read only if permitted; they never get element handles and cannot be clicked/typed/navigated. Page handles `p1` are session-scoped; element handles `p1.s3.e12` go stale on the next read. Agent tabs live in an off-screen window so WebKit lays them out.
- **Tools**: `list_tabs`, `open_tab`, `close_tab`, `navigate`, `back`, `forward`, `read_page`, `click`, `type`, `press`, `scroll`, `wait_for`, `screenshot`, `disconnect`. Limits: `read_page` 300 nodes default/2000 max, 20k text default/200k max; `type` 10k chars; `wait_for` ≤30 s, no CSS selectors; `screenshot` 200–2560 px wide.
- **Redaction**: password/card/one-time-code fields are detected by type/autocomplete/name/id/aria/placeholder/label; values never leave the page; screenshots hide them and paint black boxes; URL query/fragment values named like pass/pwd/otp/code/token/secret/auth/key/session/cvc/card/pin become `redacted`; failure text is scrubbed. No agent scripts/CSS: only Search's fixed page scripts in its own world. Only http/https to open/navigate; `file://`, Search pages, Settings, extensions refused. Downloads and file choosers refused. No camera/mic/screen/location/notifications; no password-save offer for agent sign-ins. Agent keys go to the page's view, not the app, so browser shortcuts don't fire; ⌘-letter and ⌃-letter are refused.
- **UI**: question rises from the bottom; while allowed, a status line/Stop on the sidebar foot/strip; working tab wears a dot; all agent tabs a hollow ring; Stop also on the working tab row and File → Stop Agent (`⌃⌥⌘.`). No banner over the page after consent. Unified log category `Agent` records verb/page/result, not content.
- `search-mcp` is the client; MCP over stdio, stdlib only, one connection at first call, supports protocol versions 2024-11-05 through 2025-11-25, validates the socket/folder/owner, maps `claude-code`→"Claude Code" and `codex`→"Codex", and treats Stop/no as final.

### 17.3 Bench (○ `bench`; "Let a script drive Search")

- **Socket**: `Application Support/Search/bench.sock`; 0600; same-user check; one JSON object per line, one request per connection, 4 MB max, one answer; default timeout 25 s; optional `"window": N` picks the Nth browser window; `pages on` refuses any verb that opens/brings back a window until `pages off`.
- **Test worlds**: `SEARCH_PROBE` or a `.build/` executable makes a test run; `SEARCH_PROBE=1/test` → world "test"; `SEARCH_PROBE=name` → named world; separate folder, defaults suite, and WebKit store UUIDs. `SEARCH_MEASURE` keeps production WebKit/App Nap behavior; `SEARCH_LINK` injects a launch URL; `SEARCH_FEED` overrides the update feed in a test run.
- **Safety**: the switch writes a keychain mark; a defaults-only "on" is forced off and announced. Most destructive/UI-taking verbs refuse outside `Store.testing`. `onScreen` in any answer is a failed run for `./bench`/`bench-mcp`; `pages on` is the controlled exception.
- **Verbs** cover tabs/pages (`tabs`, `open`, `go`, `close`, `wait`, `sleep`, `pin`, `select`, `text`, `nowplaying`, `page`, `eval`, `tap`, `click`, `type`, `submit`, `middle`, `linkmenu`, `shot`, `snap`, `pdf`), chrome (`probe`, `press`, `key`, `resize`, `hoversweep`, `foldtime`, `spacetime`, `swipetime`, `edgetime`, `pointer`, `hit`, `field`, `switcher`, `caret`, `tab-type`, `ui`, `menus`, `menu`, `find`, `icons`, `appleoffer`, `fill`), model (`windows`, `news`, `quit`, `alive`, `columnmenu`, `suggest`, `sitesearch`, `bookmark`, `dropdown`, `translate`, `accounts`, `notifications`, `peek`, `pull`, `magnify`, `wheel`, `group`, `tospace`, `window`, `pages`, `picture`, `float`, `rate`, `film`, `little`, `launcher`, `chooser`, `site`, `pinfolder`, `column`, `profile`, `space`, `split`, `sidepanel`, `popups`, `sitestyle`, `extensions`, `state`, `rows`, `mouse`, `pairPicked`, etc.), and settings/updates/sync (`settings`, `icloud`, `update`).
- **`bench-mcp`**: starts this checkout's `build/Search.app` hidden in a test world (never the installed app), one controller per world via an `fcntl` lease, tools `open_session`/`close_session`/`list_tabs`/`open_tab`/`close_tab`/`navigate`/`back`/`forward`/`read_page`/`click`/`type`/`wait_for`/`screenshot`/`evaluate`/`window_state`/`bench`; blocks `quit`/`update`; fails on any `onScreen`.
- The bench is the development safety net: a `.build/` run never touches the real session/settings, and a hidden probe can render windows without putting one on screen.

---

## 18. External links, launcher, and small windows

### 18.1 Default browser and link intake

- `http`/`https` URL types plus HTML/XHTML document types (XHTML is required for the Desktop & Dock default-browser list); file types: PDF, PNG/JPEG/GIF/HEIC/WebP/TIFF, plain text, JSON, Safari web archives, HTML/XHTML — not SVG (a script risk). Appears as an Alternate handler for the document types.
- Default-browser check asks macOS what opens `https://example.com`; setting it calls `NSWorkspace.setDefaultApplication` for http/https and lets macOS confirm. Default email is separate (`mailto`).
- Links arrive as Apple Events (`kInternetEventClass`/`kAEGetURL`) before or after launch; they wait for the first window. The first URL goes into the blank tab; later ones arrive 0.15 s apart; a routed space opens in front. Test runs refuse outside links.
- **Link routing order** (`Links.little`): (1) a `SpaceRoute` match opens in that space; (2) with small links + find-tabs, an already-open page comes forward; (3) with `links.ask` and >1 space, show the chooser; (4) with small links on, open a small window; (5) otherwise deliver into the browser.
- **Chooser panel**: every space of every profile, profile headings when several; digits 1–9 then 0 pick the first ten rows (keypad digits too, read by keyCode not layout); Return opens the marked row; ↑/↓ moves; Space toggles "Always"; Esc/⌘W cancels; click outside cancels. Links arriving while it is open queue and are asked one at a time. Title "Open a link from <app>". It remembers the last space per site (up to 200). "Always open <site> links here" creates a SpaceRoute only if `sendsLinksToSpaces` is on. A vanished space says so and keeps the link. On cancel it returns to the sender app; if Search was hidden and alone, it hides again.
- **Webmail** (`mailto`): None (system mail), Gmail, Outlook.com, Fastmail, Proton Mail. `mailto:` is parsed as RFC 6068 (to/cc/bcc/subject/body; 50 recipients each, 500-char subject, 8,000-char URL; body shortened to fit). A mailto in a page opens the webmail compose in a foreground tab beside it (never a private tab, even from one; popups go to a browser window). A mailto from another app follows normal link routing. With no webmail chosen, the link goes to the next mail app (system default, then Mail, then first other), never back to Search. Send Feedback uses the webmail compose when set; it only creates a draft.
- Files opened from Finder or another app open in a tab; `file://` is allowed as a URL but not as a page that can script its folder.

### 18.2 Launcher (○ `launcher.on`)

- Global hotkey via Carbon `RegisterEventHotKey` (signature `SrcL`), no Accessibility/global event monitor. No key until chosen; Settings offers Spotlight's key with Shift, i.e. **⇧⌘Space**.
- Registered only while the launcher is on, a key is chosen, shortcut recording is off, and `wants` is true. `wants` = the launcher is up, or Search is not active with a window showing. With a Search window active, the key belongs to the page; `⌘L` is the field there.
- macOS symbolic hotkeys are rejected; another app's Carbon key can't be distinguished. The key is dropped when not wanted, returning it to the app in front.
- Panel: borderless, nonactivating, can become key but not main; floating/modal level; joins all Spaces, fullScreenAuxiliary, transient, ignores cycle, no hide-on-deactivate, clear background, no shadow, immovable. Appears on the screen under the pointer, top at 18% down, horizontally centered, above other apps and the floating video.
- It takes keys without making Search active or bringing it forward; Escape/⌘W/click outside/the key again leaves the other app in front. No search-engine network query while typing; uses the target window's bookmarks/history and open pages (up to 3, same profile, no private/bench tabs, front window first). A typed command stays on top.
- Return order: chosen open tab → command → chosen offer/typed+completion/`destination(for:)` → internal `search://` page in its own tab → small window if `launcher.little` → existing tab with the page → `Links.land`. It always brings Search forward and deminiaturizes. A dismissed draft is kept for next time.
- Key recording: click the box and press; Escape cancels, Backspace/Delete clears; requires ⌘/⌥/⌃ or a function key; rejects reserved macOS/app combos and active symbolic hotkeys; stores `KeyCombo` + keyCode in `launcher.key`; all Search shortcuts stand aside while recording.

### 18.3 Small windows / "Little" (○ `links.little`, default on for new installs only)

- A link from another app opens a small window: the page, a thin 34 pt line with the site, "Open in Search", and close; no tabs, no browser chrome. Default 900×640, min 420×320, transparent hidden titlebar.
- New small windows cascade down/right from the newest visible one. The browser window stays where it was (closed, in the Dock, or behind another app) until "Open in Search" asks for it.
- `⌘O` / Open in Search moves the page into the browser row after the current tab, never among pins; nothing reloads; the small window closes; the browser window comes forward.
- `Esc` closes a link window; `⌘W` closes; `Esc` does **not** close a lifted tab (it is the user's own tab). Closing a link window discards its tab; closing a lifted tab counts as closing that tab and `⇧⌘T` restores it. Lifted tabs are written back into their original space at quit; if their window closes, another window of the profile adopts them.
- A tab can be moved into a small window (tab menu → Open in Small Window); `⇧⌘T` brings it back.
- Frame stored in `little.frame`; restored only if a screen still shows >60% of it; not stored while full screen or merely cascaded. A chosen-space link opens a window showing that space; if none, it asks again or doesn't keep.
- `⌃Tab` shows small windows as "moons" beside the tab grid; picking one brings its window forward. Cards can close/reload/mute/keep.
- Page-first keys: `⌘[`/`⌘]` go to the page first if it has a caret/editor, and only fall back to little-window Back/Forward if WebKit returns them.
- A page's own `window.open` can become a small window if sized (`tabs.popupWindows`); popups reuse `LittleView` but have no "Open in Search"; sign-in pop-ups stay attached to their opener.

---

## 19. Floating video and picture-in-picture

- **Search's own floating window** (default `float.style=search`): a small always-on-top panel that holds the actual page reparented out of the tab; everything but the video is hidden in the page and the video is stretched to the viewport. The page keeps playing; the tab's view is moved, not copied.
- **Triggers**: `⇧⌘P`; leaving the tab (`float.leave`, ●); leaving the app (`float.away`, ○); a playing video on a video site when you switch tabs; the site card can disable floating per site.
- **Controls**: play/pause, skip ±10 s, progress line, close, minimize (`float.minimize`), return to tab (click it), corner flick with two fingers (`float.flicks`, ●), drag anywhere, always on top of other apps, stays over full-screen apps.
- **Return**: going back to the tab or closing the floating window puts the page back where it was; the video does not stop.
- **macOS PiP** (`float.style=system`): hands the video to WebKit's system PiP; requires a user gesture and a private WebKit preference (`_setAllowsPictureInPictureMediaPlayback:`); system controls/corners/spaces.
- **Private tabs**: floating works but the page is the private tab's; no other profile sees it. Unlike AI, agents, Handoff, and notifications, the float path has **no `tab.shy` guard** — a private tab's playing video can float if the other conditions hold. Decide this deliberately in a rewrite.
- **Space/multi-window**: a floating video belongs to its tab; switching spaces parks the tab (still playing); the float stays until closed or the tab closes.

---

## 20. macOS integrations (the porting surface)

| Framework / API | Used for | Rewrite replacement |
|---|---|---|
| WebKit (`WKWebView`, `WKWebExtension`, `WKContentRuleList`, `WKUserScript`, `WKDownload`, `WKWebsiteDataStore`, `WKContentWorld`) | Entire page engine, extensions, blocking, data stores, downloads, isolated scripts | Per-OS webview + new engine abstraction; no cross-platform equivalent of `WKWebExtension` |
| AppKit (`NSWindow`, `NSPanel`, `NSMenu`, `NSPasteboard`, `NSImage`, `NSEvent`, `NSScreen`, `NSWorkspace`, `NSAlert`, `NSSavePanel`, `NSOpenPanel`, `NSPrintOperation`) | Windows, floating panels, menus, clipboard, images, events, screens, share/open, sheets, print | gpui/winit + platform crates; native menu/dialog/print per OS |
| SwiftUI | All chrome, panels, settings, animations | gpui (or HTML/CSS if Tauri) |
| Security (`SecItem`, `SecTask`, keychain) | Passwords, AI keys, agent/bench consent, update signing | Secret Service/libsecret (Linux), Credential Manager/DPAPI (Windows), Keychain (macOS) |
| LocalAuthentication | Touch ID before showing/copying a password | Windows Hello, polkit/fprintd/PAM on Linux |
| AuthenticationServices | Passkeys / WebAuthn sheet, Apple sign-in | Platform WebAuthn (Windows Hello; Linux lacks it in WebKitGTK) |
| CloudKit | iCloud sync | No cross-platform equivalent; use own server or folder sync |
| AppleScript / `Search.sdef` | Read-only tabs/space/profile | D-Bus (Linux), COM/automation (Windows), or drop |
| UserNotifications | Web notifications | libnotify/DBus, Windows toast |
| Contacts | Contact-card form fill | OS contacts APIs or drop |
| CoreLocation | Site location permission | Geolocation per OS |
| Carbon hot keys | Global launcher hotkey | X11/Wayland global shortcuts, Win32 `RegisterHotKey` |
| IOKit (`pwr_mgt`, `ps`) | Power assertions, 120 Hz on power | per-OS power APIs |
| SystemConfiguration / Network | Proxy configuration | per-OS proxy settings |
| ImageIO / CoreGraphics | Favicons, snapshots, PDF | image crate/Skia/CEF |
| CryptoKit / CommonCrypto / SecurityInterface | Hashes, CRX/password decryption, certificate UI | RustCrypto/OpenSSL; per-OS certificate UI |
| NaturalLanguage / Translation | On-device language detection/translation | On-device ML per OS, or drop |
| Compression / SQLite3 | Import archives and browser databases | Portable Rust crates |
| CloudKit/iCloud entitlement, passkey entitlement, App Sandbox | Distribution and restricted capabilities | Per-OS packaging/sandbox; passkeys need browser entitlement on macOS |

Other macOS-only behaviour: default browser/email registration, Handoff, the share sheet (no Services provider or login item exists), Dock menu (New Window), menu bar, Traffic lights, window drag/zoom, full-screen spaces, screen recording permission, Picture-in-Picture, Apple Passwords helper, iCloud Tabs, AppleScript automation, app sandbox, notarization, Sparkle-less self-update, `NSPanel` floating windows, `NSHostingView` bridges, `WKWebExtension` engine.

### 20.1 Packaging, signing, and update

- **Builds**: `./build.sh` → `build/Search.app` (ad-hoc unless a Developer ID exists); `release dmg` → DMG + ZIP + signed appcast; `release ship` → notarized/stapled. `SEARCH_ARCH=x86_64` builds Intel with a separate feed path; no universal binary. `SEARCH_DEV=1` makes a yellow-icon dev build named for the branch/feature and refuses to DMG. Build number is a timestamp; version from `VERSION`; minimum macOS 14.0; `-Osize`; dSYM kept.
- **Signing**: Developer ID + hardened runtime + timestamp; passkeys require `Search.provisionprofile` and `Search.passkeys.entitlements` (ship refuses without it unless `SEARCH_ALLOW_NO_PASSKEYS`); iCloud requires a profile carrying the container and uses `Search.icloud.entitlements`; entitlements are reduced to what the profile grants. DMG/ZIP notarized; DMG stapled.
- **Appcast**: JSON with version, build, ZIP/DMG URLs, SHA-256s, first paragraph of NOTES, minimumSystemVersion, optional AI engine metadata. The JSON itself is signed under identifier `com.officecommun.search.appcast` and delivered as `appcast.json.zip`; updater verifies Team `7BYKA895MC`, the feed identifier, HTTPS, and same-host URLs.
- **Update checks**: launch + hourly (50-minute guard); newer build number only; releases requiring a newer macOS ignored. Auto-install default on (`update.install` unset/true); off stages "Install Update" in the Search menu.
- **Install**: download ZIP → SHA-256 → `ditto` unpack → bundle id/build/code-signature checks for all architectures/nested code → Developer ID requirement → rename current to `.old`, new into place; restore `.old` if the second rename fails. **No automatic relaunch**; "Restart to Update" starts a shell that waits for exit then opens the bundle; `.old` swept at quit/next launch. If the app can't be swapped, "Download Update…" fetches the DMG, checks its SHA/signature, and opens it. No rollback UI.
- **Privacy**: updates touch only the bundle, never Application Support/defaults/keychain/session/history/passwords.
- **Build-time flags**: `SEARCH_DEV`, `SEARCH_FEATURE`, `SEARCH_ARCH`, `SEARCH_SIGN_IDENTITY`, `SEARCH_NOTARY_PROFILE`, `SEARCH_DOWNLOAD_URL`, `SEARCH_AI_ENGINE_VERSION`, `SEARCH_ALLOW_NO_PASSKEYS`. **Runtime**: `SEARCH_PROBE` (test world), `SEARCH_MEASURE`, `SEARCH_LINK`, `SEARCH_FEED` (test only), `--world` for MCP clients.

---

## 21. Settings inventory

Settings is itself a `search://settings/<page>` tab, with a search field (`⌘F` on that tab) that filters every row and its synonyms. Pages: **General, Appearance, Tabs, Browsing, Spaces & Pins, Passwords, Privacy, AI, Extensions, Keyboard & Mouse, About**. Rows hidden by a parent switch are dimmed in search results rather than removed.

### General

| Setting | Key | Default |
|---|---|---|
| Open links from other apps (default browser) | system | off until chosen |
| Email links open in (webmail) | `mail.webmail` | None (system mail) |
| Make Search open email links | system | off |
| Bring things over | — | — |
| Sync with your other Macs (folder) | `sync.folder` | off |
| Handoff to iPhone/iPad | `handoff` | off |
| Sync with iCloud | `icloud.sync` | off |
| Confirm before quitting | `quit.confirm` | off |
| Save downloads to | `downloads` | `~/Downloads` |
| Ask where to save each file | `downloads.ask` | off |
| Always show downloads button | `downloads.button` | off |
| Open Downloads in | `downloads.open` | tab |

### Appearance

| Setting | Key | Default |
|---|---|---|
| Appearance | `look` | system |
| Darken pages that stay light | `pages.dusk` | off |
| Chrome colours in Web Inspector | `inspector.chrome` | off |
| Keep Web Inspector dark | `inspector.dark` | off |
| Tabs layout (Top/Left/Right/Icons only) | `sidebar`, `sidebar.position`, `sidebar.icons` | Left for new installs; Top for old |
| Hide column until edge | `sidebar.hides` | off |
| Slide column in/out | motion key | off |
| `⌘S` makes column icons only | `sidebar.foldIcons` | off |
| Page as a card | `layout.cards` | on |
| Keep card in full screen | `layout.cardsFullScreen` | off |
| Back/forward/reload on the left | `toolbar.left` | off |
| Shorter tab bar | `tabs.compact` | off |
| Always show address bar | `address.shown` | off |
| Address width | `address.width` | Small (220 pt) |
| Glass | `window.glass` | off |
| Glass style | `window.glass.style` | Frosted |
| Transparency / Grain | `window.glass.clear`, `.grain` | 0.5 / 0.2 |
| Liquid Glass tab | `tabs.liquid` | off (macOS 26) |
| Roomier tabs | `sidebar.roomy` | off |
| Buttons at the top | `sidebar.doorsUp` | off |
| What's playing in other tabs | `sidebar.player` | off |
| Tabs show letters/icons | `glyph` | letters |
| High contrast text | `tabs.highContrast` | off |
| Show how far you've read | `tabs.reading` | off |
| Tabs give when pressed | `tabs.give` | off |
| Show bookmarks bar | `bookmarks.bar` | off |
| Save power in Low Power Mode | motion key | off |
| Reduce animations | motion key | off |

### Tabs

| Setting | Key | Default |
|---|---|---|
| New tab position (Next/Top/End) | `tabs.newPlace` | Next |
| Links opened in front go beside their page | `tabs.frontBeside` | off |
| Leave a new tab when the last closes | `tabs.newAfterLast` | off |
| Closing a tab goes back | `tabs.closeGoesBack` | off |
| `⌘Z` reopens a closed tab | `tabs.undoReopens` | off |
| `⌃Tab` goes to a link opened behind | `tabs.switcherBehindFirst` | off |
| `⌃Tab` leaves out pins | `tabs.switcherLeavesOutPins` | off |
| Load background tabs when you go to them | `tabs.lazy` | off |
| Open sized pop-ups in small windows | `tabs.popupWindows` | off |
| Sleep tabs (Never/Later/Normal/Sooner) | `tabs.sleep`, `tabs.sleep.pace` | Normal, 30 min |
| Dim sleeping tabs | `tabs.sleep.dim` | off |
| Close tabs left alone (12 h/1 d/1 w/1 m) | `tabs.close.after` | Never |
| Show tab numbers while `⌘` held | `tabs.shortcuts` | off |
| Show address on the active tab | `tabs.address` | on |
| Same tabs in every window | `tabs.mirror` | off |
| Profiles (list) | — | one |
| Profiles in one window | `profiles.oneWindow` | off |

### Browsing

| Setting | Key | Default |
|---|---|---|
| Search with | `search.engine`, `search.custom` | Google |
| Shift-Return searches with | `search.shift`, `search.shiftCustom` | off |
| Suggestions from search engine | `search.suggest` | off |
| Site shortcuts | `search.keywords` | empty |
| Open links from other apps in a small window | `links.little` | on for new installs |
| Go to a tab that has the page | `links.little.tabs` | off |
| Read the site's own Markdown | `reader.twins` | off |
| Reading mode follows light/dark | `reader.follows` | off |
| Floating video: Search/PiP | `float.style` | Search |
| Float video on its own | `float.leave`, `float.away` | on / off |
| Minimize floating video | `float.minimize` | off |
| Videos wait for a click | `media.click` | off |
| Esc twice to leave full screen | `fullscreen.escapeTwice` | off |
| Keep window in use during full screen | `fullscreen.keepsWindow` | off |
| Translation language | `translate.target` | Mac's language |
| Page zoom | `pageZoom` | 100% |
| Capture pages | `pages.capture` | off |
| Your own style for a site | `sites.styles` | off |
| Pages at 120 Hz (Never/On power/Always) | `pages.120.mode` | On power |

### Spaces & Pins

| Setting | Key | Default |
|---|---|---|
| Set Up Like Arc | — | — |
| Bring Arc's spaces and pins over | — | — |
| Spaces | `spaces` | off |
| `⌘1–⌘9` switch spaces | `spaces.commandNumbers` | off |
| Profiles (list) | — | — |
| Profiles in one window | `profiles.oneWindow` | off |
| Send links to a space | `spaces.links` | off |
| Ask which space for links | `links.ask` | off |
| Pins keep their site | `pins.home`, `pins.peek` | on |
| Pinned rows | `pins.list` | off |
| Folders in pinned rows | `pins.folders` | off |
| `⌘`-click on a pin opens a new tab | `pins.commandNewTab` | off |
| New tab in a floating field | `newtab.floating` | off |
| Recent places in the new tab | `newtab.recent` | off |
| Address bar commands | `commandbar` | off |
| Start with a fresh window | `start.fresh` | off |

### Passwords

| Setting | Key | Default |
|---|---|---|
| Offer to save passwords | `passwords.save` | on |
| Fill in sign-ins | `passwords.fill` | on |
| Offer passkeys | `passkeys` | on when entitled |
| Passwords from Apple's Passwords app | `passwords.apple` | off |
| Fill in your contact details | `contacts.fill` | off |

### Privacy

| Setting | Key | Default |
|---|---|---|
| Block ads and trackers | `shield` | on |
| Prevent cross-site tracking | `sites.keep` (inverted) | on |
| Remove click trackers | `privacy.clickIDs` | on |
| Global Privacy Control | `privacy.gpc` | on |
| Let sites ask to send notifications | `notifications.ask` | on |
| Site permissions / Forget choices | `capture.*` | per site |
| Clear history / cookies / cache | — | — |
| Proxy (HTTP/SOCKS5, optional user/password) | `network.proxy` | off; password in keychain |

### Keyboard & Mouse

| Setting | Key | Default |
|---|---|---|
| Peek at a link | `links.peek`, `links.peekClick` | Shift-click |
| Peek stays with its tab | `links.peek.stays` | off |
| `⌥`-click a link opens Split View | `links.optionSplit` | off |
| Scroll with middle button | `autoscroll` | off |
| Bounce at top/bottom | `pages.bounce` | off |
| Hold a swipe to pick from history | `swipe.history` | on |
| Open address field from any app | `launcher.on`, `launcher.key` | off |
| Key | `launcher.key` | unset |
| Open in a small window | `launcher.little` | off |

### About

| Setting | Key | Default |
|---|---|---|
| Updates / Check now | — | hourly |
| Install updates on its own | update key | on |
| Found something wrong? | — | — |
| What's new | — | — |
| Let a script drive Search | `bench` | off |
| Let coding agents use Search | `agents` | off |

### Always-on notes (no switch, findable in search)

Split View; tab groups; websites in the side panel; Translate Page; where an extension runs; keep a site awake; search a site from the field; a space's colour/gradient; pins in every space; a pin's number again; keys in the tab switcher; show where links go; flick the floating video.

---

## 22. Keyboard shortcuts

All commands below are user-rebindable in Settings → Shortcuts (per app, not per window), except the macOS-owned keys (`⌃⌘F` full screen, `⌘Q`, `⌘,`? — `⌘,` is rebindable here, and the standard text-editing keys).

### App / File / Edit

| Key | Action |
|---|---|
| `⌘,` | Settings |
| `⌥⌘L` | Passwords… |
| `⌘N` | New window |
| `⌘T` | New tab (floating field if enabled) |
| `⇧⌘N` | New private tab |
| `⇧⌘T` | Reopen closed tab/window |
| `⌘L` | Open Address (field) |
| `⌘W` | Close tab / field / panel |
| `⇧⌘W` | Close window |
| `⌘P` | Print |
| `⇧⌘C` | Copy Clean Link |
| `⌘F` | Find on page (Settings search on the Settings tab) |
| `⌘G` / `⇧⌘G` | Find next / previous |
| `⌃⌥⌘.` | Stop Agent |
| `⌘Q` | Quit (with optional confirmation) |

### View

| Key | Action |
|---|---|
| `⇧⌘S` | Show tabs in sidebar |
| `⌘S` | Hide sidebar/tab bar (or fold to icons) |
| `⌥⌘[` / `⌥⌘]` | Fold / unfold all groups |
| `⌘R` | Reload |
| `⌥⌘R` | Reload from origin |
| `⇧⌘R` | Reading mode |
| `⇧⌘P` | Float video |
| `⇧⌘D` | Darken this site |
| `⌥⇧⌘D` | Pause darkening everywhere |
| `⇧⌘H` | Hide Elements |
| `⇧⌘U` | Hidden on this site |
| `⌘+` / `⌘−` / `⌘0` | Zoom in / out / actual size |
| `⌥⌘I` | Web Inspector |
| `⌥⌘J` | JavaScript console |
| `⌥⌘C` | Inspect element |
| `⌃⌘F` | Enter/exit full screen (macOS key) |

### Tabs

| Key | Action |
|---|---|
| `⌘[` / `⌘]` | Back / forward |
| `⌘←` / `⌘→` | Back / forward (when the caret is not in an editor) |
| `⇧⌘]` / `⇧⌘[` | Next / previous tab |
| `⌘K` | Search tabs… |
| `⌥⌘N` | Split current page / separate split |
| `⌃⌘←` / `⌃⌘→` | Focus left / right page |
| `⌘D` | Duplicate tab |
| `⇧⌘C` | Copy address |
| `⇧⌘V` | Paste and go |
| `⇧⌘M` | Stop sound in tab |
| `⌃Tab` | Recent-tab switcher (rebindable) |
| `⌃W` / `⌃R` / `⌃M` / `⌃O` | While `⌃Tab` is up: close / reload / mute / keep |
| `⌘1`–`⌘9` | Tab 1–9 (or spaces, if swapped) |

### Bookmarks / History

| Key | Action |
|---|---|
| `⇧⌘B` | Add this page |
| `⌘Y` | Show History |
| `⇧⌘J` | Downloads |
| `⇧⌘⌫` | Clear browsing data |

### Spaces

| Key | Action |
|---|---|
| `⌃1`–`⌃9` | Space 1–9 (or tabs, if swapped) |
| `⌘1`–`⌘9` | Spaces, when `spaces.commandNumbers` is on |

### Page editing (inside pages)

`Tab` stays the page's for form traversal. Standard editing keys work (⌘C/V/X/A/Z, ⌥⌫ word delete, ⌘⌫ line delete). Text replacements, smart quotes/dashes, and spelling correction follow the Mac or the Edit menu. In a page, `Esc` closes the find bar, peek, site card, field, or full screen (twice if configured).

---

## 23. Mouse and trackpad gestures

| Gesture | Where | Behaviour |
|---|---|---|
| Two-finger horizontal swipe | Page | Back/forward; pull-in disc; 70 pt to arm, 30 pt flick within 0.25 s; hold 0.45 s while armed for a pick-list of up to 8 pages; 22 pt per row; haptics |
| Two-finger horizontal swipe | Tab column | Switch spaces; 50 pt; rows follow; past the last space creates one |
| Two-finger vertical swipe | Top tab bar | Switch spaces, same rules |
| Mouse wheel notch | Tab bar | One space per notch, with a rest period |
| Mouse back/forward buttons | Tab bar | Previous/next space |
| Pinch | Page | WebKit's own magnification; smart zoom double-tap fills the block |
| Shift-click (default) | Link | Peek panel over the page; `⌘↩` keep as tab; `⌥⌘↩` keep beside; `Esc` close |
| ⌥-click | Link | Open in Split View (if `links.optionSplit`) |
| ⌘-click | Link | Open in a new tab behind |
| ⇧⌘-click | Link | Open in a new tab in front |
| Middle click | Link | Open in a background tab; ⇧ foreground |
| Middle click | Tab | Close tab |
| Middle click | Page | Auto-scroll mark (if `autoscroll`); move pointer to scroll; click/Esc stops |
| Click | Active tab | Site card |
| Double-click | Pinned square | Go home / edit letter |
| Double-click | Empty tab strip | New tab |
| Double-click | Window title area | Zoom/fill |
| Drag | Tab | Reorder, pin/unpin, group, folder, split, new window, small window |
| Drag | Page edge | Split preview; 120 pt/24% band; 6 pt/90 ms settle; 20 pt slack |
| Drag | Sidebar edge | Resize column (176–440 pt), written on release |
| Drag | Side panel edge | Resize panel (300–640 pt, default 380; double-click resets) |
| Drag | Split divider | Resize panes (minimum 20% each for two, 10% for 3–4) |
| Two-finger flick | Floating video | Snap to the corner pointed at |
| Pointer at window edge | Folded sidebar | Peek out; 6 pt zone, 48 pt overshoot, 65 ms grace, 150 ms dwell |
| Hover | Link | Status line shows destination |
| Hover | Tab | Close cross / pin minus appears; tab gives if enabled |
| `⌘` held | Tabs | Shortcut numbers appear |

---

## 24. Context menus (inventory)

### Tab (any tab, any layout)

Move to Group → New Group / [groups] / Remove from Group; Pin / Pin as Row; pin-specific: Show as Square/Row, New Folder, New Live Folder…, Move to Folder, Remove from Folder, Back to Pinned Page / Change Letter, Replace Pinned Page with Current URL, Keep in Every Space / Only in This Space, Unpin, Delete; Move to Space → [spaces] / New Space…; Always/Stop Opening <site> in <space>; Split View → Open in Split View / Add to Split View / Swap Pages / Remove from Split View / Separate Split Tabs / Close Both Pages; Move to Window → New Window / [windows]; Open in Small Window; Rename; Duplicate; Site Information…; Copy Address; Copy as Markdown Link; Copy Clean Link; Open in Side Panel; Mute/Unmute; Put to Sleep; Close Tab / Close N Tabs; Close Other Tabs; Close Tabs Below/Right; Reopen Closed Tab.

### Pin

Same as tab pin-specific items, plus shared-pin and folder actions; a pin that has wandered gets Back to Pinned Page; a square gets Change Letter.

### Group heading

Rename Group; Collapse/Expand Group; Colour…; Move Group to Space; Ungroup Tabs; Close Group.

### Pin folder heading

Rename Folder; Expand/Collapse Folder; Refresh (live); New Folder Inside; New Live Folder Inside…; Move Folder To; Delete Folder.

### Space dot/menu

Every space (with number, icon, tick); New Space…; Profile submenu; Rename; Icon; Colour…; Move Left/Right; Fold/Unfold All Groups; Downloads folder (Same as Settings / own folder / Choose Folder…); Delete (except first/home).

### Column empty area

New Tab; New Folder / New Live Folder… (with pin folders); Paste and Go (if clipboard is a URL); Hide Sidebar; Sidebar Settings….

### Bookmarks (item/folder)

Open; Show in Folder; Open in New Tab/Window/Split/Private; Rename…; Edit Address…; Copy Link; Remove; folders: Open All in New Tabs/New Window/Tab Group, New Folder Inside…, Rename…, Remove.

### History row

Copy Address; Remove; open; clear options. Downloads rows: Copy Address, Pause/Resume/Retry/Cancel/Remove, Show in Finder.

### Extension button

Pin/Unpin to Toolbar; Options…; Reload; Runs on (All sites / Only sites I name / On click; Always on this site; Run on this site); Remove.

### Image

Open Image in New Tab; Copy Image; Download Image; Copy Image Address.

---

## 25. Data files and where they live

Everything is under `~/Library/Application Support/Search/` (or `Search (<world>)` for a test world):

| File | Holds |
|---|---|
| `session.json` | First space's tabs, active index, groups, splits |
| `session-<spaceUUID>.json` | Every other space's row |
| `windows.json` | Additional windows: frame, space, rows per space, merge/tuck/fold |
| `pins.json` | Per-space pins, shared pins, pin IDs, homes, letters, rows, folders |
| `pin-folders.json` | Pin folder tree, live feeds and their posts |
| `spaces.json` | Spaces: name, colour, icon, gradient, strength, profile, store sharing, downloads folder |
| `profiles.json` | Profiles: name, home space |
| `bookmarks.json` | Bookmark tree, imported-from marks |
| `history.json` / `history-<profile>.json` / `history-space-<space>.json` | Visits, titles, counts, last visits; a signed-out space gets its own |
| `archive.json` / `archive-<profile>.json` | Tabs auto-closed, 30 days/500, ISO-8601 dates |
| `downloads.json` | Download history/rows, capped 50 |
| `hidden.json` | Hidden selectors by site |
| `styles.json` | Per-site CSS |
| `icloud.json` | iCloud sync ledger (record versions, device id, engine state) |
| `sync-<scope>.json` | This Mac's last-agreed folder-sync map (never in the shared folder) |
| `<sync folder>/Search Sync/<scope>/<device>.json` | Folder-sync file per Mac per scope |
| `search-sites.json` | Learned OpenSearch site searches |
| `installed.json` | Extensions (id, name, version, enabled, permissions, pinned, source) |
| `icons/pages.json` + icon files | Favicon index/cache |
| `crash.log` | Uncaught exception lines |
| `agent.sock` / `bench.sock` | Local control sockets |
| `Agent/` | Agent-related state |
| Keychain items | Passwords (`Search` tag), AI keys, agent/bench consent marks, update identity |

UserDefaults keys worth preserving: `space.current` / `space.current.<profileUUID>`, `spaces.orderKept`, `spaces.erasing`, `sync.device`, `icloud.sync`/`icloud.off`, `import.records`, `shield.paused`, `zoom.<host>`, `autoplay.<host>`, `dusk.sites`, `passwords.never`, `capture.<origin>|<kind>`, `extensions.checked`, `extensions.sites.<id>`, `bookmarks.top`, `bookmarks.lastFolder.<file>`, `fileImport` scratch state.

Additional directories: WebKit website data stores (per space/profile, or default), WebKit extension data, favicon cache next to history, `Downloads` for a test world, extension install folders.

Data migration: the app was "Office Browser" until September 2026; its folder and settings are moved once on first run under the new name. A file that fails to decode is quarantined as `*.unreadable-<timestamp>.json` rather than overwritten.

---

## 26. Internal pages (`search://`)

| Address | Page |
|---|---|
| `search://settings` | Settings (detail = section, e.g. `search://settings/privacy`) |
| `search://history` | History |
| `search://downloads` | Downloads |
| `search://bookmarks` | Bookmarks |
| `search://passwords` | Passwords |
| `search://archive` | Archived Tabs |
| `search://import` | Bring Things Over (detail = source) |
| `search://release-notes` | What's New |

One tab of each kind per window; asking again brings the existing tab forward; an empty tab in front is reused; private tabs never become internal pages. They keep their own Back/Forward trail and survive relaunch. They hold no web view while shown. They are never offered to extensions.

---

## 27. Dormant, hidden, and developer-only features

- **AI add-on**: code complete, `AI.shipped = false`; hidden from Settings/menus/welcome; a test build can enable it.
- **Bench**: local socket, test worlds, measurement mode, link injection, UI automation, screenshots. Off unless `bench` is enabled and the launch was explicitly allowed (a keychain mark).
- **Agents**: MCP control, consent prompt, trust marks, agent tabs, redaction, logging.
- **Private WebKit selectors** (fragile; must be replaced or dropped):
  - `_setAllowsPictureInPictureMediaPlayback:` (native PiP)
  - `_setRubberBandingEnabled:` (edge bounce)
  - `_setResourceLoadStatisticsEnabled:` (tracking prevention)
  - `_setCookieAcceptPolicy:completionHandler:` (cookie policy)
  - `_WKProcessTerminationReason`, `_WKRenderingProgressEventFirstVisuallyNonEmptyLayout`, `_WKRectEdge`
  - `set_delegate:` on `WKWebsiteDataStore` (web notifications)
  - `toggleAutomaticSpellingCorrection:` (autocorrect)
  - `setValue:forKey:` `developerExtrasEnabled` (inspector)
- **Test-only**: `SEARCH_PROBE`, `SEARCH_MEASURE`, `SEARCH_LINK`, `SEARCH_FEED`, `SEARCH_ARCH=x86_64`, `SEARCH_DEV=1`, `SEARCH_FEATURE=name`, `SEARCH_ALLOW_NO_PASSKEYS`, `bench` verbs, `Tests/` harnesses.
- **Dead/legacy settings** read or removed: `mind.*` (old assistant), `ai.*` keys, `inspector` (old switch), old `manner`, old `pages.120` boolean, `passkeys.entitled` vs `passkeys.for`.
- **macOS 26-only**: Liquid Glass tab, Liquid Glass glass style; hidden before 26.

---

## 28. Rewrite parity matrix

### Tier A — must exist for the product to be Search

| Feature | Why it is core | Difficulty |
|---|---|---|
| One-field address/search with the parsing rules | The whole interaction | Low |
| Tabs (top/side/icons/fold), drag, pin, split, groups | The chrome | High |
| Session restore, sleep, reopen, crash recovery | Perceived speed/reliability | Medium |
| History, bookmarks, downloads, find, reader, zoom | Every browser | Medium |
| Ad/tracker blocking + hiding elements + per-site off | Product promise | Medium (engine-specific) |
| Spaces/profiles with separate sign-ins | Differentiator | High |
| Floating video | Signature feature | High (webview reparenting) |
| Private tabs, permissions, clear data | Security promise | Medium |
| Passwords (save/fill/autofill) | Table stakes | High (keychain per OS) |
| Import (Chrome/Arc/Safari/Firefox) | Migration | Medium |
| Update mechanism + signing | Shipping | Medium per OS |

### Tier B — strongly expected, can lag

| Feature | Notes |
|---|---|
| Pins as rows/folders/live folders | Arc-style identity; only meaningful with sidebar |
| Tab switcher with previews | Performance-sensitive |
| Reading mode twins/Markdown | Content heuristic is portable |
| Translate (on-device) | macOS-only framework; drop or replace |
| Capture (area/element/page) | WebKit snapshot; per-engine |
| Site style (CSS boosts) | Portable injection |
| Dusk (darken light pages) | Complex twin generation; portable but costly |
| Sync (folder/iCloud) | iCloud is macOS-only; folder sync portable |
| Handoff | macOS-only |
| Passkeys | macOS/Windows; Linux WebKitGTK lacks WebAuthn |
| Apple Passwords | macOS-only, unofficial |
| Small windows / launcher | Portable concept, per-OS panels/hotkeys |
| 120 Hz / glass / Liquid Glass | Per-OS compositor work |

### Tier C — platform-specific or likely to be dropped

- `WKWebExtension`/Chrome extension engine; 14.5K lines; no Linux equivalent; WebView2 has BYO Edge extensions only.
- AppleScript dictionary, Apple Passwords helper, iCloud Tabs/CloudKit, Handoff, Contacts/CoreLocation/Touch ID, Carbon global hotkeys, IOKit power assertions, `NSPanel` specifics, macOS Services/share sheet, system PiP.
- Private WebKit APIs listed in §27.
- The AI add-on until it ships.

### Hard problems specific to this product

1. **Three engines, three capability sets.** Extensions and passkeys define the macOS experience; Linux WebKitGTK has no passkeys and only an embryonic WebExtension API; WebView2 has no store installs and a different extension model. A rewrite must either accept a reduced feature matrix per OS or bundle Chromium (CEF) and lose the 10 MB/no-Chromium identity.
2. **Passwords and passkeys are security surfaces.** An OS keychain abstraction is not enough; autofill origin checks, private-tab isolation, Touch ID/Hello prompts, and page-script isolation all need reimplementation and review.
3. **The floating video reparents a live web view.** This is WKWebView-specific in behavior; on WebView2 a webview cannot be freely reparented without losing media state, and on WebKitGTK it is a GTK widget. A rewrite may need a page-side video extraction + a separate player window instead.
4. **Overlays above web content.** Menus, peeks, site cards, find bars, split covers, and the site card all float over the page. gpui's native-view composition (2026) can do this, but each OS/webview needs testing; on Windows the WebView2 child window sits above gpui unless composition is used.
5. **Input latency and gesture ownership.** The swipe-back system asks the page whether a sideways scroll belongs to it, with a final answer after the fingers lift. This kind of arbitration is engine- and event-model-specific.
6. **Accessibility.** Page accessibility comes from the webview; chrome accessibility must be built per toolkit. gpui's AccessKit support is young; a browser needs both.
7. **The test harness.** `Bench.swift` is 5,891 lines of AppKit/WebKit automation; a rewrite needs an equivalent, or it will regress silently.

---

## 29. Quirks, inconsistencies, and rewrite warnings

- The README claims ~42,000 lines; the tree is 107,058. It also says "about 10 MB" — accurate for the release, but a cross-platform build with WebView2/WebKitGTK will vary.
- `Omnibox.swift`'s header says the field "takes addresses and only addresses"; the actual `Browser.destination(for:)` also searches. The comment is stale.
- Several settings are "always on" but still properties for tests (`splitView`, `usesTabGroups`, `sitesInPanel`, `searchesSites`, `keepsSitesAwake`, `sharesPins`, `pinNumberGoesBack`, `switcherKeys`). A rewrite can drop their switches but should keep the behavior.
- "Prevent cross-site tracking" is stored inverted (`sites.keep = true` means tracking prevention off), which is easy to get wrong in a port.
- `passkeys` behavior depends on the build's entitlement, not just the setting; a source-built app cannot offer passkeys.
- Apple Passwords integration uses Apple's Chrome-extension helper id and is explicitly unofficial; it can break at any macOS update.
- `AI.shipped = false`; do not schedule AI parity as if it were visible.
- `README` says "macOS 14+", but Liquid Glass requires macOS 26 and extensions require 15.4; the feature matrix is version-gated.
- The app relies on a number of private WebKit selectors (PiP, rubber-banding, tracking prevention, cookie policy, notifications delegate, spelling correction, inspector). These are not API contracts and are the first things to break on a new macOS.
- The "one window" principle is not absolute: profiles, small windows, popups, floating video, the launcher, and agent tabs all create auxiliary windows/panels.
- Tab groups and pinned rows are separate mental models (groups = sections of ordinary tabs; pinned rows = pins with titles/folders). Arc's folders map to pin folders; its "Today" is dropped.
- Spaces are not profiles and profiles are not spaces: spaces swap tabs in one window; profiles own spaces/cookies/history/bookmarks and windows.
- Auto-close and sleep both count from `touched`/`letGo`; the session file carries these across quits, so "close after a day" works while the app is closed.
- A tab in a group is "put away" on `⌘W` rather than closed; a pinned tab is "put down"; a grouped row closed a second time deletes, while a pin is removed only by Unpin/Delete. This is Arc semantics and must be intentional.
- Drag/drop is not one system: `TabDrag`, `PinDrop`, `GroupDropFrames`, `PinTree`, `LooseFrames`, and `SpaceSwipe` each own a piece. A rewrite should unify them behind one drag session.
- The Stop button's tooltip advertises `⌘.`, but no handler or command exists for it; only the button stops a load. Treat this as a bug to fix or remove in a rewrite.
- There is no home button. "Home" is a pinned tab's double-click / Back to Pinned Page / ⌘-click, not a chrome control.
- The address field shows the raw punycode; the look-alike defence appears on the tab at rest and in the site card, not in the field. A rewrite should not assume the field is the warning surface.
- `⌘S` is fold/hide the sidebar, not Save Page; Save Page has no shortcut.
- `Tab` in the field is suggestion-walking/site-chip, not completion; the grey completion is accepted by Return only. This is easy to get wrong in a port.
- A page's `window.print()` is rate-limited after two cancels in 10 seconds; direct `⌘P` is not.
- A tab that turns out to be a download on its first navigation forgets its address and, if it was opened by a link, closes back to the opener.
- **Spaces are not isolation boundaries for bookmarks.** Bookmarks are per profile even for a signed-out space; only history and cookies split when a space has its own store. A rewrite that assumes per-space bookmarks won't match the files or behavior.
- **Signed-out space ≠ private tab.** A signed-out space persists (cookies, history, pins, sync), while a private tab is ephemeral, unlisted, unsynced, and writes nothing.
- **Deleting a signed-out space erases its own history**, despite the confirmation saying history stays. Decide the intended behavior before porting.
- **Session does not save scroll, Back/Forward, or form state across launches.** Only a sleeping tab's in-memory interaction state or a closed window's in-memory keepsake keeps them; both die with the process. Paused downloads also die with the process.
- **Window/session ownership moves.** The oldest window of a profile owns that profile's session files; other windows live in `windows.json`; when the oldest closes, the files transfer.
- **Pin folders are local and per space**; folder sync does not carry them, and iCloud only carries the field for peers that know it. Shared pins cannot be in folders.
- **Auto-close and sleep share one timer.** A rewrite that separates them must still do the launch-time sweep before the first frame and preserve the one-at-a-time wake/sleep ordering.
- **History is capped at 2,000 on disk and 2,000 + 250 slack in memory**, and has no date-range deletion.
- **Downloads have no Notification Center notifications**, and private downloads are deliberately unlisted.
- **Profiles in one window do not change a window's profile.** They swap whole profile windows in place; closing the visible one closes the merged set.
- **Floating video has no private-tab guard**, unlike AI/agents/Handoff/notifications; passkeys also have no private-tab check. Both are deliberate-looking gaps worth deciding before a rewrite.
- **The Stop tooltip advertises `⌘.` with no handler**; the address-field `Tab` key walks suggestions rather than accepting completion; the README's line count is stale. These are the kind of small mismatches a rewrite will inherit if it copies behavior from docs instead of code.

---

## Appendix A — Source map

| Area | Files |
|---|---|
| App shell, windows, menus | `App.swift`, `Windows.swift`, `OneWindow.swift`, `Links.swift`, `Little.swift`, `Welcome.swift`, `WhatsNew.swift` |
| Tab model and engine glue | `Tab.swift`, `Browser.swift`, `Stage.swift`, `PaneStage.swift`, `SplitView.swift`, `Split.swift` |
| Chrome | `Side.swift`, `SideIcons.swift`, `TabBar.swift`, `TabDrag.swift`, `TabLift.swift`, `PinDrop.swift`, `TabGroups.swift`, `Fold.swift`, `TabSwitcher.swift`, `LiquidTab.swift`, `Glass.swift`, `Design.swift`, `AtRest.swift`, `StatusLine.swift`, `AddressLine.swift` |
| Navigation, field, engine features | `Omnibox.swift`, `Address.swift`, `AddressCommands.swift`, `Engine.swift`, `EngineSuggest.swift`, `Keyword.swift`, `SiteSearch.swift`, `Find.swift`, `PageFind.swift`, `Reader.swift`, `Markdown.swift`, `Snap.swift`, `Dusk.swift`, `DuskColours.swift`, `SiteStyles.swift`, `Shield.swift`, `Privacy.swift`, `YouTubeAds.swift`, `Translate.swift`, `Inspector.swift`, `AutoScroll.swift`, `MediaKeys.swift`, `Swipe.swift` |
| Spaces, profiles, pins | `Spaces.swift`, `SpaceSwipe.swift`, `SpaceRoutes.swift`, `SpaceGradient.swift`, `Profiles.swift`, `Pins.swift`, `PinFolders.swift`, `BrowserPinFolders.swift`, `PinFolderViews.swift`, `LiveFolders.swift`, `TabGroup.swift` |
| History, bookmarks, downloads, archive | `History.swift`, `Bookmarks.swift`, `BookmarksFile.swift`, `BookmarksBar.swift`, `Recall.swift`, `Fetching.swift`, `DownloadsFloat.swift`, `Loot.swift`, `Archive.swift`, `Session.swift`, `Closing.swift`, `Sleep.swift`, `Twins.swift` |
| Passwords, passkeys, forms | `Vault.swift`, `Passwords.swift`, `Passkeys.swift`, `ApplePasswords.swift`, `ApplePasswordsProtocol.swift`, `ApplePasswordsOffer.swift`, `ApplePasswordsSettings.swift`, `Forms.swift`, `MeCard.swift` |
| Extensions | `Extensions.swift`, `ExtensionsUI.swift`, `ExtensionShims.swift`, `ExtensionNative.swift`, `ExtensionFetch.swift`, `ExtensionFiles.swift`, `ExtensionOffscreen.swift`, `ExtensionPopup.swift`, `ExtensionCapture.swift`, `ExtensionSocket.swift`, `ExtensionWatch.swift`, `ExtensionWorkerRecovery.swift`, `ExtensionPageScript.swift`, `ExtensionSiteAccess.swift`, `ExtensionRedirectPolicy.swift`, `ExtensionReturnNavigation.swift`, `ExtensionExternal.swift`, `Crx.swift` |
| Import | `Import.swift`, `ImportArc.swift`, `ImportFile.swift`, `ImportGeckoTabs.swift`, `ImportRecord.swift`, `ImportSession.swift`, `ImportWebKit.swift`, `ImportZen.swift`, `ImportPanel.swift` |
| Sync, update, OS services | `Sync.swift`, `SyncMerge.swift`, `ICloudSync.swift`, `Handoff.swift`, `Scripting.swift`, `Notifications.swift`, `HotKey.swift`, `Launcher.swift`, `MediaKeys.swift`, `Proxy.swift`, `SiteLocation.swift`, `RecordingIndicator.swift`, `Sharing.swift`, `Webmail.swift`, `Updater.swift`, `Substitutions.swift` |
| AI, agents, bench | `AI.swift`, `AIClient.swift`, `AIEngine.swift`, `AIPage.swift`, `AISignIn.swift`, `AIKeys` in `AI.swift`, `AIAssist.swift`, `AgentControl.swift`, `AgentTrust.swift`, `AgentBanner.swift`, `PageQuestions.swift`, `Bench.swift` |
| Settings and help | `Settings.swift`, `SettingsIndex.swift`, `Prefs.swift`, `Shortcuts.swift`, `ShortcutsPage.swift`, `InternalPages.swift`, `Dialogs.swift`, `ImageMenu.swift` |

## Appendix B — How this inventory was produced

- Read the full README, ROADMAP, CONTRIBUTING and NOTES.
- Classified all 183 source files by subsystem and platform import.
- Enumerated every setting (`SettingsIndex.swift`), every command and default key (`Shortcuts.swift`), every persisted file (`Store.file`), every context menu, every injected page script, and every engine delegate.
- Traced each gesture to its implementation and constants (`Swipe.swift`, `Tab.swift`/`PageView`, `SpaceSwipe.swift`, `TabDrag.swift`, `Fold.swift`, `AutoScroll.swift`, `Float.swift`).
- Cross-checked engine behaviour against the WKWebView/WKWebExtension usage and the macOS-only private selectors.
- Where the code and the README disagree, the code wins; stale comments are called out in §29.

*End of inventory.*
