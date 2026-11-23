//! The letter and digit shortcuts, which the user can change. Named keys (arrows, Enter,
//! Escape, Delete, Space, the brackets, plus and minus) keep their meaning.

/// What a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    Pencil,
    Eraser,
    Fill,
    Picker,
    Line,
    Rect,
    Marquee,
    Lasso,
    Wand,
    Move,
    CaptureBrush,
    KeyCursor,
    Undo,
    Redo,
    RedoOther,
    New,
    Open,
    PlaceReference,
    Save,
    SaveAs,
    Export,
    ExportSheet,
    SelectAll,
    SelectNone,
    Invert,
    Copy,
    Cut,
    Paste,
    FlipHorizontal,
    FlipVertical,
    RotateClockwise,
    RotateCounter,
    NewFrame,
    DuplicateFrame,
    Hold,
    HoldAcross,
    Tag,
    Direction,
    Onion,
    OnionDepth,
    TruePreview,
    FitView,
    Group,
    TileMode,
    Text,
    TextFont,
    TextScale,
    TextSpacing,
    TextAlign,
    CanvasSize,
    ViewMenu,
    ScriptsMenu,
    KeysDialog,
    WorkspaceDraw,
    WorkspaceAnimate,
    WorkspaceTiles,
    WorkspacePalette,
    CanvasOnly,
    MenuBar,
    WindowMenu,
    SaveCustomWorkspace,
    ResetWorkspace,
    LargeText,
    // The actions of docs/ui-redesign.md phases 5 and 6, not in [`ACTIONS`] yet, so without a
    // key and out of the Keys dialog; each phase fills its handler and lists its actions.
    /// Phase 5: focus moves to the context bar.
    #[allow(dead_code)]
    ContextBar,
    /// Phase 6: the timeline between its single line and its open rows.
    TimelineForm,
}

impl Action {
    /// Tools, Open, New and Fit always ignored Shift, before the keys could be changed.
    fn ignores_shift(self) -> bool {
        index(self)
            .zip(index(Action::Move))
            .is_some_and(|(at, last_tool)| at <= last_tool)
            || matches!(
                self,
                Action::Open | Action::New | Action::FitView | Action::Text
            )
    }
}

/// A letter or digit, with Ctrl and Shift as they must be held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Binding {
    pub letter: u8,
    pub ctrl: bool,
    pub shift: bool,
}

const fn key(letter: u8) -> Binding {
    Binding {
        letter,
        ctrl: false,
        shift: false,
    }
}

const fn shift(letter: u8) -> Binding {
    Binding {
        letter,
        ctrl: false,
        shift: true,
    }
}

const fn ctrl(letter: u8) -> Binding {
    Binding {
        letter,
        ctrl: true,
        shift: false,
    }
}

const fn ctrl_shift(letter: u8) -> Binding {
    Binding {
        letter,
        ctrl: true,
        shift: true,
    }
}

/// A function key, which a binding holds as its virtual-key code (F1 is 0x70), never a letter.
const fn function(n: u8) -> Binding {
    Binding {
        letter: FUNCTION_BASE + n,
        ctrl: false,
        shift: false,
    }
}

/// What a function key's number is added to to make its binding's letter.
pub(crate) const FUNCTION_BASE: u8 = 0x6F;

/// The usual key of an action that has none: no binding can be this.
const UNBOUND: Binding = key(0);

