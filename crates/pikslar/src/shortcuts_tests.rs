use super::*;
use std::collections::HashSet;

fn read(lines: &[(&str, &str)]) -> Keymap {
    let mut map = Keymap::default();
    let mut changed = Vec::new();
    for (name, value) in lines {
        map.apply_line(name, value, &mut changed);
    }
    map
}

#[test]
fn the_usual_keys_are_all_different_and_all_named() {
    let bound: Vec<Binding> = ACTIONS
        .iter()
        .map(|(.., b)| *b)
        .filter(|b| *b != UNBOUND)
        .collect();
    let bindings: HashSet<Binding> = bound.iter().copied().collect();
    assert_eq!(bindings.len(), bound.len());
    let names: HashSet<&str> = ACTIONS.iter().map(|(_, n, ..)| *n).collect();
    assert_eq!(names.len(), ACTIONS.len());
    for (action, ..) in ACTIONS {
        assert_eq!(index(action).map(|at| ACTIONS[at].0), Some(action));
    }
}

#[test]
fn the_usual_keys_do_what_they_always_did() {
    let map = Keymap::default();
    assert_eq!(map.action_for(key(b'B')), Some(Action::Pencil));
    assert_eq!(map.action_for(ctrl(b'Z')), Some(Action::Undo));
    assert_eq!(map.action_for(ctrl_shift(b'Z')), Some(Action::Redo));
    assert_eq!(map.action_for(ctrl(b'Y')), Some(Action::RedoOther));
    assert_eq!(map.action_for(shift(b'R')), Some(Action::RotateCounter));
    assert_eq!(map.action_for(key(b'N')), Some(Action::NewFrame));
    assert_eq!(map.action_for(ctrl(b'N')), Some(Action::New));
    assert_eq!(map.action_for(key(b'0')), Some(Action::FitView));
    assert_eq!(map.action_for(key(b'J')), Some(Action::TileMode));
    assert_eq!(map.action_for(key(b'F')), None);
    assert_eq!(map.action_for(ctrl(b'J')), None);
}

#[test]
fn shift_falls_back_to_the_key_alone_only_when_it_has_no_binding_of_its_own() {
    let map = Keymap::default();
    assert_eq!(map.action_for(shift(b'B')), Some(Action::Pencil));
    assert_eq!(map.action_for(ctrl_shift(b'O')), Some(Action::Open));
    assert_eq!(map.action_for(shift(b'V')), Some(Action::FlipVertical));
    assert_eq!(map.action_for(ctrl(b'H')), None, "Ctrl never falls back");
    assert_eq!(
        map.action_for(shift(b'N')),
        None,
        "Shift N never made a frame"
    );
    assert_eq!(
        map.action_for(ctrl_shift(b'C')),
        None,
        "nor did Ctrl Shift C copy"
    );
    assert_eq!(map.action_for(shift(b'0')), Some(Action::FitView));
}

#[test]
fn binding_a_key_in_use_takes_it_from_the_other_action() {
    let mut map = Keymap::default();
    assert_eq!(
        map.bind(Action::Fill, Some(key(b'B'))),
        Some(Action::Pencil)
    );
    assert_eq!(map.binding(Action::Pencil), None);
    assert_eq!(map.action_for(key(b'B')), Some(Action::Fill));
    assert_eq!(map.action_for(key(b'G')), None);
    assert_eq!(
        map.bind(Action::Fill, Some(key(b'B'))),
        None,
        "already its own"
    );
    assert_eq!(map.bind(Action::Fill, None), None);
    assert_eq!(map.action_for(key(b'B')), None);
}

#[test]
fn bindings_read_back_as_written() {
    for b in [key(b'B'), shift(b'7'), ctrl(b'Z'), ctrl_shift(b'S')] {
        assert_eq!(parse_binding(&binding_name(Some(b))), Some(Some(b)));
    }
    assert_eq!(binding_name(Some(ctrl_shift(b'S'))), "Ctrl Shift S");
    assert_eq!(parse_binding("none"), Some(None));
    assert_eq!(parse_binding("ctrl+shift+s"), Some(Some(ctrl_shift(b'S'))));
    for bad in [
        "",
        "Ctrl",
        "Ctrl Ctrl S",
        "Alt S",
        "S T",
        "Ctrl [",
        "é",
        "Shift ?",
    ] {
        assert_eq!(parse_binding(bad), None, "{bad:?}");
    }
}

#[test]
fn a_changed_keymap_survives_its_own_lines() {
    let mut map = Keymap::default();
    map.bind(Action::Fill, Some(key(b'B')));
    map.bind(Action::Save, Some(ctrl_shift(b'Q')));
    map.bind(Action::Onion, None);
    let lines = map.changed_lines();
    assert_eq!(lines.len(), 4, "{lines:?}");
    let pairs: Vec<(String, String)> = lines
        .iter()
        .filter_map(|l| l.split_once(" = "))
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect();
    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    assert_eq!(read(&refs), map);
    assert!(Keymap::default().changed_lines().is_empty());
}

