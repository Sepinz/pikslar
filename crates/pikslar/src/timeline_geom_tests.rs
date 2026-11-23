use super::*;

const BODY_H: i32 = 82;

fn body(w_units: i32, s: i32) -> Rect {
    Rect::new(0, 40 * s, w_units * s, BODY_H * s)
}

#[test]
fn the_default_names_column_puts_the_cels_at_192_units() {
    let g = Geom::of(body(1366, 1), 1, DEFAULT_NAMES);
    assert_eq!(g.names.w, 176);
    assert_eq!(g.strip.x, 192);
    assert_eq!(g.gutter.x, 8 + 176);
    assert_eq!(g.tags.x, g.strip.x, "the tags row lines up with the frames");
}

#[test]
fn nothing_overlaps_or_leaves_the_body_at_any_width_scale_or_names_width() {
    for s in 1..=3 {
        for w in [600, 640, 800, 1000, 1366, 1920, 2560] {
            for asked in [0, 120, 176, 320, 9_999] {
                let panel = body(w, s);
                let g = Geom::of(panel, s, asked);
                let parts = g.parts();
                for (i, a) in parts.iter().enumerate() {
                    let inside = a.x >= panel.x
                        && a.right() <= panel.right()
                        && a.y >= panel.y
                        && a.bottom() <= panel.bottom();
                    assert!(inside, "{w}/{s}: {a:?}");
                    for b in &parts[i + 1..] {
                        assert!(a.intersect(*b).is_empty(), "{w}/{s}/{asked}: {a:?} {b:?}");
                    }
                }
                assert!(
                    g.strip.w >= STRIP_LEAST * s,
                    "{w}/{s}/{asked}: {:?}",
                    g.strip
                );
                assert!(g.names.right() <= g.strip.x, "{w}/{s}/{asked}");
            }
        }
    }
}

#[test]
fn the_names_column_is_kept_to_its_range_and_to_the_strip() {
    assert_eq!(names_width(1366, 0), NAMES_RANGE.0);
    assert_eq!(names_width(1366, 9_999), NAMES_RANGE.1);
    assert_eq!(names_width(1366, 200), 200);
    assert!(
        names_width(640, 9_999) < NAMES_RANGE.1,
        "a narrow body gives the strip room"
    );
    assert_eq!(names_most(640), names_width(640, 9_999));
    assert!(names_most(600) >= NAMES_RANGE.0);
}

#[test]
fn a_wider_names_column_moves_the_cels_and_the_strip_together() {
    let narrow = Geom::of(body(1366, 1), 1, 120);
    let wide = Geom::of(body(1366, 1), 1, 320);
    assert_eq!(wide.strip.x - narrow.strip.x, 200);
    assert_eq!(wide.tags.x, wide.strip.x);
    assert_eq!(
        wide.strip.right(),
        narrow.strip.right(),
        "the right end stays"
    );
}

#[test]
fn every_line_button_is_24_units_with_a_24_unit_pitch_between_neighbours() {
    let g = Geom::of(body(1366, 2), 2, DEFAULT_NAMES);
    for r in g.transport {
        assert_eq!((r.w, r.h), (48, 48));
    }
    assert_eq!(g.transport[1].x - g.transport[0].x, 52);
    assert_eq!(g.transport[4].x - g.transport[3].x, 52);
    assert_eq!((g.chevron.w, g.chevron.h), (48, 48));
}