/// Every action, with its name in the settings file, its name on screen and its usual key.
pub(crate) const ACTIONS: [(Action, &str, &str, Binding); 64] = [
    (Action::Pencil, "pencil", "Pencil", key(b'B')),
    (Action::Eraser, "eraser", "Eraser", key(b'E')),
    (Action::Fill, "fill", "Fill", key(b'G')),
    (Action::Picker, "picker", "Color picker", key(b'I')),
    (Action::Line, "line", "Line", key(b'L')),
    (Action::Rect, "rectangle", "Rectangle", key(b'U')),
    (Action::Marquee, "marquee", "Marquee", key(b'M')),
    (Action::Lasso, "lasso", "Lasso", key(b'Q')),
    (Action::Wand, "wand", "Magic wand", key(b'W')),
    (Action::Move, "move", "Move", key(b'V')),
    (
        Action::CaptureBrush,
        "capture_brush",
        "Capture brush",
        ctrl(b'B'),
    ),
    (
        Action::KeyCursor,
        "key_cursor",
        "Keyboard cursor",
        key(b'C'),
    ),
    (Action::Text, "text", "Text", key(b'X')),
    (Action::TextFont, "text_font", "Text font", ctrl(b'F')),
    (Action::TextScale, "text_scale", "Text size", ctrl(b'T')),
    (
        Action::TextSpacing,
        "text_spacing",
        "Text letter gap",
        ctrl(b'K'),
    ),
    (
        Action::TextAlign,
        "text_align",
        "Text alignment",
        ctrl(b'L'),
    ),
    (Action::Undo, "undo", "Undo", ctrl(b'Z')),
    (Action::Redo, "redo", "Redo", ctrl_shift(b'Z')),
    (
        Action::RedoOther,
        "redo_other",
        "Redo, other key",
        ctrl(b'Y'),
    ),
    (Action::New, "new", "New sprite", ctrl(b'N')),
    (Action::Open, "open", "Open", ctrl(b'O')),
    (
        Action::PlaceReference,
        "place_reference",
        "Place reference",
        ctrl_shift(b'R'),
    ),
    (Action::Save, "save", "Save", ctrl(b'S')),
    (Action::SaveAs, "save_as", "Save as", ctrl_shift(b'S')),
    (Action::Export, "export", "Export", ctrl(b'E')),
    (
        Action::ExportSheet,
        "export_sheet",
        "Export sheet",
        ctrl_shift(b'E'),
    ),
    (Action::SelectAll, "select_all", "Select all", ctrl(b'A')),
    (Action::SelectNone, "select_none", "Select none", ctrl(b'D')),
    (
        Action::Invert,
        "invert",
        "Invert selection",
        ctrl_shift(b'I'),
    ),
    (Action::Copy, "copy", "Copy", ctrl(b'C')),
    (Action::Cut, "cut", "Cut", ctrl(b'X')),
    (Action::Paste, "paste", "Paste", ctrl(b'V')),
    (
        Action::FlipHorizontal,
        "flip_horizontal",
        "Flip across",
        shift(b'H'),
    ),
    (
        Action::FlipVertical,
        "flip_vertical",
        "Flip up-down",
        shift(b'V'),
    ),
    (Action::RotateClockwise, "rotate", "Rotate right", key(b'R')),
    (
        Action::RotateCounter,
        "rotate_back",
        "Rotate left",
        shift(b'R'),
    ),
    (Action::NewFrame, "new_frame", "New frame", key(b'N')),
    (
        Action::DuplicateFrame,
        "duplicate_frame",
        "Copy frame",
        key(b'D'),
    ),
    (Action::Hold, "hold", "Hold drawing", key(b'K')),
    (
        Action::HoldAcross,
        "hold_across",
        "Hold across",
        shift(b'K'),
    ),
    (Action::Tag, "tag", "Tag frames", key(b'T')),
    (Action::Direction, "direction", "Tag direction", shift(b'T')),
    (Action::Onion, "onion", "Onion skin", key(b'O')),
    (
        Action::OnionDepth,
        "onion_depth",
        "Onion depth",
        shift(b'O'),
    ),
    (Action::TruePreview, "true_preview", "True size", key(b'P')),
    (Action::FitView, "fit_view", "Fit to window", key(b'0')),
    (Action::Group, "group_layer", "Group or ungroup", ctrl(b'G')),
    (Action::TileMode, "tile_mode", "Tiles or pixels", key(b'J')),
    (
        Action::CanvasSize,
        "canvas_size",
        "Canvas size",
        ctrl_shift(b'M'),
    ),
    (Action::ViewMenu, "view_menu", "View menu", ctrl_shift(b'V')),
    (
        Action::ScriptsMenu,
        "scripts_menu",
        "Scripts menu",
        ctrl_shift(b'P'),
    ),
    (
        Action::KeysDialog,
        "keys_dialog",
        "Keys dialog",
        ctrl_shift(b'H'),
    ),
    (
        Action::WorkspaceDraw,
        "workspace_draw",
        "Draw preset",
        ctrl(b'1'),
    ),
    (
        Action::WorkspaceAnimate,
        "workspace_animate",
        "Animate preset",
        ctrl(b'2'),
    ),
    (
        Action::WorkspaceTiles,
        "workspace_tiles",
        "Tiles preset",
        ctrl(b'3'),
    ),
    (
        Action::WorkspacePalette,
        "workspace_palette",
        "Palette preset",
        ctrl(b'4'),
    ),
    (
        Action::CanvasOnly,
        "canvas_only",
        "Canvas only",
        function(11),
    ),
    (Action::MenuBar, "menu_bar", "Menu bar", function(10)),
    (
        Action::TimelineForm,
        "timeline_form",
        "Timeline rows",
        ctrl_shift(b'T'),
    ),
    (
        Action::WindowMenu,
        "window_menu",
        "Window menu",
        ctrl_shift(b'W'),
    ),
    (
        Action::SaveCustomWorkspace,
        "save_workspace",
        "Save workspace",
        UNBOUND,
    ),
    (
        Action::ResetWorkspace,
        "reset_workspace",
        "Reset workspace",
        UNBOUND,
    ),
    (Action::LargeText, "large_text", "Large text", UNBOUND),
];

/// Where `action` is in [`ACTIONS`]; none for an action not listed there yet.
fn index(action: Action) -> Option<usize> {
    ACTIONS.iter().position(|&(a, ..)| a == action)
}

/// The name of `binding` as written in settings and shown on screen, such as `Ctrl Shift S`.
pub(crate) fn binding_name(binding: Option<Binding>) -> String {
    let Some(b) = binding else {
        return "none".to_string();
    };
    let mut name = String::new();
    if b.ctrl {
        name.push_str("Ctrl ");
    }
    if b.shift {
        name.push_str("Shift ");
    }
    match b.letter.checked_sub(FUNCTION_BASE) {
        Some(n) if n >= 1 => name.push_str(&format!("F{n}")),
        _ => name.push(char::from(b.letter)),
    }
    name
}

