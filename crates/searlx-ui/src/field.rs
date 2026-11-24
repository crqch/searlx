//! The one address field.
//!
//! Adapted from gpui's Apache-2.0 `examples/input.rs` (gpui 0.2.2), trimmed to
//! the browser's needs. Composition (IME) text is underlined while marked, and
//! selection, clipboard, grapheme-aware cursor movement, and emoji all go
//! through gpui's input handler, so the field behaves like a real text control.

use crate::theme::Palette;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Hsla, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    Render, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div,
    fill, point, prelude::*, px, relative, rgba, size,
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

actions!(
    address_field,
    [
        Backspace,
        Delete,
        DeleteWordBackward,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Submit,
        SubmitShift,
        SubmitBackground,
        SubmitForeground,
        MoveUp,
        MoveDown,
        TabComplete,
        ShiftTabComplete,
        Cancel,
    ]
);

/// The default key bindings for the field. Registered once at startup.
#[must_use]
pub fn key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("backspace", Backspace, Some("AddressField")),
        KeyBinding::new("alt-backspace", DeleteWordBackward, Some("AddressField")),
        KeyBinding::new("delete", Delete, Some("AddressField")),
        KeyBinding::new("left", Left, Some("AddressField")),
        KeyBinding::new("right", Right, Some("AddressField")),
        KeyBinding::new("shift-left", SelectLeft, Some("AddressField")),
        KeyBinding::new("shift-right", SelectRight, Some("AddressField")),
        KeyBinding::new("cmd-a", SelectAll, Some("AddressField")),
        KeyBinding::new("ctrl-a", SelectAll, Some("AddressField")),
        KeyBinding::new("cmd-v", Paste, Some("AddressField")),
        KeyBinding::new("ctrl-v", Paste, Some("AddressField")),
        KeyBinding::new("cmd-c", Copy, Some("AddressField")),
        KeyBinding::new("ctrl-c", Copy, Some("AddressField")),
        KeyBinding::new("cmd-x", Cut, Some("AddressField")),
        KeyBinding::new("ctrl-x", Cut, Some("AddressField")),
        KeyBinding::new("home", Home, Some("AddressField")),
        KeyBinding::new("end", End, Some("AddressField")),
        KeyBinding::new("enter", Submit, Some("AddressField")),
        KeyBinding::new("shift-enter", SubmitShift, Some("AddressField")),
        KeyBinding::new("cmd-enter", SubmitBackground, Some("AddressField")),
        KeyBinding::new("ctrl-enter", SubmitBackground, Some("AddressField")),
        KeyBinding::new("cmd-shift-enter", SubmitForeground, Some("AddressField")),
        KeyBinding::new("ctrl-shift-enter", SubmitForeground, Some("AddressField")),
        KeyBinding::new("up", MoveUp, Some("AddressField")),
        KeyBinding::new("down", MoveDown, Some("AddressField")),
        KeyBinding::new("tab", TabComplete, Some("AddressField")),
        KeyBinding::new("shift-tab", ShiftTabComplete, Some("AddressField")),
        KeyBinding::new("escape", Cancel, Some("AddressField")),
    ]
}

/// What the field tells its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldEvent {
    /// Return was pressed with the current text.
    Submitted(String),
    /// Return was pressed with modifier keys (`⌘Return`, `⇧⌘Return`, `⇧Return`).
    SubmittedWith {
        /// Current typed text.
        text: String,
        /// Whether `Cmd`/`Ctrl` was held.
        command: bool,
        /// Whether `Shift` was held.
        shift: bool,
    },
    /// Escape was pressed.
    Cancelled,
    /// The text changed.
    Changed(String),
    /// Backspace was pressed when the field was already empty (clears site-search chip).
    BackspaceOnEmpty,
    /// Arrow up/down inside the field to walk suggestions.
    SuggestionStep(isize),
    /// Tab or Shift-Tab pressed inside the field (walks suggestions or locks site chip).
    TabPressed {
        /// True for `Shift+Tab`.
        shift: bool,
    },
}

/// The address/search field.
pub struct AddressField {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    inline_completion: Option<SharedString>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<gpui::ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    palette: Palette,
    placeholder_color: Hsla,
}

impl std::fmt::Debug for AddressField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddressField")
            .field("length", &self.content.len())
            .field("marked", &self.marked_range.is_some())
            .field("has_completion", &self.inline_completion.is_some())
            .finish_non_exhaustive()
    }
}

