//! The Phase 0 window shell: one window, fake tabs, one field, a page
//! placeholder. No engine is attached yet.

use crate::UiError;
use crate::field::{AddressField, FieldEvent, key_bindings};
use crate::metrics;
use crate::theme::{Palette, SpaceTint, Theme};
use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, KeyBinding, MouseButton, Render,
    SharedString, TitlebarOptions, Window, WindowAppearance, WindowBounds, WindowDecorations,
    WindowOptions, actions, div, point, prelude::*, px, size,
};
use searlx_core::PageUrl;

actions!(
    searlx,
    [NewTab, CloseTab, FocusAddress, NextTab, PreviousTab, Quit]
);

/// The default key bindings for the shell.
#[must_use]
pub fn shell_key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("ctrl-t", NewTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("cmd-l", FocusAddress, None),
        KeyBinding::new("ctrl-l", FocusAddress, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("ctrl-q", Quit, None),
    ]
}

struct TabState {
    title: SharedString,
    address: SharedString,
}

impl TabState {
    fn new(title: impl Into<SharedString>, address: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            address: address.into(),
        }
    }
}

/// The window's root view.
pub struct Shell {
    tabs: Vec<TabState>,
    active: usize,
    field: gpui::Entity<AddressField>,
    focus_handle: FocusHandle,
    palette: Palette,
    theme: Theme,
    tint: SpaceTint,
    status: Option<SharedString>,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("tabs", &self.tabs.len())
            .field("active", &self.active)
            .field("theme", &self.theme)
            .finish_non_exhaustive()
    }
}