/// Reads a binding as [`binding_name`] writes it. `Some(None)` is a deliberately empty one.
pub(crate) fn parse_binding(text: &str) -> Option<Option<Binding>> {
    if text.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    let mut b = key(0);
    for word in text.split([' ', '+']).filter(|w| !w.is_empty()) {
        match word.to_ascii_lowercase().as_str() {
            "ctrl" if !b.ctrl => b.ctrl = true,
            "shift" if !b.shift => b.shift = true,
            _ if b.letter == 0 && function_word(word).is_some() => {
                b.letter = FUNCTION_BASE + function_word(word)?;
            }
            _ if word.len() == 1 && b.letter == 0 => b.letter = letter_of(word.as_bytes()[0])?,
            _ => return None,
        }
    }
    (b.letter != 0 && !is_reserved(b)).then_some(Some(b))
}

/// Keys the window handles itself before any shortcut: F6 and Shift F6 move between the parts of
/// the window, and F10 with Shift (and any Ctrl) opens the context menu. Ctrl F6 is free.
pub(crate) fn is_reserved(b: Binding) -> bool {
    let f = |n: u8| b.letter == FUNCTION_BASE + n;
    (f(6) && !b.ctrl) || (f(10) && b.shift)
}

/// The number of a function key written as `F11`, 1 to 12.
fn function_word(word: &str) -> Option<u8> {
    let digits = word.strip_prefix(['F', 'f'])?;
    digits.parse().ok().filter(|n| (1..=12).contains(n))
}

/// A letter or digit, in upper case, or `None` for anything a shortcut cannot use.
pub(crate) fn letter_of(byte: u8) -> Option<u8> {
    let upper = byte.to_ascii_uppercase();
    upper.is_ascii_alphanumeric().then_some(upper)
}

/// Which binding each action has, one at most, and no binding used twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Keymap {
    bindings: [Option<Binding>; ACTIONS.len()],
}

/// An action's usual key, if it has one.
fn usual(b: Binding) -> Option<Binding> {
    (b != UNBOUND).then_some(b)
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap {
            bindings: ACTIONS.map(|(.., b)| usual(b)),
        }
    }
}

impl Keymap {
    pub fn binding(&self, action: Action) -> Option<Binding> {
        index(action).and_then(|at| self.bindings[at])
    }

    /// The action for a press. Shift held with a key that has no Shift binding falls back to
    /// the key alone for actions that never cared about Shift, so Shift B still picks the
    /// pencil and Ctrl Shift O still opens.
    pub fn action_for(&self, pressed: Binding) -> Option<Action> {
        let find = |wanted: Binding| {
            ACTIONS
                .iter()
                .zip(&self.bindings)
                .find(|(_, b)| **b == Some(wanted))
                .map(|((a, ..), _)| *a)
        };
        find(pressed).or_else(|| {
            pressed
                .shift
                .then(|| {
                    find(Binding {
                        shift: false,
                        ..pressed
                    })
                })?
                .filter(|a| a.ignores_shift())
        })
    }

    /// Gives `action` the binding, taking it from any action that had it. Returns that action.
    /// An action not listed in [`ACTIONS`] takes no binding.
    pub fn bind(&mut self, action: Action, binding: Option<Binding>) -> Option<Action> {
        if binding.is_some_and(is_reserved) {
            return None;
        }
        let own = index(action)?;
        let taken = binding.and_then(|b| {
            let at = self.bindings.iter().position(|&x| x == Some(b))?;
            (at != own).then(|| {
                self.bindings[at] = None;
                ACTIONS[at].0
            })
        });
        self.bindings[own] = binding;
        taken
    }

    /// Lines for the settings file: only the actions whose binding is not the usual one.
    pub fn changed_lines(&self) -> Vec<String> {
        ACTIONS
            .iter()
            .zip(&self.bindings)
            .filter(|((.., u), b)| **b != usual(*u))
            .map(|((_, name, ..), b)| format!("key.{name} = {}", binding_name(*b)))
            .collect()
    }

    /// Applies a `key.name = binding` line, if it is one. A binding another changed action
    /// already holds is ignored; one an unchanged action holds is taken from it.
    pub fn apply_line(&mut self, name: &str, value: &str, changed: &mut Vec<Action>) {
        let Some(name) = name.strip_prefix("key.") else {
            return;
        };
        let Some(&(action, ..)) = ACTIONS.iter().find(|(_, n, ..)| *n == name) else {
            return;
        };
        let Some(binding) = parse_binding(value) else {
            return;
        };
        let held_by_changed = binding.and_then(|b| self.action_for_exact(b));
        if held_by_changed.is_some_and(|other| changed.contains(&other) && other != action) {
            return;
        }
        self.bind(action, binding);
        changed.push(action);
    }

    fn action_for_exact(&self, b: Binding) -> Option<Action> {
        let at = self.bindings.iter().position(|&x| x == Some(b))?;
        Some(ACTIONS[at].0)
    }
}

#[cfg(test)]
#[path = "shortcuts_tests.rs"]
mod tests;
