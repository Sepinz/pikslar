//! Interface colors, as opaque BGRA, read through a small table of semantic tokens.
//!
//! The normal theme is Lightbox: slate surfaces, a neutral grey around the canvas, amber for what
//! is chosen and cyan for keyboard focus. When Windows high contrast
//! is on, [`Theme::high_contrast`] builds the table from the system colors instead. The current
//! table is kept per thread, since only the window's thread draws the interface; each test
//! thread starts from the normal theme.

use std::cell::Cell;

/// What a color is for. A later redesign can add tokens without touching the callers of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Panel,
    Raised,
    Hover,
    Pressed,
    Rule,
    Text,
    Muted,
    Accent,
    AccentHover,
    AccentPressed,
    OnAccent,
    /// The 1-unit edge drawn around buttons.
    Edge,
    /// The edge of a button under the pointer.
    EdgeHover,
    /// Gutters, splitters and the space outside the docks.
    Base,
    /// The menu bar, tab strips and the status bar.
    Header,
    /// Fields, lists, sliders and the cel grid.
    Well,
    /// The 1-unit highlight under a raised face's top edge; not drawn when it equals Raised.
    Lip,
    /// Unavailable controls and hidden layers.
    Disabled,
    /// The keyboard focus ring and a focused splitter.
    Focus,
    /// Errors and destructive confirmations.
    Danger,
    /// Pixel Lint findings and warnings.
    Warn,
    /// Saved, passed.
    Ok,
    /// The background around the canvas.
    Surround,
    /// The light squares of the transparency checker.
    CheckerA,
    /// The dark squares of the transparency checker.
    CheckerB,
    /// The mark of a filled cel in the timeline.
    Cel,
    /// The current frame's column.
    Playhead,
    /// A dock's drop zone while a tab is dragged.
    DropTarget,
}

/// How many tokens there are: the first 13 and the 15 added for the redesign.
const TOKENS: usize = 28;

const LIGHTBOX_ACCENT: u32 = 0xffff_b347;
const LIGHTBOX_FOCUS: u32 = 0xff4c_c9f0;

/// One color for each token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    colors: [u32; TOKENS],
}

/// The system colors a high contrast theme is drawn with, as 0xffRRGGBB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemColors {
    pub window: u32,
    pub window_text: u32,
    pub button_face: u32,
    pub button_text: u32,
    pub highlight: u32,
    pub highlight_text: u32,
    pub gray_text: u32,
    pub hot_light: u32,
}

impl SystemColors {
    /// Windows' classic High Contrast Black, for tests and `--high-contrast black`.
    pub const BLACK: SystemColors = SystemColors {
        window: 0xff00_0000,
        window_text: 0xffff_ffff,
        button_face: 0xff00_0000,
        button_text: 0xffff_ffff,
        highlight: 0xff1a_ebff,
        highlight_text: 0xff00_0000,
        gray_text: 0xff3f_f23f,
        hot_light: 0xffff_ff00,
    };
    /// Windows' classic High Contrast White, for tests and `--high-contrast white`.
    pub const WHITE: SystemColors = SystemColors {
        window: 0xffff_ffff,
        window_text: 0xff00_0000,
        button_face: 0xffff_ffff,
        button_text: 0xff00_0000,
        highlight: 0xff37_006e,
        highlight_text: 0xffff_ffff,
        gray_text: 0xff60_0000,
        hot_light: 0xff00_009f,
    };
}

impl Theme {
    /// The usual look: Lightbox.
    pub const NORMAL: Theme = Theme::LIGHTBOX;

    /// Lightbox (docs/ui-redesign.md 2.3): a calm slate bench around a
    /// neutral grey surround, amber for what is chosen and cyan for keyboard focus only.
    pub const LIGHTBOX: Theme = Theme {
        colors: [
            0xff1b_1c21,     // Panel
            0xff2a_2c34,     // Raised
            0xff34_3743,     // Hover
            0xff17_181c,     // Pressed
            0xff0b_0b0d,     // Rule
            0xffe8_e4da,     // Text
            0xffa3_a1ab,     // Muted
            LIGHTBOX_ACCENT, // Accent
            0xffff_c76f,     // AccentHover
            0xffd9_9436,     // AccentPressed
            0xff1a_1408,     // OnAccent
            0xff6a_6e7e,     // Edge
            0xff8a_8fa3,     // EdgeHover
            0xff12_1316,     // Base
            0xff23_242b,     // Header
            0xff14_1519,     // Well
            0xff3b_3e4a,     // Lip
            0xff6b_6a73,     // Disabled
            LIGHTBOX_FOCUS,  // Focus
            0xffff_6b5e,     // Danger
            0xffff_d166,     // Warn
            0xff7b_d88f,     // Ok
            0xff2b_2b2b,     // Surround
            0xff3d_3d3d,     // CheckerA
            0xff34_3434,     // CheckerB
            0xffc9_c5bb,     // Cel
            LIGHTBOX_ACCENT, // Playhead
            LIGHTBOX_FOCUS,  // DropTarget
        ],
    };

