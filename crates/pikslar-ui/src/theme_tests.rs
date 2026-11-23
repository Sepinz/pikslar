use super::*;

fn channel(c: u32, shift: u32) -> f64 {
    let v = f64::from((c >> shift) & 255) / 255.0;
    if v <= 0.03928 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(c: u32) -> f64 {
    0.2126 * channel(c, 16) + 0.7152 * channel(c, 8) + 0.0722 * channel(c, 0)
}

/// WCAG 2.x contrast ratio.
fn ratio(a: u32, b: u32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

use Token::*;

const PAIR_COUNT: usize = 36;
/// Every word is read on every surface it can be drawn on (4.5), every edge and signal is seen
/// against the surfaces it borders (3). Disabled controls are exempt from the text minimum.
const PAIR_LIST: [(Token, Token, f64); PAIR_COUNT] = [
    (Text, Base, 4.5),
    (Text, Panel, 4.5),
    (Text, Header, 4.5),
    (Text, Well, 4.5),
    (Text, Raised, 4.5),
    (Text, Hover, 4.5),
    (Text, Pressed, 4.5),
    (Text, Surround, 4.5),
    (Muted, Base, 4.5),
    (Muted, Panel, 4.5),
    (Muted, Header, 4.5),
    (Muted, Well, 4.5),
    (Muted, Raised, 4.5),
    (Muted, Hover, 4.5),
    (Accent, Panel, 4.5),
    (Accent, Header, 4.5),
    (OnAccent, Accent, 4.5),
    (OnAccent, AccentHover, 4.5),
    (OnAccent, AccentPressed, 4.5),
    (Danger, Panel, 4.5),
    (Danger, Header, 4.5),
    (Warn, Panel, 4.5),
    (Ok, Panel, 4.5),
    (Cel, Well, 4.5),
    (Edge, Base, 3.0),
    (Edge, Panel, 3.0),
    (Edge, Header, 3.0),
    (Edge, Well, 3.0),
    (EdgeHover, Panel, 3.0),
    (EdgeHover, Header, 3.0),
    (Focus, Base, 3.0),
    (Focus, Panel, 3.0),
    (Focus, Header, 3.0),
    (Focus, Raised, 3.0),
    (Focus, Surround, 3.0),
    (Accent, Well, 3.0),
];
/// Foreground, background, minimum ratio. Text needs 4.5, edges 3.
const PAIRS: [(Token, Token, f64); PAIR_COUNT] = PAIR_LIST;

fn meets_minimums(theme: &Theme, name: &str) {
    for (fg, bg, min) in PAIRS {
        let got = ratio(theme.get(fg), theme.get(bg));
        assert!(got >= min, "{name} {fg:?}/{bg:?}: {got:.2} is below {min}");
    }
}

#[test]
fn palette_meets_contrast_minimums() {
    meets_minimums(&Theme::NORMAL, "normal");
}

#[test]
fn muted_stays_visibly_below_text() {
    let t = Theme::NORMAL;
    for bg in [Panel, Raised, Hover] {
        assert!(ratio(t.get(Text), t.get(bg)) - ratio(t.get(Muted), t.get(bg)) >= 2.0);
    }
}

#[test]
fn the_new_tokens_come_after_the_old_thirteen() {
    assert_eq!(EdgeHover as usize, 12, "the old order is kept");
    assert_eq!(Base as usize, 13);
    assert_eq!(DropTarget as usize, TOKENS - 1);
    assert_eq!(TOKENS, 28);
}

/// System colors that all differ, so each token shows which one it took.
const DISTINCT: SystemColors = SystemColors {
    window: 0xff00_0001,
    window_text: 0xff00_0002,
    button_face: 0xff00_0003,
    button_text: 0xff00_0004,
    highlight: 0xff00_0005,
    highlight_text: 0xff00_0006,
    gray_text: 0xff00_0007,
    hot_light: 0xff00_0008,
};

#[test]
fn high_contrast_maps_every_token_as_the_plan_says() {
    let sys = DISTINCT;
    let t = Theme::high_contrast(&sys);
    let n = Theme::NORMAL;
    let table = [
        (Base, sys.window),
        (Panel, sys.window),
        (Header, sys.window),
        (Well, sys.window),
        (Raised, sys.window),
        (Hover, sys.window),
        (Pressed, sys.window),
        (Lip, sys.window),
        (Rule, sys.window_text),
        (Edge, sys.window_text),
        (EdgeHover, sys.hot_light),
        (Text, sys.window_text),
        (Muted, sys.window_text),
        (Disabled, sys.gray_text),
        (Accent, sys.highlight),
        (AccentHover, sys.highlight),
        (AccentPressed, sys.highlight),
        (OnAccent, sys.highlight_text),
        (Focus, sys.window_text),
        (Danger, sys.window_text),
        (Warn, sys.window_text),
        (Ok, sys.window_text),
        (Surround, sys.window),
        (CheckerA, n.get(CheckerA)),
        (CheckerB, n.get(CheckerB)),
        (Cel, sys.window_text),
        (Playhead, sys.highlight),
        (DropTarget, sys.window_text),
    ];
    assert_eq!(table.len(), TOKENS, "every token is in the table");
    for (token, color) in table {
        assert_eq!(t.get(token), color, "{token:?}");
    }
}

#[test]
fn high_contrast_muted_words_are_window_text_and_only_disabled_is_gray() {
    for sys in [SystemColors::BLACK, SystemColors::WHITE, DISTINCT] {
        let t = Theme::high_contrast(&sys);
        assert_eq!(t.get(Muted), sys.window_text);
        let gray: Vec<u32> = (0..TOKENS)
            .filter(|&i| t.colors[i] == sys.gray_text)
            .map(|i| i as u32)
            .collect();
        assert_eq!(gray, [Disabled as u32], "GrayText is used once");
    }
}

#[test]
fn high_contrast_lip_equals_raised_so_it_is_not_drawn() {
    let t = Theme::high_contrast(&SystemColors::BLACK);
    assert_eq!(t.get(Lip), t.get(Raised));
}

#[test]
fn both_classic_high_contrast_themes_meet_the_minimums() {
    meets_minimums(&Theme::high_contrast(&SystemColors::BLACK), "black");
    meets_minimums(&Theme::high_contrast(&SystemColors::WHITE), "white");
}

#[test]
fn the_current_theme_is_per_thread_and_starts_normal() {
    assert_eq!(current(), Theme::NORMAL);
    set(Theme::high_contrast(&SystemColors::WHITE));
    assert_eq!(panel(), 0xffff_ffff);
    std::thread::spawn(|| assert_eq!(panel(), Theme::NORMAL.get(Panel)))
        .join()
        .unwrap_or_else(|_| panic!("the other thread failed"));
    set(Theme::NORMAL);
    assert_eq!(panel(), 0xff1b_1c21);
}

#[test]
fn the_lightbox_table_has_the_planned_values() {
    let t = Theme::LIGHTBOX;
    let planned = [
        (Panel, 0xff1b_1c21),
        (Raised, 0xff2a_2c34),
        (Hover, 0xff34_3743),
        (Pressed, 0xff17_181c),
        (Rule, 0xff0b_0b0d),
        (Text, 0xffe8_e4da),
        (Muted, 0xffa3_a1ab),
        (Accent, 0xffff_b347),
        (AccentHover, 0xffff_c76f),
        (AccentPressed, 0xffd9_9436),
        (OnAccent, 0xff1a_1408),
        (Edge, 0xff6a_6e7e),
        (EdgeHover, 0xff8a_8fa3),
        (Base, 0xff12_1316),
        (Header, 0xff23_242b),
        (Well, 0xff14_1519),
        (Lip, 0xff3b_3e4a),
        (Disabled, 0xff6b_6a73),
        (Focus, 0xff4c_c9f0),
        (Danger, 0xffff_6b5e),
        (Warn, 0xffff_d166),
        (Ok, 0xff7b_d88f),
        (Surround, 0xff2b_2b2b),
        (CheckerA, 0xff3d_3d3d),
        (CheckerB, 0xff34_3434),
        (Cel, 0xffc9_c5bb),
        (Playhead, 0xffff_b347),
        (DropTarget, 0xff4c_c9f0),
    ];
    assert_eq!(planned.len(), TOKENS, "every token is in the table");
    for (token, color) in planned {
        assert_eq!(t.get(token), color, "{token:?}");
    }
    assert_eq!(t.get(Playhead), t.get(Accent));
    assert_eq!(t.get(DropTarget), t.get(Focus));
}

/// Every pair of docs/ui-redesign.md 2.3: foreground, background, the ratio the plan measured,
/// and the minimum it needs.
const LIGHTBOX_PAIRS: [(Token, Token, f64, f64); 12] = [
    (Text, Panel, 13.4, 7.0),
    (Text, Header, 12.2, 7.0),
    (Text, Raised, 11.0, 7.0),
    (Muted, Panel, 6.7, 4.5),
    (Muted, Header, 6.1, 4.5),
    (Muted, Raised, 5.5, 4.5),
    (OnAccent, Accent, 10.3, 4.5),
    (Focus, Panel, 8.8, 3.0),
    (Focus, Base, 9.7, 3.0),
    (Edge, Panel, 3.4, 3.0),
    (Danger, Panel, 6.1, 4.5),
    (Disabled, Panel, 3.2, 3.0),
];

#[test]
fn lightbox_meets_every_planned_contrast_pair() {
    let t = Theme::LIGHTBOX;
    for (fg, bg, planned, min) in LIGHTBOX_PAIRS {
        let got = ratio(t.get(fg), t.get(bg));
        assert!(got >= min, "{fg:?}/{bg:?}: {got:.2} is below {min}");
        assert!(
            (got - planned).abs() <= 0.1,
            "{fg:?}/{bg:?}: {got:.2}, the plan says {planned}"
        );
    }
}

#[test]
fn lightbox_meets_the_old_minimums_too() {
    meets_minimums(&Theme::LIGHTBOX, "lightbox");
}