impl AddressField {
    /// Create an empty field.
    pub fn new(palette: Palette, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: "Search or enter address".into(),
            inline_completion: None,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            palette,
            placeholder_color: palette.muted,
        }
    }

    /// Update the palette after a theme change.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
        self.placeholder_color = palette.muted;
    }

    /// Update the placeholder text shown when empty.
    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = placeholder.into();
        self.last_layout = None;
        cx.notify();
    }

    /// Update the grey inline completion suffix (`FEATURES.md` §4.3).
    pub fn set_inline_completion(&mut self, completion: Option<String>, cx: &mut Context<Self>) {
        let next = completion.filter(|s| !s.is_empty()).map(SharedString::from);
        if self.inline_completion != next {
            self.inline_completion = next;
            self.last_layout = None;
            cx.notify();
        }
    }

    /// Current grey inline completion suffix, if any.
    #[must_use]
    pub fn inline_completion(&self) -> Option<&str> {
        self.inline_completion.as_deref().map(AsRef::as_ref)
    }

    /// The current text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replace the text, placing the cursor at the end.
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        cx.notify();
    }

    /// Select everything, as `⌘L` does.
    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.content.is_empty() && self.selected_range.is_empty() {
            self.inline_completion = None;
            cx.emit(FieldEvent::BackspaceOnEmpty);
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.inline_completion = None;
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inline_completion = None;
        if self.selected_range.is_empty() {
            let start = self.previous_word_boundary(self.cursor_offset());
            self.select_to(start, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.selected_range.end), cx);
        }
        self.inline_completion = None;
        self.replace_text_in_range(None, "", window, cx);
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::Submitted(self.content.to_string()));
    }

    fn submit_shift(&mut self, _: &SubmitShift, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::SubmittedWith {
            text: self.content.to_string(),
            command: false,
            shift: true,
        });
    }

    fn submit_background(&mut self, _: &SubmitBackground, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::SubmittedWith {
            text: self.content.to_string(),
            command: true,
            shift: false,
        });
    }

    fn submit_foreground(&mut self, _: &SubmitForeground, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::SubmittedWith {
            text: self.content.to_string(),
            command: true,
            shift: true,
        });
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::SuggestionStep(-1));
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::SuggestionStep(1));
    }

    fn tab_complete(&mut self, _: &TabComplete, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::TabPressed { shift: false });
    }

    fn shift_tab_complete(&mut self, _: &ShiftTabComplete, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::TabPressed { shift: true });
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::Cancelled);
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = text.replace(['\n', '\r'], " ");
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.clamp_char_range(&self.selected_range);
        if !range.is_empty()
            && let Some(slice) = self.content.get(range)
        {
            cx.write_to_clipboard(ClipboardItem::new_string(slice.to_owned()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.clamp_char_range(&self.selected_range);
        if !range.is_empty()
            && let Some(slice) = self.content.get(range)
        {
            cx.write_to_clipboard(ClipboardItem::new_string(slice.to_owned()));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn emit_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::Changed(self.content.to_string()));
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let clamped = self.clamp_char_boundary(offset);
        self.selected_range = clamped..clamped;
        self.selection_reversed = false;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
            .min(self.content.len())
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp_char_boundary(offset);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn clamp_char_boundary(&self, offset: usize) -> usize {
        let mut idx = offset.min(self.content.len());
        while idx > 0 && !self.content.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

    fn clamp_char_range(&self, range: &Range<usize>) -> Range<usize> {
        let start = self.clamp_char_boundary(range.start);
        let end = self.clamp_char_boundary(range.end).max(start);
        start..end
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn previous_word_boundary(&self, offset: usize) -> usize {
        let prefix = &self.content[..self.clamp_char_boundary(offset)];
        let trimmed = prefix.trim_end();
        if trimmed.is_empty() {
            return 0;
        }
        trimmed
            .rfind(|c: char| c.is_whitespace() || c == '/' || c == '.')
            .map_or(0, |idx| {
                let mut pos = idx;
                if let Some(ch) = trimmed[idx..].chars().next() {
                    pos += ch.len_utf8();
                }
                if pos == trimmed.len() { idx } else { pos }
            })
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for AddressField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.clamp_char_range(&self.range_from_utf16(&range_utf16));
        actual_range.replace(self.range_to_utf16(&range));
        self.content.get(range).map(ToOwned::to_owned)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw_range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let range = self.clamp_char_range(&raw_range);

        let before = self.content.get(0..range.start).unwrap_or("");
        let after = self.content.get(range.end..).unwrap_or("");
        let next_content = format!("{before}{new_text}{after}");
        let changed = self.content.as_ref() != next_content.as_str();

        self.content = next_content.into();
        let caret = range.start + new_text.len();
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.marked_range.take();
        if changed {
            self.emit_changed(cx);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw_range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let range = self.clamp_char_range(&raw_range);

        let before = self.content.get(0..range.start).unwrap_or("");
        let after = self.content.get(range.end..).unwrap_or("");
        let next_content = format!("{before}{new_text}{after}");
        let changed = self.content.as_ref() != next_content.as_str();

        self.content = next_content.into();
        if new_text.is_empty() {
            self.marked_range = None;
        } else {
            self.marked_range = Some(range.start..range.start + new_text.len());
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| {
                let s = (new_range.start + range.start).min(self.content.len());
                let e = (new_range.end + range.start).min(self.content.len()).max(s);
                s..e
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        if changed {
            self.emit_changed(cx);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.clamp_char_range(&self.range_from_utf16(&range_utf16));
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        if !last_layout.text.starts_with(self.content.as_ref()) {
            return None;
        }
        let utf8_index = last_layout
            .index_for_x(point.x - line_point.x)?
            .min(self.content.len());
        Some(self.offset_to_utf16(utf8_index))
    }
}

impl Focusable for AddressField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl gpui::EventEmitter<FieldEvent> for AddressField {}

struct FieldElement {
    input: Entity<AddressField>,
    palette: Palette,
    placeholder_color: Hsla,
}

struct PrepaintState {
    line: Option<gpui::ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for FieldElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FieldElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let completion = input.inline_completion.clone();
        let selected_range = input.clamp_char_range(&input.selected_range);
        let cursor = input.clamp_char_boundary(input.cursor_offset());
        let style = window.text_style();

        let (display_text, runs) = if content.is_empty() {
            let text = input.placeholder.clone();
            let run = TextRun {
                len: text.len(),
                font: style.font(),
                color: self.placeholder_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            (text, vec![run])
        } else if let Some(marked_range) = input.marked_range.as_ref() {
            let marked = input.clamp_char_range(marked_range);
            let run = TextRun {
                len: content.len(),
                font: style.font(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end.saturating_sub(marked.start),
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: content.len().saturating_sub(marked.end),
                    ..run
                },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect();
            (content, runs)
        } else if let Some(suffix) = completion.as_ref().filter(|s| !s.is_empty()) {
            let full: SharedString = format!("{content}{suffix}").into();
            let runs = vec![
                TextRun {
                    len: content.len(),
                    font: style.font(),
                    color: style.color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                },
                TextRun {
                    len: suffix.len(),
                    font: style.font(),
                    color: self.palette.muted,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                },
            ];
            (full, runs)
        } else {
            let run = TextRun {
                len: content.len(),
                font: style.font(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            (content, vec![run])
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        size(px(1.5), bounds.bottom() - bounds.top()),
                    ),
                    self.palette.accent,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(0x3b74f233),
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let Some(line) = prepaint.line.take() else {
            return;
        };
        if line
            .paint(bounds.origin, window.line_height(), window, cx)
            .is_ok()
            && focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for AddressField {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .key_context("AddressField")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete_word_backward))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::submit_shift))
            .on_action(cx.listener(Self::submit_background))
            .on_action(cx.listener(Self::submit_foreground))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::tab_complete))
            .on_action(cx.listener(Self::shift_tab_complete))
            .on_action(cx.listener(Self::cancel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .h_full()
            .w_full()
            .child(FieldElement {
                input: cx.entity(),
                palette: self.palette,
                placeholder_color: self.placeholder_color,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Application;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn headless_address_field_manages_text_completion_and_selection() {
        Application::headless().run(|cx| {
            let palette = Palette::light();
            let field = cx.new(|cx| AddressField::new(palette, cx));
            let events: Rc<RefCell<Vec<FieldEvent>>> = Rc::new(RefCell::new(Vec::new()));
            let events_ref = events.clone();
            cx.subscribe(&field, move |_, event, _| {
                events_ref.borrow_mut().push(event.clone());
            })
            .detach();

            field.update(cx, |f, cx| {
                f.set_text("git", cx);
                f.set_inline_completion(Some("hub.com".into()), cx);
                assert_eq!(f.text(), "git");
                assert_eq!(f.inline_completion(), Some("hub.com"));
                f.select_all_text(cx);
                assert_eq!(f.selected_range, 0..3);
                f.set_placeholder("Filter...", cx);

                // Safe UTF-8 boundary clamping never panics even on out-of-range offsets
                let clamped = f.clamp_char_range(&(0..999));
                assert_eq!(clamped, 0..3);
                assert_eq!(f.previous_word_boundary(3), 0);
            });
            // Programmatic set_text does not emit spurious Changed events
            assert!(events.borrow().is_empty());

            cx.spawn(async move |cx| {
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        });
    }
}