    /// The look for Windows high contrast: every surface takes the window color, every
    /// boundary and word the window text color, and what is chosen the highlight pair. States
    /// show by shape: hover on a button's edge, press by the one-unit drop, focus as a ring
    /// outside the control. Only disabled controls take the gray text color. The Lip equals
    /// Raised, so it is not drawn; the checker keeps the normal look's greys, being part of the
    /// artwork view rather than the chrome.
    pub const fn high_contrast(sys: &SystemColors) -> Theme {
        let (surface, ink, chosen) = (sys.window, sys.window_text, sys.highlight);
        let normal = Theme::NORMAL;
        Theme {
            colors: [
                surface,                                 // Panel
                surface,                                 // Raised
                surface,                                 // Hover
                surface,                                 // Pressed
                ink,                                     // Rule
                ink,                                     // Text
                ink,                                     // Muted
                chosen,                                  // Accent
                chosen,                                  // AccentHover
                chosen,                                  // AccentPressed
                sys.highlight_text,                      // OnAccent
                ink,                                     // Edge
                sys.hot_light,                           // EdgeHover
                surface,                                 // Base
                surface,                                 // Header
                surface,                                 // Well
                surface,                                 // Lip
                sys.gray_text,                           // Disabled
                ink,                                     // Focus
                ink,                                     // Danger
                ink,                                     // Warn
                ink,                                     // Ok
                surface,                                 // Surround
                normal.colors[Token::CheckerA as usize], // CheckerA
                normal.colors[Token::CheckerB as usize], // CheckerB
                ink,                                     // Cel
                chosen,                                  // Playhead
                ink,                                     // DropTarget
            ],
        }
    }

    pub const fn get(&self, token: Token) -> u32 {
        self.colors[token as usize]
    }
}

thread_local! {
    static CURRENT: Cell<Theme> = const { Cell::new(Theme::NORMAL) };
}

/// Draws from now on with `theme`, on this thread.
pub fn set(theme: Theme) {
    CURRENT.with(|c| c.set(theme));
}

/// The theme drawing uses now, on this thread.
pub fn current() -> Theme {
    CURRENT.with(Cell::get)
}

/// The current color for `token`.
pub fn color(token: Token) -> u32 {
    CURRENT.with(|c| c.get().get(token))
}

pub fn panel() -> u32 {
    color(Token::Panel)
}
pub fn raised() -> u32 {
    color(Token::Raised)
}
pub fn hover() -> u32 {
    color(Token::Hover)
}
pub fn pressed() -> u32 {
    color(Token::Pressed)
}
pub fn rule() -> u32 {
    color(Token::Rule)
}
pub fn text() -> u32 {
    color(Token::Text)
}
pub fn muted() -> u32 {
    color(Token::Muted)
}
pub fn accent() -> u32 {
    color(Token::Accent)
}
pub fn accent_hover() -> u32 {
    color(Token::AccentHover)
}
pub fn accent_pressed() -> u32 {
    color(Token::AccentPressed)
}
pub fn on_accent() -> u32 {
    color(Token::OnAccent)
}
pub fn edge() -> u32 {
    color(Token::Edge)
}
pub fn edge_hover() -> u32 {
    color(Token::EdgeHover)
}
pub fn base() -> u32 {
    color(Token::Base)
}
pub fn header() -> u32 {
    color(Token::Header)
}
pub fn well() -> u32 {
    color(Token::Well)
}
pub fn lip() -> u32 {
    color(Token::Lip)
}
pub fn disabled() -> u32 {
    color(Token::Disabled)
}
pub fn focus() -> u32 {
    color(Token::Focus)
}
pub fn danger() -> u32 {
    color(Token::Danger)
}
pub fn warn() -> u32 {
    color(Token::Warn)
}
pub fn ok() -> u32 {
    color(Token::Ok)
}
pub fn surround() -> u32 {
    color(Token::Surround)
}
pub fn checker_a() -> u32 {
    color(Token::CheckerA)
}
pub fn checker_b() -> u32 {
    color(Token::CheckerB)
}
pub fn cel() -> u32 {
    color(Token::Cel)
}
pub fn playhead() -> u32 {
    color(Token::Playhead)
}
pub fn drop_target() -> u32 {
    color(Token::DropTarget)
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
