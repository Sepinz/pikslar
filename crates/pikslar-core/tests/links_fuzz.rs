//! Random commands on documents with links across layers. After every step the rules of the
//! links hold, a locked layer's cels are untouched by anything that draws, and undoing walks
//! back through exactly the states the steps went through, then redo walks forward again.
//!
//! A modest number of seeds runs with the suite; `many_random_sessions_keep_the_links` runs
//! 2,000, or `PIKSLAR_LINK_SEEDS`, when asked for with `--ignored`.

use pikslar_core::command::CanvasCmd;
use pikslar_core::*;
use std::collections::HashMap;
use std::sync::Arc;

const STEPS: usize = 40;
const QUICK_SEEDS: u64 = 60;
const LONG_SEEDS: u64 = 2_000;
const INK: u32 = rgba(10, 200, 30, 255);

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn between(&mut self, a: i32, b: i32) -> i32 {
        a + (self.next() % (b - a + 1) as u64) as i32
    }

    fn coin(&mut self) -> bool {
        self.below(2) == 0
    }
}

/// One cel as seen: its link numbered by first appearance, its place and its pixels, read
/// over an area wider than the canvas so pixels past the edge count too.
type CelSeen = (u32, (i16, i16), Vec<u32>);
type Seen = Vec<(String, Vec<CelSeen>)>;

fn pixels(cel: &Cel) -> Vec<u32> {
    let mut out = Vec::with_capacity(56 * 56);
    for y in -20..36 {
        for x in -20..36 {
            out.push(cel.get(x, y));
        }
    }
    out
}

fn seen(doc: &Document) -> Seen {
    let mut numbers: HashMap<u32, u32> = HashMap::new();
    let mut out = Vec::new();
    for layer in doc.layers() {
        let mut cels = Vec::new();
        for (frame, cel) in layer.cels().iter().enumerate() {
            let id = match layer.link(frame) {
                0 => 0,
                id => {
                    let next = numbers.len() as u32 + 1;
                    *numbers.entry(id).or_insert(next)
                }
            };
            cels.push((id, cel.position(), pixels(cel)));
        }
        let name = format!("{} {:?} {:?}", layer.name, layer.kind, layer.mode);
        out.push((name, cels));
    }
    out
}

fn layer_seen(doc: &Document, index: usize) -> Vec<((i16, i16), Vec<u32>)> {
    let cels = doc.layers()[index].cels().iter();
    cels.map(|cel| (cel.position(), pixels(cel))).collect()
}

/// Four layers of five frames, one of them a palette layer, each cel with a dot of its own.
fn session() -> Editor {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("room");
    doc.add_layer(2, ColorMode::Rgba).expect("room");
    doc.add_layer(3, ColorMode::Palette).expect("room");
    let mut e = Editor::new(doc);
    for at in 1..5 {
        e.apply(FrameCmd::Add { at, copy: false }).expect("room");
    }
    e.pixel_perfect = false;
    e.color = INK;
    for layer in 0..4 {
        for frame in 0..5 {
            (e.layer, e.frame) = (layer, frame);
            let x = layer as i32 * 3 + frame as i32;
            let dot = DrawCmd::Stroke {
                tool: Tool::Pencil,
                path: vec![(x, 2)],
            };
            e.apply(dot).expect("drawn");
        }
    }
    e
}

/// Where a random command acts: a layer and frame of the document as it is.
struct Aim {
    layers: usize,
    frames: usize,
    layer: usize,
    frame: usize,
    id: LayerId,
}

fn aim(e: &Editor, r: &mut Rng) -> Aim {
    let (layers, frames) = (e.doc().layers().len(), e.doc().frames.len());
    let layer = r.below(layers);
    let id = e.layer_id(layer).expect("a layer");
    Aim {
        layers,
        frames,
        layer,
        frame: r.below(frames),
        id,
    }
}

fn drag(a: &Aim, r: &mut Rng, kind: DragKind, wide: bool) -> Command {
    let last = (a.frame + if wide { r.below(2) } else { 0 }).min(a.frames - 1);
    let landing = match r.below(4) {
        0 => Landing::Insert,
        _ => Landing::Overwrite,
    };
    CelCmd::Drag(CelDrag {
        layers: (a.layer, a.layer),
        frames: (a.frame, last),
        to_layer: r.below(a.layers),
        to_frame: r.below(a.frames),
        kind,
        landing,
    })
    .into()
}

