use super::*;

#[test]
fn the_layout_names_the_focused_widget_on_a_line_of_its_own() {
    let mut ui = Ui::new();
    ui.record_names();
    let mut px = vec![0u32; 100 * 40];
    let mut sf = Surface {
        px: &mut px,
        w: 100,
        h: 40,
    };
    let pass = |ui: &mut Ui, sf: &mut Surface<'_>| {
        ui.begin();
        ui.focus.collect(true);
        button(ui, sf, 1, Rect::new(0, 0, 40, 20), "Create", false);
        button(ui, sf, 2, Rect::new(50, 0, 40, 20), "Cancel", false);
        ui.focus.collect(false);
        ui.end();
    };
    pass(&mut ui, &mut sf);
    let text = ui.layout().unwrap_or_default();
    assert!(!text.contains("focus:"), "nothing is focused");
    ui.focus.set(2);
    pass(&mut ui, &mut sf);
    let text = ui.layout().unwrap_or_default();
    assert!(
        text.ends_with("0\t50\t0\t40\t20\tfocus: Cancel\tbutton\t\n"),
        "{text}"
    );
}