impl Shell {
    /// Build the shell.
    pub fn new(
        field: gpui::Entity<AddressField>,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_window_appearance(window, |shell, window, cx| {
            let next = Palette::for_theme(shell.theme, is_dark(window.appearance()));
            if next != shell.palette {
                shell.palette = next;
                shell.field.update(cx, |field, cx| {
                    field.set_palette(next);
                    cx.notify();
                });
                cx.notify();
            }
        })
        .detach();

        cx.subscribe(&field, |shell, _field, event, cx| match event {
            FieldEvent::Submitted(text) => shell.submit(text.clone(), cx),
            FieldEvent::Cancelled => {
                shell.status = None;
                cx.notify();
            }
            FieldEvent::Changed(_) => {}
        })
        .detach();

        Self {
            tabs: vec![
                TabState::new("New Tab", "search://newtab"),
                TabState::new("searlx — design notes", "https://example.com/design"),
            ],
            active: 0,
            field,
            focus_handle: cx.focus_handle(),
            palette,
            theme: Theme::System,
            tint: SpaceTint::None,
            status: None,
        }
    }

    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active = index;
            self.status = None;
            cx.notify();
        }
    }

    fn new_tab(&mut self, cx: &mut Context<Self>) {
        self.tabs
            .insert(self.active + 1, TabState::new("New Tab", "search://newtab"));
        self.active += 1;
        self.status = None;
        self.focus_address(cx);
    }

    fn close_tab(&mut self, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            self.status = Some("The last tab stays open".into());
            cx.notify();
            return;
        }
        self.tabs.remove(self.active);
        self.active = self.active.min(self.tabs.len() - 1);
        cx.notify();
    }

    fn cycle(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let len = self.tabs.len() as isize;
        self.active = ((self.active as isize + delta).rem_euclid(len)) as usize;
        cx.notify();
    }

    fn focus_address(&mut self, cx: &mut Context<Self>) {
        self.field.update(cx, |field, cx| {
            field.select_all_text(cx);
            cx.notify();
        });
        cx.notify();
    }

    fn submit(&mut self, text: String, cx: &mut Context<Self>) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        match PageUrl::parse(trimmed) {
            Ok(url) => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.address = SharedString::from(url.as_str().to_string());
                    tab.title = url
                        .host()
                        .map_or_else(
                            || url.display(),
                            |host| host.trim_start_matches("www.").to_owned(),
                        )
                        .into();
                }
                self.status = None;
            }
            Err(_) => {
                self.status =
                    Some("That is not an address yet — Phase 1 adds search parsing".into());
            }
        }
        cx.notify();
    }

    fn on_new_tab(&mut self, _: &NewTab, _: &mut Window, cx: &mut Context<Self>) {
        self.new_tab(cx);
    }

    fn on_close_tab(&mut self, _: &CloseTab, _: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(cx);
    }

    fn on_focus_address(&mut self, _: &FocusAddress, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_address(cx);
        let handle = self.field.read(cx).focus_handle(cx);
        window.focus(&handle);
    }

    fn on_next_tab(&mut self, _: &NextTab, _: &mut Window, cx: &mut Context<Self>) {
        self.cycle(1, cx);
    }

    fn on_previous_tab(&mut self, _: &PreviousTab, _: &mut Window, cx: &mut Context<Self>) {
        self.cycle(-1, cx);
    }

    fn on_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn tab_row(&self, index: usize, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let tab = &self.tabs[index];
        let active = index == self.active;
        let palette = self.palette;
        let title = tab.title.clone();
        let is_last = self.tabs.len() == 1;
        div()
            .id(("tab", index))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(metrics::TAB_ROW_HEIGHT))
            .px_2()
            .mx_1()
            .rounded_md()
            .cursor_pointer()
            .bg(if active {
                palette.selected
            } else {
                palette.chrome
            })
            .hover(move |style| style.bg(palette.hover))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |shell, _, _, cx| shell.activate(index, cx)),
            )
            .child(
                div()
                    .size(px(18.0))
                    .rounded(px(5.0))
                    .bg(if active { palette.accent } else { palette.line })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.0))
                    .text_color(palette.selected)
                    .child(
                        tab.title
                            .chars()
                            .next()
                            .map_or_else(|| "·".to_string(), |c| c.to_uppercase().to_string()),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(12.5))
                    .text_color(if active { palette.text } else { palette.muted })
                    .child(title),
            )
            .when(!is_last, |row| {
                row.child(
                    div()
                        .id(("close", index))
                        .w(px(18.0))
                        .h(px(18.0))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.0))
                        .text_color(palette.muted)
                        .hover(move |style| style.bg(palette.hover).text_color(palette.text))
                        .child("×")
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |shell, _, _, cx| {
                                shell.activate(index, cx);
                                shell.close_tab(cx);
                            }),
                        ),
                )
            })
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = is_dark(window.appearance());
        let palette = Palette::for_theme(self.theme, dark);
        if palette != self.palette {
            self.palette = palette;
            self.field.update(cx, |field, cx| {
                field.set_palette(palette);
                cx.notify();
            });
        }
        let wash = self.tint.wash(dark);
        let active = &self.tabs[self.active];
        let address = active.address.clone();
        let title = active.title.clone();
        let status = self.status.clone();

        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(palette.ground)
            .text_color(palette.text)
            .font_family("Inter")
            .child(
                // Tab column. Phase 0 shows the left-column layout only.
                div()
                    .flex()
                    .flex_col()
                    .w(px(metrics::SIDEBAR_DEFAULT))
                    .h_full()
                    .bg(wash.map_or(palette.chrome, Into::into))
                    .border_r_1()
                    .border_color(palette.line)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .h(px(38.0))
                            .child(
                                div()
                                    .size(px(metrics::SPACE_DOT))
                                    .rounded_full()
                                    .bg(palette.accent),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(palette.muted)
                                    .child("Personal"),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id("new-tab")
                                    .w(px(22.0))
                                    .h(px(22.0))
                                    .rounded(px(6.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.0))
                                    .text_color(palette.muted)
                                    .cursor_pointer()
                                    .hover(move |style| {
                                        style.bg(palette.hover).text_color(palette.text)
                                    })
                                    .child("+")
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|shell, _, _, cx| shell.new_tab(cx)),
                                    ),
                            ),
                    )
                    .child(div().flex().flex_col().gap_0p5().py_1().children({
                        let mut rows = Vec::with_capacity(self.tabs.len());
                        for index in 0..self.tabs.len() {
                            rows.push(self.tab_row(index, cx));
                        }
                        rows
                    }))
                    .child(div().flex_1())
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_size(px(10.5))
                            .text_color(palette.muted)
                            .child(format!(
                                "{} tab{}",
                                self.tabs.len(),
                                if self.tabs.len() == 1 { "" } else { "s" }
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .child(
                        // A drag strip stands in for the hidden title bar.
                        div().id("title-drag").h(px(8.0)).w_full().on_mouse_down(
                            MouseButton::Left,
                            |_, window, _| {
                                window.start_window_move();
                            },
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px(px(metrics::PAGE_GAP))
                            .pb_2()
                            .child(
                                div()
                                    .id("address-shell")
                                    .flex()
                                    .items_center()
                                    .h(px(metrics::FIELD_HEIGHT))
                                    .px_3()
                                    .flex_1()
                                    .max_w(px(metrics::FIELD_WIDTH))
                                    .rounded(px(8.0))
                                    .bg(palette.selected)
                                    .border_1()
                                    .border_color(palette.line)
                                    .cursor_text()
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|shell, _, window, cx| {
                                            shell.focus_address(cx);
                                            let handle = shell.field.read(cx).focus_handle(cx);
                                            window.focus(&handle);
                                        }),
                                    )
                                    .child(self.field.clone()),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(palette.muted)
                                    .child("Phase 0 shell"),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .mx(px(metrics::PAGE_GAP))
                            .mb(px(metrics::PAGE_GAP))
                            .rounded(px(metrics::PAGE_RADIUS))
                            .bg(palette.page)
                            .border_1()
                            .border_color(palette.line)
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(28.0))
                                            .text_color(palette.text)
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(palette.muted)
                                            .child(address),
                                    )
                                    .child(
                                        div()
                                            .mt_4()
                                            .text_size(px(11.0))
                                            .text_color(palette.muted)
                                            .child("No engine attached — P0-02 shell"),
                                    )
                                    .when_some(status, |page, status| {
                                        page.child(
                                            div()
                                                .mt_2()
                                                .px_2()
                                                .py_1()
                                                .rounded(px(6.0))
                                                .bg(palette.hover)
                                                .text_size(px(11.0))
                                                .text_color(palette.danger)
                                                .child(status),
                                        )
                                    }),
                            )
                            .child(
                                // Proof that gpui content sits above the page
                                // surface. The real engine overlay test is P0-03/04/05.
                                div()
                                    .absolute()
                                    .top(px(12.0))
                                    .right(px(12.0))
                                    .px_2()
                                    .py_1()
                                    .rounded(px(6.0))
                                    .bg(palette.accent)
                                    .text_size(px(10.0))
                                    .text_color(gpui::white())
                                    .child("overlay layer"),
                            ),
                    ),
            )
            .on_action(cx.listener(Self::on_new_tab))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_focus_address))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_previous_tab))
            .on_action(cx.listener(Self::on_quit))
    }
}

fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

/// Run the app. Blocks until the last window closes.
///
/// # Errors
/// Returns [`UiError::Window`] if the window cannot be opened.
pub fn run() -> Result<(), UiError> {
    let startup_error: std::rc::Rc<std::cell::RefCell<Option<UiError>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let error_slot = startup_error.clone();
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys(key_bindings());
        cx.bind_keys(shell_key_bindings());
        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::new(
            point(px(80.0), px(80.0)),
            size(
                px(metrics::WINDOW_DEFAULT_WIDTH),
                px(metrics::WINDOW_DEFAULT_HEIGHT),
            ),
        );
        let window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(
                    px(metrics::WINDOW_MIN_WIDTH),
                    px(metrics::WINDOW_MIN_HEIGHT),
                )),
                titlebar: Some(TitlebarOptions {
                    title: Some("searlx".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                window_decorations: Some(WindowDecorations::Client),
                app_id: Some("searlx".into()),
                ..Default::default()
            },
            |window, cx| {
                let dark = is_dark(window.appearance());
                let palette = Palette::for_theme(Theme::System, dark);
                let field = cx.new(|cx| AddressField::new(palette, cx));
                cx.new(|cx| Shell::new(field, palette, window, cx))
            },
        ) {
            Ok(window) => window,
            Err(error) => {
                *error_slot.borrow_mut() = Some(UiError::Window(error.to_string()));
                cx.quit();
                return;
            }
        };

        if let Err(error) = window.update(cx, |shell, window, cx| {
            let handle = shell.field.read(cx).focus_handle(cx);
            window.focus(&handle);
            cx.activate(true);
        }) {
            *error_slot.borrow_mut() = Some(UiError::Window(error.to_string()));
            cx.quit();
        }
    });

    match std::rc::Rc::try_unwrap(startup_error) {
        Ok(cell) => match cell.into_inner() {
            Some(error) => Err(error),
            None => Ok(()),
        },
        Err(_) => Ok(()),
    }
}
