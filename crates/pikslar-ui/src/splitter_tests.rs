use super::*;
use crate::focus::Press;
use crate::theme::Theme;

const W: i32 = 120;
const H: i32 = 60;
/// A standing gutter at x 50, four units wide at scale 1.
const GUTTER_AT: Rect = Rect::new(50, 0, 4, 60);
const SPAN: Span = Span {
    size: 100,
    least: 40,
    most: 160,
    step: 4,
    after: false,
};

fn pass(ui: &mut Ui, px: &mut [u32], span: Span) -> Option<SplitChange> {
    let mut sf = Surface { px, w: W, h: H };
    ui.begin();
    let out = splitter(ui, &mut sf, 9, GUTTER_AT, Axis::Across, span);
    ui.end();
    out
}

fn canvas() -> Vec<u32> {
    vec![0u32; (W * H) as usize]
}

#[test]
fn dragging_the_gutter_resizes_the_pane_by_whole_units_inside_its_limits() {
    let mut px = canvas();
    let mut ui = Ui::new();
    ui.mouse = (51, 30);
    ui.pressed = true;
    ui.down = true;
    assert_eq!(pass(&mut ui, &mut px, SPAN), None, "caught, not moved");
    ui.mouse = (61, 30);
    assert_eq!(pass(&mut ui, &mut px, SPAN), Some(SplitChange::Resize(110)));
    let right = Span {
        after: true,
        ..SPAN
    };
    assert_eq!(
        pass(&mut ui, &mut px, right),
        Some(SplitChange::Resize(90)),
        "a pane after the gutter shrinks as it moves forward"
    );
    ui.mouse = (500, 30);
    assert_eq!(pass(&mut ui, &mut px, SPAN), Some(SplitChange::Resize(160)));
}

#[test]
fn arrows_move_a_step_shift_four_home_and_end_go_to_the_limits_enter_folds() {
    let mut px = canvas();
    let mut ui = Ui::new();
    pass(&mut ui, &mut px, SPAN);
    ui.focus.set(9);
    let mut key = |ui: &mut Ui, key: WidgetKey, shift: bool| {
        ui.focus.request_press(Press { key, shift });
        pass(ui, &mut px, SPAN)
    };
    let resize = |n| Some(SplitChange::Resize(n));
    assert_eq!(key(&mut ui, WidgetKey::Right, false), resize(104));
    assert_eq!(key(&mut ui, WidgetKey::Left, false), resize(96));
    assert_eq!(key(&mut ui, WidgetKey::Left, true), resize(84));
    assert_eq!(key(&mut ui, WidgetKey::Home, false), resize(40));
    assert_eq!(key(&mut ui, WidgetKey::End, false), resize(160));
    assert_eq!(key(&mut ui, WidgetKey::Up, false), None, "across its axis");
    assert_eq!(
        key(&mut ui, WidgetKey::Enter, false),
        Some(SplitChange::Fold)
    );
    let at_end = Span { size: 160, ..SPAN };
    assert_eq!(at_end.key(Axis::Across, WidgetKey::End, false), None);
    assert_eq!(
        at_end.key(Axis::Down, WidgetKey::Up, false),
        Some(SplitChange::Resize(156)),
        "a lying gutter takes Up and Down"
    );
    assert!(ui.focus.takes_keys(9));
}

#[test]
fn the_pointer_finds_it_eight_units_wide_unless_a_control_lies_there() {
    let mut px = canvas();
    let mut sf = Surface {
        px: &mut px,
        w: W,
        h: H,
    };
    let mut ui = Ui::new();
    ui.mouse = (48, 30);
    ui.begin();
    splitter(&mut ui, &mut sf, 9, GUTTER_AT, Axis::Across, SPAN);
    assert_eq!(ui.widget_at(48, 30), 9, "two units left of the gutter");
    assert_eq!(ui.widget_at(55, 30), 9, "and two right");
    assert_ne!(ui.widget_at(47, 30), 9);
    ui.begin();
    ui.interact(3, Rect::new(40, 20, 10, 20));
    splitter(&mut ui, &mut sf, 9, GUTTER_AT, Axis::Across, SPAN);
    assert_eq!(ui.widget_at(48, 30), 3, "the control keeps its edge");
}

#[test]
fn focus_draws_a_cyan_ring_outside_and_hover_widens_the_grip() {
    theme::set(Theme::LIGHTBOX);
    let mut px = canvas();
    let mut ui = Ui::new();
    ui.mouse = (100, 30);
    pass(&mut ui, &mut px, SPAN);
    let at = |px: &[u32], x: i32, y: i32| px[(y * W + x) as usize];
    assert_eq!(at(&px, 50, 30), theme::base(), "gutter in Base");
    assert_ne!(at(&px, 48, 30), theme::focus(), "no ring unfocused");
    let thin: Vec<u32> = (50..54).map(|x| at(&px, x, 30)).collect();
    ui.focus.set(9);
    ui.mouse = (52, 30);
    pass(&mut ui, &mut px, SPAN);
    assert_eq!(at(&px, 48, 30), theme::focus());
    assert_eq!(at(&px, 47, 30), theme::focus());
    assert_eq!(at(&px, 49, 30), 0, "a unit of gap");
    let thick: Vec<u32> = (50..54).map(|x| at(&px, x, 30)).collect();
    let count = |row: &[u32]| row.iter().filter(|&&c| c != theme::base()).count();
    assert_eq!(
        (count(&thin), count(&thick)),
        (1, 2),
        "{thin:x?} {thick:x?}"
    );
    theme::set(Theme::NORMAL);
}

#[test]
fn it_says_it_is_a_splitter_and_its_size() {
    let mut px = canvas();
    let mut ui = Ui::new();
    pass(&mut ui, &mut px, SPAN);
    let kind = ui.kind_of(9).unwrap_or_default();
    assert_eq!(kind.role, Role::Splitter);
    assert_eq!(kind.state, State::value(100));
}

/// In high contrast every surface is the Window colour, so the gutter is a line from end to end
/// (final review, defect 3; docs/ui-redesign.md 2.4).
#[test]
fn in_high_contrast_the_gutter_is_a_line_its_whole_length() {
    use crate::theme::SystemColors;
    for sys in [&SystemColors::BLACK, &SystemColors::WHITE] {
        crate::theme::set(Theme::high_contrast(sys));
        crate::icons::set_high_contrast(true);
        let mut px = canvas();
        let mut ui = Ui::new();
        ui.mouse = (0, 0);
        pass(&mut ui, &mut px, SPAN);
        for y in [0, 5, 30, 59] {
            let row = &px[(y * W) as usize + 50..(y * W) as usize + 54];
            assert!(row.contains(&theme::rule()), "y {y}: {row:x?}");
        }
    }
    crate::theme::set(Theme::NORMAL);
    crate::icons::set_high_contrast(false);
}
