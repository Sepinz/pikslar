use super::{draw_art, draw_icon, high_contrast, set_high_contrast};
use super::{ActionIcon, Art, Icon, Look, MarkIcon, Tone, Tones, ToolIcon, ICON_SIZE};
use crate::theme::{self, Theme};
use pikslar_render::Surface;

fn every_art() -> Vec<(String, Art)> {
    let tools = ToolIcon::ALL.iter().map(|i| (format!("{i:?}"), i.art()));
    let actions = ActionIcon::ALL.iter().map(|i| (format!("{i:?}"), i.art()));
    let marks = MarkIcon::ALL.iter().map(|i| (format!("{i:?}"), i.art()));
    tools.chain(actions).chain(marks).collect()
}

fn pixels(art: Art) -> Vec<Tone> {
    let n = art.size();
    (0..n * n).map(|i| art.tone(i % n, i / n)).collect()
}

#[test]
fn the_inventory_matches_the_plan() {
    assert_eq!(ToolIcon::ALL.len(), 14);
    assert_eq!(ActionIcon::ALL.len(), 63);
    assert_eq!(MarkIcon::ALL.len(), 17);
    for (i, icon) in ToolIcon::ALL.iter().enumerate() {
        assert_eq!(*icon as usize, i, "{icon:?}");
    }
    for (i, icon) in ActionIcon::ALL.iter().enumerate() {
        assert_eq!(*icon as usize, i, "{icon:?}");
    }
    for (i, icon) in MarkIcon::ALL.iter().enumerate() {
        assert_eq!(*icon as usize, i, "{icon:?}");
    }
}

#[test]
fn every_icon_is_its_grids_size() {
    assert!(ToolIcon::ALL.iter().all(|i| i.art().size() == 16));
    assert!(ActionIcon::ALL.iter().all(|i| i.art().size() == 12));
    assert!(MarkIcon::ALL.iter().all(|i| i.art().size() == 8));
}

#[test]
fn every_icon_has_ink_and_none_repeats_another() {
    let all = every_art();
    for (name, art) in &all {
        assert!(pixels(*art).contains(&Tone::Ink), "{name} has no ink");
    }
    for (i, (a, art_a)) in all.iter().enumerate() {
        for (b, art_b) in &all[i + 1..] {
            if art_a.size() == art_b.size() {
                assert_ne!(pixels(*art_a), pixels(*art_b), "{a} and {b} are the same");
            }
        }
    }
}

#[test]
fn every_icon_keeps_a_clear_border_on_its_grid() {
    // Tools and actions keep their outer ring clear, so icons never touch a button's edge.
    // Marks fill their whole grid.
    let mut touching = Vec::new();
    for (name, art) in every_art().into_iter().filter(|(_, a)| a.size() > 8) {
        let n = art.size();
        let mut edge = (0..n).flat_map(|i| [(i, 0), (i, n - 1), (0, i), (n - 1, i)]);
        if edge.any(|(x, y)| art.tone(x, y) != Tone::Clear) {
            touching.push(name);
        }
    }
    assert!(touching.is_empty(), "touch the edge: {touching:?}");
}

#[test]
fn signal_is_kept_for_the_few_icons_that_need_it() {
    let allowed = ["Eraser", "Fill", "Wand"];
    for (name, art) in every_art() {
        let has = pixels(art).contains(&Tone::Signal);
        assert_eq!(has, allowed.contains(&name.as_str()), "{name}");
    }
}

#[test]
fn every_old_icon_draws_its_new_art() {
    use Icon::*;
    let expect = [
        (Pencil, ToolIcon::Pencil.art()),
        (Eraser, ToolIcon::Eraser.art()),
        (Fill, ToolIcon::Fill.art()),
        (Picker, ToolIcon::Picker.art()),
        (Line, ToolIcon::Line.art()),
        (Rect, ToolIcon::Rectangle.art()),
        (Eye, MarkIcon::Eye.art()),
        (Plus, ActionIcon::Plus.art()),
        (Minus, ActionIcon::Minus.art()),
        (Up, ActionIcon::MoveUp.art()),
        (Down, ActionIcon::MoveDown.art()),
        (Play, ActionIcon::Play.art()),
        (Pause, ActionIcon::Pause.art()),
        (Copy, ActionIcon::Duplicate.art()),
        (Onion, ActionIcon::OnionSkin.art()),
        (Prev, ActionIcon::PreviousFrame.art()),
        (Next, ActionIcon::NextFrame.art()),
        (Marquee, ToolIcon::SelectBox.art()),
        (Lasso, ToolIcon::Lasso.art()),
        (Wand, ToolIcon::Wand.art()),
        (Move, ToolIcon::Move.art()),
        (Lock, MarkIcon::Lock.art()),
        (Merge, ActionIcon::MergeDown.art()),
        (Group, ActionIcon::NewGroup.art()),
        (Text, ToolIcon::Text.art()),
    ];
    assert_eq!(expect.len(), Icon::ALL.len());
    for ((icon, art), listed) in expect.iter().zip(Icon::ALL) {
        assert_eq!(*icon, listed);
        assert_eq!(icon.art(), *art, "{icon:?}");
    }
}