#[test]
fn damaged_key_lines_are_skipped_and_two_claims_on_one_key_keep_the_first() {
    let map = read(&[
        ("key.pencil", "Alt P"),
        ("key.nothing", "F"),
        ("key.fill", "F"),
        ("key.line", "F"),
        ("keys.eraser", "Y"),
    ]);
    assert_eq!(map.binding(Action::Pencil), Some(key(b'B')));
    assert_eq!(map.binding(Action::Fill), Some(key(b'F')));
    assert_eq!(map.binding(Action::Line), Some(key(b'L')), "F was taken");
    assert_eq!(map.binding(Action::Eraser), Some(key(b'E')));
}

#[test]
fn letters_and_digits_only() {
    assert_eq!(letter_of(b'a'), Some(b'A'));
    assert_eq!(letter_of(b'7'), Some(b'7'));
    assert_eq!(letter_of(b'['), None);
    assert_eq!(letter_of(b' '), None);
}

#[test]
fn the_window_actions_have_function_keys_that_can_be_changed_and_written() {
    let mut map = Keymap::default();
    assert_eq!(map.action_for(function(11)), Some(Action::CanvasOnly));
    assert_eq!(map.action_for(function(10)), Some(Action::MenuBar));
    assert_eq!(binding_name(map.binding(Action::CanvasOnly)), "F11");
    assert_eq!(
        parse_binding("Ctrl F9"),
        Some(Some(Binding {
            ctrl: true,
            ..function(9)
        }))
    );
    assert_eq!(parse_binding("F13"), None);
    assert_eq!(parse_binding("F"), Some(Some(key(b'F'))));
    map.bind(Action::CanvasOnly, Some(function(9)));
    assert_eq!(
        map.changed_lines(),
        vec!["key.canvas_only = F9".to_string()]
    );
    let back = read(&[("key.canvas_only", "F9")]);
    assert_eq!(back.binding(Action::CanvasOnly), Some(function(9)));
    assert_eq!(Keymap::default().changed_lines(), Vec::<String>::new());
}

#[test]
fn ctrl_b_captures_a_brush_and_b_alone_is_still_the_pencil() {
    let map = Keymap::default();
    assert_eq!(map.action_for(ctrl(b'B')), Some(Action::CaptureBrush));
    assert_eq!(map.action_for(key(b'B')), Some(Action::Pencil));
    assert_eq!(map.action_for(ctrl_shift(b'B')), None);
}

#[test]
fn the_presets_have_ctrl_digits_and_later_actions_have_no_key() {
    let mut map = Keymap::default();
    assert_eq!(map.action_for(ctrl(b'1')), Some(Action::WorkspaceDraw));
    assert_eq!(map.action_for(ctrl(b'4')), Some(Action::WorkspacePalette));
    assert_eq!(map.binding(Action::SaveCustomWorkspace), None);
    assert_eq!(
        map.bind(Action::SaveCustomWorkspace, Some(ctrl(b'1'))),
        Some(Action::WorkspaceDraw)
    );
    assert_eq!(
        map.action_for(ctrl(b'1')),
        Some(Action::SaveCustomWorkspace)
    );
}

#[test]
fn keys_the_window_handles_itself_are_reserved_and_cannot_be_bound_or_read() {
    let f = |n, ctrl, shift| Binding {
        letter: FUNCTION_BASE + n,
        ctrl,
        shift,
    };
    for reserved in [
        f(6, false, false),
        f(6, false, true),
        f(10, false, true),
        f(10, true, true),
    ] {
        assert!(is_reserved(reserved), "{reserved:?}");
        let mut map = Keymap::default();
        assert_eq!(map.bind(Action::CanvasOnly, Some(reserved)), None);
        assert_eq!(map.binding(Action::CanvasOnly), Some(f(11, false, false)));
    }
    for free in [
        f(6, true, false),
        f(10, false, false),
        f(10, true, false),
        f(11, false, true),
    ] {
        assert!(!is_reserved(free), "{free:?}");
    }
    assert_eq!(parse_binding("Shift F10"), None);
    assert_eq!(parse_binding("F6"), None);
    assert_eq!(parse_binding("Shift F6"), None);
    assert_eq!(parse_binding("Ctrl F6"), Some(Some(f(6, true, false))));
    let map = read(&[
        ("key.canvas_only", "Shift F10"),
        ("key.menu_bar", "Shift F6"),
    ]);
    assert_eq!(
        map,
        Keymap::default(),
        "settings naming reserved keys change nothing"
    );
}

#[test]
fn ctrl_shift_t_opens_and_folds_the_timeline_rows() {
    let map = Keymap::default();
    let bound = binding_name(map.binding(Action::TimelineForm));
    assert_eq!(bound, "Ctrl Shift T");
    let others = ACTIONS
        .iter()
        .filter(|&&(a, ..)| a != Action::TimelineForm)
        .filter(|&&(a, ..)| map.binding(a) == map.binding(Action::TimelineForm));
    assert_eq!(others.count(), 0, "no other action takes it");
}