fn cel_cmd(a: &Aim, r: &mut Rng) -> Command {
    let (layer, frame) = (a.id, a.frame);
    let (dx, dy) = (r.between(-3, 3), r.between(-3, 3));
    match r.below(8) {
        0..=2 => drag(a, r, DragKind::Link, true),
        3 => {
            let kind = if r.coin() {
                DragKind::Move
            } else {
                DragKind::Copy
            };
            drag(a, r, kind, false)
        }
        4 => CelCmd::MoveOne {
            layer,
            frame,
            dx,
            dy,
        }
        .into(),
        5 => CelCmd::Move {
            layer,
            frame,
            dx,
            dy,
        }
        .into(),
        6 => CelCmd::Turn {
            layer,
            frame,
            turn: Turn::Clockwise,
        }
        .into(),
        _ => {
            let to = (frame + 2 + r.below(2)).min(a.frames - 1);
            CelCmd::Tween {
                layer,
                from: frame,
                to,
                ease: Ease::InOut,
            }
            .into()
        }
    }
}

fn frame_cmd(a: &Aim, r: &mut Rng) -> Command {
    let (layer, frame, frames) = (a.id, a.frame, a.frames);
    let pair_end = (frame + r.below(2)).min(frames - 1);
    match r.below(7) {
        0 => FrameCmd::Add {
            at: r.below(frames + 1),
            copy: r.coin(),
        }
        .into(),
        1 => FrameCmd::Remove {
            from: frame,
            to: pair_end,
        }
        .into(),
        2 => FrameCmd::Move {
            from: frame,
            to: r.below(frames),
        }
        .into(),
        3 => FrameCmd::Duplicate {
            from: frame,
            to: pair_end,
        }
        .into(),
        4 => FrameCmd::HoldPrevious { layer, frame }.into(),
        5 => FrameCmd::HoldAcross {
            layer,
            source: frame,
            from: frame,
            to: r.below(frames),
        }
        .into(),
        _ => FrameCmd::Release {
            layer,
            from: frame,
            to: frame,
        }
        .into(),
    }
}

fn layer_cmd(a: &Aim, r: &mut Rng) -> Command {
    let layer = a.id;
    match r.below(10) {
        0 => LayerCmd::Remove { layer }.into(),
        1 => LayerCmd::Move {
            layer,
            to: r.below(a.layers),
        }
        .into(),
        2 => LayerCmd::Duplicate { layer }.into(),
        3 => LayerCmd::MergeDown { layer }.into(),
        4 => LayerCmd::Group { layer }.into(),
        5 => LayerCmd::Ungroup { layer }.into(),
        6 => {
            let mode = if r.coin() {
                ColorMode::Rgba
            } else {
                ColorMode::Palette
            };
            LayerCmd::SetMode { layer, mode }.into()
        }
        7 => LayerCmd::SetLocked {
            layer,
            locked: r.below(3) != 0,
        }
        .into(),
        8 => LayerCmd::PlaceReference {
            below: r.below(a.layers + 1),
            image: Arc::new(Image::blank()),
            name: "ref".into(),
        }
        .into(),
        _ => LayerCmd::Step {
            layer,
            up: r.coin(),
        }
        .into(),
    }
}

/// Puts the editor on the aimed cel, for commands that draw on the current one.
fn drawing_cmd(e: &mut Editor, a: &Aim, r: &mut Rng) -> Command {
    (e.layer, e.frame) = (a.layer, a.frame);
    if r.below(3) != 0 {
        let mut point = || (r.between(0, 14), r.between(0, 14));
        let path = vec![point(), point()];
        return DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        }
        .into();
    }
    let _ = e.apply(SelectionCmd::All);
    match r.below(3) {
        0 => FloatingCmd::Lift { copy: r.coin() }.into(),
        1 => FloatingCmd::Move {
            dx: 2,
            dy: 1,
            join: false,
        }
        .into(),
        _ => FloatingCmd::Anchor.into(),
    }
}

