use super::*;
use crate::tilemap_oracle::{random_scene, Rng};
use pikslar_core::{rgba, Damage, DrawCmd, StepCmd, Tool};

/// What a compositor with the same onion skin, made just now, shows for the whole canvas.
fn fresh(e: &Editor, like: &Compositor) -> Vec<u32> {
    let mut comp = like.resized(like.width(), like.height());
    comp.update(e, Damage::All, &mut Vec::new());
    comp.pixels().to_vec()
}

fn random_region(rng: &mut Rng, (w, h): (usize, usize)) -> Option<Rect> {
    if rng.chance(5) {
        return None;
    }
    let (x, y) = (
        rng.below(w as u32) as i32 - 4,
        rng.below(h as u32) as i32 - 4,
    );
    let size = (
        1 + rng.below(w as u32) as i32,
        1 + rng.below(h as u32) as i32,
    );
    Some(Rect::new(x, y, size.0, size.1))
}

/// A screen as the window keeps it: pixels copied from the compositor only where an update
/// says something changed, and the whole shown part whenever the view moves.
struct Screen {
    px: Vec<u32>,
    width: usize,
}

impl Screen {
    fn copy(&mut self, comp: &Compositor, r: Rect) {
        let r = r.intersect(Rect::new(0, 0, comp.width() as i32, comp.height() as i32));
        for y in r.y..r.bottom() {
            let start = y as usize * self.width + r.x as usize;
            let end = start + r.w as usize;
            self.px[start..end].copy_from_slice(&comp.pixels()[start..end]);
        }
    }

    fn check(&self, wanted: &[u32], region: Rect, what: &str) {
        let r = region.intersect(Rect::new(
            0,
            0,
            self.width as i32,
            (wanted.len() / self.width) as i32,
        ));
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let i = y as usize * self.width + x as usize;
                assert_eq!(self.px[i], wanted[i], "{what} at {x},{y}");
            }
        }
    }
}

/// One random change: a frame step, a view move, a stroke on any layer, or an undo.
fn act(rng: &mut Rng, e: &mut Editor, comp: &mut Compositor, region: &mut Option<Rect>) -> bool {
    let (w, h) = (e.doc().width, e.doc().height);
    match rng.below(4) {
        0 => {
            let from = e.frame;
            e.frame = rng.below(e.doc().frames.len() as u32) as usize;
            if comp.onion {
                e.damage_all();
            } else {
                e.damage_frame_change(from);
            }
        }
        1 => {
            *region = random_region(rng, (w, h));
            comp.set_visible(*region);
            return true;
        }
        2 => {
            e.layer = rng.below(e.doc().layers().len() as u32) as usize;
            e.color = rgba(rng.below(256) as u8, 40, 200, 255);
            let point = |rng: &mut Rng| (rng.below(w as u32) as i32, rng.below(h as u32) as i32);
            let path = vec![point(rng), point(rng)];
            let _ = e.apply(DrawCmd::Stroke {
                tool: Tool::Pencil,
                path,
            });
        }
        _ => {
            let _ = e.apply(StepCmd::Undo);
        }
    }
    false
}

