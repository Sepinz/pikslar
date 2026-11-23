//! Menus with more rows than the window holds, and labels wider than it.

use super::*;

const BOUNDS: Rect = Rect::new(0, 0, 640, 480);

fn items(count: usize, label: impl Fn(usize) -> String) -> Vec<String> {
    (0..count).map(label).collect()
}

fn as_items(labels: &[String]) -> Vec<MenuItem<'_>> {
    labels
        .iter()
        .map(|l| MenuItem {
            label: l,
            enabled: true,
        })
        .collect()
}

fn draw(ui: &mut Ui, px: &mut [u32], rect: Rect, items: &[MenuItem<'_>]) -> MenuResult {
    let mut sf = Surface { px, w: 640, h: 480 };
    ui.begin();
    let out = menu(ui, &mut sf, 100, rect, items);
    ui.end();
    out
}

#[test]
fn a_200_character_label_and_40_items_leave_no_ink_outside_the_menu() {
    let labels = items(40, |n| {
        if n == 0 {
            "x".repeat(200)
        } else {
            format!("Item {n}")
        }
    });
    let list = as_items(&labels);
    let rect = menu_rect(1, (300, 100), &list, BOUNDS);
    assert!(rect.right() <= 640 && rect.bottom() <= 480, "{rect:?}");
    let mut px = vec![0u32; 640 * 480];
    let mut ui = Ui::new();
    draw(&mut ui, &mut px, rect, &list);
    let frame = rect.inset(-1);
    for y in 0..480 {
        for x in 0..640 {
            if !frame.contains(x, y) {
                assert_eq!(
                    px[(y * 640 + x) as usize],
                    0,
                    "ink at {x},{y} outside {rect:?}"
                );
            }
        }
    }
}

#[test]
fn the_last_of_forty_items_can_be_reached_by_keys_and_chosen() {
    let labels = items(40, |n| format!("Item {n}"));
    let list = as_items(&labels);
    let rect = menu_rect(1, (10, 10), &list, BOUNDS);
    assert!(rect.h < 40 * 24, "the menu is capped to the window");
    let mut px = vec![0u32; 640 * 480];
    let mut ui = Ui::new();
    assert_eq!(menu_key(&list, None, MenuKey::End), MenuNav::Highlight(39));
    ui.menu_highlight = Some(39);
    draw(&mut ui, &mut px, rect, &list);
    let (r, _) = *ui
        .widgets()
        .iter()
        .find(|(_, id)| *id == 100 + 39)
        .expect("row 39 is shown once highlighted");
    assert!(rect.contains(r.x, r.y) && r.bottom() <= rect.bottom());
    assert_eq!(
        menu_key(&list, Some(39), MenuKey::Enter),
        MenuNav::Chosen(39)
    );
    // And by the pointer, once it is shown.
    ui.menu_highlight = None;
    ui.mouse = (r.x + 5, r.y + 5);
    ui.pressed = true;
    ui.down = true;
    ui.released = true;
    assert_eq!(draw(&mut ui, &mut px, rect, &list), MenuResult::Chosen(39));
}

#[test]
fn the_wheel_scrolls_the_rows_and_stops_at_both_ends() {
    let labels = items(40, |n| format!("Item {n}"));
    let list = as_items(&labels);
    let rect = menu_rect(1, (10, 10), &list, BOUNDS);
    let mut px = vec![0u32; 640 * 480];
    let mut ui = Ui::new();
    draw(&mut ui, &mut px, rect, &list);
    assert!(ui.widgets().iter().any(|(_, id)| *id == 100));
    assert!(!ui.widgets().iter().any(|(_, id)| *id == 139));
    ui.menu_scroll(-1000);
    draw(&mut ui, &mut px, rect, &list);
    assert!(ui.widgets().iter().any(|(_, id)| *id == 139), "the end");
    assert!(!ui.widgets().iter().any(|(_, id)| *id == 100));
    ui.menu_scroll(1000);
    draw(&mut ui, &mut px, rect, &list);
    assert!(
        ui.widgets().iter().any(|(_, id)| *id == 100),
        "back at the top"
    );
}

#[test]
fn a_menu_that_fits_shows_every_row_from_the_top() {
    let labels = items(5, |n| format!("Item {n}"));
    let list = as_items(&labels);
    let rect = menu_rect(1, (10, 10), &list, BOUNDS);
    let mut px = vec![0u32; 640 * 480];
    let mut ui = Ui::new();
    ui.menu_scroll(-50);
    draw(&mut ui, &mut px, rect, &list);
    assert_eq!(ui.widgets().len(), 5);
}