fn random_cmd(e: &mut Editor, r: &mut Rng) -> Command {
    let a = aim(e, r);
    match r.below(10) {
        0..=2 => cel_cmd(&a, r),
        3..=5 => drawing_cmd(e, &a, r),
        6 | 7 => frame_cmd(&a, r),
        8 => Command::Canvas(CanvasCmd::Resize {
            width: 16 + r.below(4),
            height: 16 + r.below(4),
            dx: r.between(-2, 2),
            dy: r.between(-2, 2),
        }),
        _ => layer_cmd(&a, r),
    }
}

/// True for a command that would change some layer's pixels or places, which a locked layer
/// must refuse. Inserting cels moves every layer's frames along, so it is left out.
fn draws(command: &Command) -> bool {
    let inserts = matches!(command, Command::Cel(CelCmd::Drag(d)) if d.landing == Landing::Insert);
    let drawing = matches!(
        command,
        Command::Draw(_) | Command::Floating(_) | Command::Cel(_)
    );
    drawing && !inserts
}

/// One random step, checked. The states seen are kept by undo depth for the walk back.
fn step(e: &mut Editor, r: &mut Rng, states: &mut HashMap<usize, Seen>) -> Result<(), String> {
    let command = random_cmd(e, r);
    let doc = e.doc();
    let locked: Vec<usize> = match draws(&command) {
        true => (0..doc.layers().len())
            .filter(|&i| doc.locked_through(i))
            .collect(),
        false => Vec::new(),
    };
    let before: Vec<_> = locked.iter().map(|&i| layer_seen(doc, i)).collect();
    let what = format!("{command:?}");
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = e.apply(command);
    }));
    ran.map_err(|_| format!("panicked on {what}"))?;
    if let Err(fault) = e.doc().check_links() {
        return Err(format!("{fault} after {what}"));
    }
    for (&index, before) in locked.iter().zip(&before) {
        if layer_seen(e.doc(), index) != *before {
            return Err(format!("locked layer {index} changed by {what}"));
        }
    }
    states.insert(e.history().undo_depth(), seen(e.doc()));
    Ok(())
}

/// Undoes every step, each state matching the one seen at that depth, then redoes them all.
fn walk_back_and_forth(e: &mut Editor, states: &HashMap<usize, Seen>) -> Result<(), String> {
    let last = seen(e.doc());
    let start = states.keys().min().copied().unwrap_or(0);
    while e.history().undo_depth() > start {
        let undone = e.apply(StepCmd::Undo).map(|o| o.changed);
        if undone != Ok(true) {
            break;
        }
        e.doc()
            .check_links()
            .map_err(|f| format!("{f} after undo"))?;
        let depth = e.history().undo_depth();
        if states.get(&depth) != Some(&seen(e.doc())) {
            return Err(format!("undo to depth {depth} differs"));
        }
    }
    while e.apply(StepCmd::Redo).map(|o| o.changed) == Ok(true) {
        e.doc()
            .check_links()
            .map_err(|f| format!("{f} after redo"))?;
    }
    match seen(e.doc()) == last {
        true => Ok(()),
        false => Err("redoing everything differs from the last state".into()),
    }
}

fn run_seed(seed: u64) -> Result<(), String> {
    let mut r = Rng::new(seed);
    let mut e = session();
    let mut states = HashMap::new();
    states.insert(e.history().undo_depth(), seen(e.doc()));
    for n in 0..STEPS {
        step(&mut e, &mut r, &mut states).map_err(|f| format!("seed {seed} step {n}: {f}"))?;
    }
    walk_back_and_forth(&mut e, &states).map_err(|f| format!("seed {seed}: {f}"))
}

fn run_seeds(count: u64) {
    let failures: Vec<String> = (1..=count).filter_map(|s| run_seed(s).err()).collect();
    assert!(
        failures.is_empty(),
        "{} of {count} seeds failed, first: {:?}",
        failures.len(),
        failures.first()
    );
}

#[test]
fn random_sessions_keep_the_links() {
    run_seeds(QUICK_SEEDS);
}

#[test]
#[ignore = "long: 2,000 sessions, or PIKSLAR_LINK_SEEDS"]
fn many_random_sessions_keep_the_links() {
    let count = std::env::var("PIKSLAR_LINK_SEEDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(LONG_SEEDS);
    run_seeds(count);
}