#[test]
fn what_is_shown_is_always_what_a_full_composite_shows() {
    for seed in 1..=60u64 {
        let scene = random_scene(seed);
        let mut rng = Rng(seed * 31 + 7);
        let mut e = Editor::new(scene.doc.clone());
        let (w, h) = (e.doc().width, e.doc().height);
        let mut comp = Compositor::new(w, h);
        comp.onion = rng.chance(3);
        comp.onion_depth = 1 + rng.below(2) as usize;
        // Most scenes composite in bands on threads, whatever the number of tiles.
        comp.set_threads(1 + rng.below(4) as usize, rng.below(3) as usize);
        let mut region = random_region(&mut rng, (w, h));
        comp.set_visible(region);
        let mut screen = Screen {
            px: vec![0; w * h],
            width: w,
        };
        let whole = Rect::new(0, 0, w as i32, h as i32);
        for step in 0..40 {
            let moved = step == 0 || act(&mut rng, &mut e, &mut comp, &mut region);
            let mut changed = Vec::new();
            let damage = if step == 0 {
                Damage::All
            } else {
                e.take_damage()
            };
            comp.update(&e, damage, &mut changed);
            let shown = region.unwrap_or(whole);
            changed.iter().for_each(|&r| screen.copy(&comp, r));
            if moved {
                screen.copy(&comp, shown);
            }
            let what = format!("seed {seed} step {step}");
            screen.check(&fresh(&e, &comp), shown, &what);
        }
        // Settling reads anything off screen as it is now.
        let part = (1, 2, w / 2, h - 2);
        comp.settle(&e, part);
        let wanted = fresh(&e, &comp);
        let mut settled = Screen {
            px: comp.pixels().to_vec(),
            width: w,
        };
        let part_rect = Rect::new(1, 2, part.2 as i32, part.3 as i32);
        settled.check(&wanted, part_rect, &format!("seed {seed} settled part"));
        comp.settle(&e, (0, 0, w, h));
        assert_eq!(comp.stale_tiles(), 0, "seed {seed}");
        settled.px = comp.pixels().to_vec();
        settled.check(&wanted, whole, &format!("seed {seed} settled whole"));
    }
}

/// One compositor on one thread and one on several, stepped alike through random changes,
/// views and onion skin, hold the same pixels bit for bit and report the same areas. Scenes
/// are 18 to 67 pixels wide and 18 to 57 high, so they have two to four tile rows, edge tiles
/// cut by the canvas, and shown regions that cut through tiles. The later seeds use
/// Aseprite-compatible blending.
#[test]
fn threads_composite_bit_for_bit_what_one_thread_does() {
    for seed in 1..=60u64 {
        let scene = random_scene(seed);
        let mut rng = Rng(seed * 17 + 3);
        let mut doc = scene.doc.clone();
        if seed > 40 {
            doc.blending = pikslar_core::Blending::Aseprite;
        }
        let mut e = Editor::new(doc);
        let (w, h) = (e.doc().width, e.doc().height);
        let mut one = Compositor::new(w, h);
        one.set_threads(1, 0);
        one.onion = rng.chance(3);
        let mut many = one.resized(w, h);
        many.set_threads(2 + rng.below(3) as usize, 0);
        let mut region = random_region(&mut rng, (w, h));
        for comp in [&mut one, &mut many] {
            comp.set_visible(region);
        }
        for step in 0..30 {
            if step > 0 {
                act(&mut rng, &mut e, &mut one, &mut region);
                many.set_visible(region);
            }
            let damage = match step {
                0 => Damage::All,
                _ => e.take_damage(),
            };
            let (mut a, mut b) = (Vec::new(), Vec::new());
            one.update(&e, damage.clone(), &mut a);
            many.update(&e, damage, &mut b);
            assert!(one.pixels() == many.pixels(), "seed {seed} step {step}");
            assert_eq!(a, b, "seed {seed} step {step}");
        }
        let all = (0, 0, w, h);
        one.settle(&e, all);
        many.settle(&e, all);
        assert!(one.pixels() == many.pixels(), "seed {seed} settled");
    }
}