#[test]
fn tones_follow_the_look() {
    theme::set(Theme::NORMAL);
    let normal = Tones::for_look(Look::Normal, false);
    assert_eq!(
        normal,
        Tones {
            ink: theme::text(),
            shade: theme::muted(),
            signal: theme::accent(),
        }
    );
    assert_eq!(
        Tones::for_look(Look::Normal, true),
        Tones::one(theme::text())
    );
    assert_eq!(
        Tones::for_look(Look::Chosen, false),
        Tones::one(theme::on_accent())
    );
    assert_eq!(
        Tones::for_look(Look::Disabled, false),
        Tones::one(theme::disabled())
    );
    assert_eq!(normal.get(Tone::Clear), None);
    assert_eq!(normal.get(Tone::Signal), Some(theme::accent()));
}

fn canvas(size: usize) -> Vec<u32> {
    vec![0; size * size]
}

fn draw_old(px: &mut [u32], side: i32, icon: Icon, color: u32, scale: i32) {
    let mut sf = Surface {
        px,
        w: side,
        h: side,
    };
    draw_icon(&mut sf, 0, 0, icon, color, scale);
}

#[test]
fn draw_icon_centres_the_art_at_one_pixel_a_unit_and_marks_at_two() {
    theme::set(Theme::NORMAL);
    set_high_contrast(false);
    for scale in [2, 4, 6] {
        let side = ICON_SIZE * scale;
        for icon in Icon::ALL {
            let art = icon.art();
            let unit = if art.size() == 8 { scale } else { scale / 2 };
            let inset = (side - art.size() * unit) / 2;
            let mut px = canvas(side as usize);
            draw_old(&mut px, side, icon, theme::text(), scale);
            for y in 0..side {
                for x in 0..side {
                    let tone = art.tone((x - inset).div_euclid(unit), (y - inset).div_euclid(unit));
                    let tone = if x < inset || y < inset {
                        Tone::Clear
                    } else {
                        tone
                    };
                    let want = Tones::for_look(Look::Normal, false).get(tone).unwrap_or(0);
                    assert_eq!(
                        px[(y * side + x) as usize],
                        want,
                        "{icon:?} x{scale} {x},{y}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_eye_and_lock_fill_their_old_box_at_every_scale() {
    // The old 9 by 9 icons filled 18 pixels at 1x; marks are drawn doubled to 16, on whole
    // pixels, so layer rows read as before.
    theme::set(Theme::NORMAL);
    for (icon, scale) in [
        (Icon::Eye, 2),
        (Icon::Lock, 2),
        (Icon::Eye, 4),
        (Icon::Lock, 6),
    ] {
        let side = ICON_SIZE * scale;
        let mut px = canvas(side as usize);
        draw_old(&mut px, side, icon, theme::text(), scale);
        let lit: Vec<i32> = (0..side * side).filter(|&i| px[i as usize] != 0).collect();
        let span = |f: &dyn Fn(i32) -> i32| {
            let lo = lit.iter().map(|&i| f(i)).min().unwrap_or(0);
            let hi = lit.iter().map(|&i| f(i)).max().unwrap_or(0);
            hi - lo + 1
        };
        let wide = span(&|i| i % side).max(span(&|i| i / side));
        assert!(wide > 8 * scale / 2, "{icon:?} x{scale}: {wide} px");
        assert!(wide <= 8 * scale, "{icon:?} x{scale}: inside its box");
    }
}

#[test]
fn draw_icon_reads_the_look_from_its_colour() {
    theme::set(Theme::NORMAL);
    set_high_contrast(false);
    let side = ICON_SIZE * 2;
    let colours = |color: u32| {
        let mut px = canvas(side as usize);
        draw_old(&mut px, side, Icon::Fill, color, 2);
        let mut seen: Vec<u32> = px.into_iter().filter(|&c| c != 0).collect();
        seen.sort_unstable();
        seen.dedup();
        seen
    };
    let mut normal = vec![theme::text(), theme::muted(), theme::accent()];
    normal.sort_unstable();
    assert_eq!(colours(theme::text()), normal);
    assert_eq!(colours(theme::on_accent()), vec![theme::on_accent()]);
    assert_eq!(colours(theme::muted()), vec![theme::muted()]);
    set_high_contrast(true);
    assert!(high_contrast());
    assert_eq!(colours(theme::text()), vec![theme::text()]);
    set_high_contrast(false);
}

#[test]
fn runs_fill_once_per_row_stretch() {
    // A row of one tone draws as one rectangle: a full-width strip at any unit.
    let side = 12 * 3;
    let mut px = canvas(side as usize);
    let mut sf = Surface {
        px: &mut px,
        w: side,
        h: side,
    };
    let art = ActionIcon::Minus.art();
    draw_art(&mut sf, 0, 0, art, Tones::one(7), 3);
    let row = 5 * 3;
    let line = &px[(row * side) as usize..((row + 1) * side) as usize];
    assert_eq!(line.iter().filter(|&&c| c == 7).count(), 10 * 3);
}