/// Shared bands: scenes of 10 to 25 tile rows cut into many more bands than threads (two to
/// eight a thread, or one a tile row), each thread taking the next band left, hold the same
/// pixels bit for bit as one thread and report the same areas.
#[test]
fn many_more_bands_than_threads_composite_bit_for_bit_what_one_thread_does() {
    for seed in 1..=30u64 {
        let scene = crate::tilemap_oracle::random_tall_scene(seed, 150 + seed as usize * 7);
        let mut rng = Rng(seed * 29 + 11);
        let mut doc = scene.doc.clone();
        if seed > 20 {
            doc.blending = pikslar_core::Blending::Aseprite;
        }
        let mut e = Editor::new(doc);
        let (w, h) = (e.doc().width, e.doc().height);
        let mut one = Compositor::new(w, h);
        one.set_threads(1, 0);
        one.onion = rng.chance(3);
        let mut many = one.resized(w, h);
        many.set_threads(2 + rng.below(3) as usize, 0);
        let per_thread = [2, 3, 8, usize::MAX][rng.below(4) as usize];
        many.set_bands_per_thread(per_thread);
        let mut region = random_region(&mut rng, (w, h));
        for comp in [&mut one, &mut many] {
            comp.set_visible(region);
        }
        for step in 0..20 {
            if step > 0 {
                act(&mut rng, &mut e, &mut one, &mut region);
                many.set_visible(region);
            }
            let damage = match step {
                0 => Damage::All,
                _ => e.take_damage(),
            };
            let (mut a, mut b) = (Vec::new(), Vec::new());
            one.update(&e, damage.clone(), &mut a);
            many.update(&e, damage, &mut b);
            assert!(one.pixels() == many.pixels(), "seed {seed} step {step}");
            assert_eq!(a, b, "seed {seed} step {step}");
        }
        let all = (0, 0, w, h);
        one.settle(&e, all);
        many.settle(&e, all);
        assert!(one.pixels() == many.pixels(), "seed {seed} settled");
    }
}

#[test]
fn tiles_off_screen_wait_and_show_once_they_are_on_screen() {
    let scene = random_scene(3);
    let e = Editor::new(scene.doc.clone());
    let (w, h) = (e.doc().width, e.doc().height);
    let mut comp = Compositor::new(w, h);
    comp.set_visible(Some(Rect::new(0, 0, 1, 1)));
    let mut changed = Vec::new();
    comp.update(&e, Damage::All, &mut changed);
    let tiles = w.div_ceil(TILE) * h.div_ceil(TILE);
    assert_eq!(
        comp.stale_tiles(),
        tiles - 1,
        "only the first tile was composited"
    );
    comp.set_visible(None);
    changed.clear();
    comp.update(&e, Damage::None, &mut changed);
    assert_eq!(comp.stale_tiles(), 0);
    assert_eq!(changed.len(), tiles - 1, "each tile caught up is reported");
    assert!(comp.pixels() == fresh(&e, &comp).as_slice());
}

#[test]
fn tiles_left_stale_are_reported_once_around_them() {
    let (w, h) = (TILE * 3 + 5, TILE * 2 + 3);
    let doc = pikslar_core::Document::new(w, h, pikslar_core::ColorMode::Rgba).expect("valid");
    let e = Editor::new(doc);
    let mut comp = Compositor::new(w, h);
    let side = TILE as i32;
    comp.set_visible(Some(Rect::new(0, 0, 1, 1)));
    comp.update(&e, Damage::Tiles(vec![0]), &mut Vec::new());
    assert!(
        comp.take_deferred().is_empty(),
        "a shown tile is not deferred"
    );
    let last = (w.div_ceil(TILE) * h.div_ceil(TILE) - 1) as u32;
    comp.update(&e, Damage::Tiles(vec![1, last]), &mut Vec::new());
    let around = comp.take_deferred();
    assert_eq!((around.x, around.y), (side, 0));
    assert_eq!((around.right(), around.bottom()), (w as i32, h as i32));
    assert!(comp.take_deferred().is_empty(), "taken once");
}

#[test]
fn the_shown_region_is_every_pixel_the_blit_reads() {
    let view = View {
        num: 1,
        den: 4,
        x: 10,
        y: -3,
    };
    let area = Rect::new(0, 0, 50, 40);
    assert_eq!(
        shown_region(view, area, (2048, 2048)),
        Rect::new(0, 12, 157, 157)
    );
    let zoomed = View {
        num: 3,
        den: 1,
        x: -30,
        y: 5,
    };
    assert_eq!(
        shown_region(zoomed, area, (64, 64)),
        Rect::new(10, 0, 17, 12)
    );
    assert!(shown_region(zoomed, Rect::default(), (64, 64)).is_empty());
}
