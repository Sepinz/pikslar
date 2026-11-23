//! Running a list of operations as one transaction through the scripting API, and what a
//! batch or a session reports of it.

use crate::api_json;
use crate::fields::Refusal;
use crate::json::Value;
use crate::results::{Plan, Replies};
use pikslar_api::{ApiError, Op, Reply, API_VERSION};
use pikslar_core::Editor;

/// The most operations one batch or `run` request may hold.
pub const MAX_OPS: usize = 10_000;

/// One operation of a list, read and checked before any runs.
#[derive(Clone, Debug)]
pub struct Planned {
    op: Op,
    /// The operation as given, kept when it refers to an earlier result: it is read again with
    /// the real ids once those operations have run.
    referring: Option<Value>,
}

impl Planned {
    /// The operation with every reference to an earlier result filled in from `replies`.
    fn resolved(&self, replies: &[Reply]) -> Result<Op, Refusal> {
        match &self.referring {
            None => Ok(self.op.clone()),
            Some(value) => api_json::op(value, &Replies(replies)),
        }
    }
}

/// Every operation of a list, read before any runs, so a malformed one, or one that refers to
/// a result it cannot have, runs none.
pub fn read_ops(list: &[Value]) -> Result<Vec<Planned>, Refusal> {
    if list.len() > MAX_OPS {
        return Err(Refusal::new(
            "too_many_ops",
            format!(
                "{} operations given; at most {MAX_OPS} run at once",
                list.len()
            ),
        ));
    }
    let mut gives = Vec::with_capacity(list.len());
    let mut planned = Vec::with_capacity(list.len());
    for (at, value) in list.iter().enumerate() {
        let plan = Plan::new(&gives);
        let op = api_json::op(value, &plan).map_err(|r| r.at(at))?;
        let referring = plan.referred.get().then(|| value.clone());
        gives.push(api_json::gives(&op));
        planned.push(Planned { op, referring });
    }
    Ok(planned)
}

/// What a run did.
pub struct Ran {
    /// Each operation's reply, in order.
    pub results: Vec<Value>,
    /// False when the document is exactly as it was, so there is nothing to save.
    pub changed: bool,
}

fn version_refusal(version: i64) -> Refusal {
    Refusal::new(
        "version",
        format!(
            "API version {version} is not available; this Pikslar provides version {API_VERSION}"
        ),
    )
}

/// What one run may cost, so that no request can make a reply, or the work behind it, as
/// large as memory or time allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Colors the run's `pixels` operations may give back, all together.
    pub cells: u64,
    /// Pixels the run's operations may read or write, all together: a block counts its area,
    /// a fill the whole canvas, a line its length, a `put_pixels` its colors.
    pub work: u64,
}

/// The budget of every batch and every session `run`.
pub const BUDGET: Budget = Budget {
    cells: 1 << 20,
    work: 1 << 28,
};

/// What `op` costs: the cells it gives back and the pixels it works on.
fn cost(op: &Op, canvas: u64) -> (u64, u64) {
    let area = |w: i64, h: i64| (w.max(0) as u64).saturating_mul(h.max(0) as u64);
    match op {
        Op::Pixels { width, height, .. } => (area(*width, *height), area(*width, *height)),
        Op::FillRect { width, height, .. } => (0, area(*width, *height)),
        Op::PutPixels { colors, .. } => (0, colors.len() as u64),
        Op::DrawText { text, scale, .. } => (0, text_work(text, *scale)),
        Op::Fill { .. } => (0, canvas),
        Op::DrawLine { from, to, .. } => {
            let (dx, dy) = (from.0.abs_diff(to.0), from.1.abs_diff(to.1));
            (0, dx.max(dy).saturating_add(1))
        }
        _ => (0, 1),
    }
}

/// The pixels of the drawing a text draw makes, whether a letter lights them or not, as a
/// script is charged for it. A drawing too large to be made is refused and counts as one.
fn text_work(text: &str, scale: i64) -> u64 {
    use pikslar_core::glyphs::{self, Font, MAX_SCALE, MAX_TEXT_AREA};
    let scale = i32::try_from(scale.clamp(1, i64::from(MAX_SCALE))).unwrap_or(1);
    let area = glyphs::text_area(Font::Pikslar5x7, text, scale, 1);
    if area > MAX_TEXT_AREA {
        1
    } else {
        area as u64
    }
}

/// What a run has spent of its budget.
struct Spent {
    budget: Budget,
    cells: u64,
    work: u64,
}

impl Spent {
    /// Spends what `op` costs, or says why the run may not go on.
    fn charge(&mut self, op: &Op, canvas: u64) -> Result<(), Refusal> {
        let (cells, work) = cost(op, canvas);
        self.cells = self.cells.saturating_add(cells);
        self.work = self.work.saturating_add(work);
        let b = self.budget;
        if self.cells > b.cells {
            return Err(too_large(format!(
                "the run would give back more than {} pixel colors; read smaller blocks over \
                 several runs",
                b.cells
            )));
        }
        match self.work > b.work {
            true => Err(too_large(format!(
                "the run would work on more than {} pixels; split it into several runs",
                b.work
            ))),
            false => Ok(()),
        }
    }
}

fn too_large(message: String) -> Refusal {
    Refusal::new("too_large", message)
}

/// Runs `ops` asking for API `version`, as one undo step, within [`BUDGET`]. On a refusal
/// nothing remains of the run, and the refusal says which operation it was.
pub fn run(editor: &mut Editor, version: i64, ops: &[Planned]) -> Result<Ran, Refusal> {
    run_within(editor, version, ops, BUDGET)
}

/// [`run`] held to `budget`.
pub fn run_within(
    editor: &mut Editor,
    version: i64,
    ops: &[Planned],
    budget: Budget,
) -> Result<Ran, Refusal> {
    let version = u32::try_from(version).map_err(|_| version_refusal(version))?;
    let before = (editor.revision(), editor.revision_rewinds());
    let canvas = (editor.doc().width as u64).saturating_mul(editor.doc().height as u64);
    let mut spent = Spent {
        budget,
        cells: 0,
        work: 0,
    };
    let mut failed_at = None;
    let mut stopped = None;
    let replies = pikslar_api::run(editor, version, |script| {
        let mut replies = Vec::with_capacity(ops.len());
        for (at, planned) in ops.iter().enumerate() {
            failed_at = Some(at);
            let op = planned
                .resolved(&replies)
                .and_then(|op| spent.charge(&op, canvas).map(|()| op))
                .map_err(|r| {
                    stopped = Some(r);
                    // Only ends the run, which takes everything back; `stopped` is reported.
                    ApiError::Cancelled
                })?;
            replies.push(script.call(op)?);
        }
        failed_at = None;
        Ok::<_, ApiError>(replies)
    });
    let replies = replies.map_err(|e| {
        let refusal = stopped.unwrap_or_else(|| api_json::api_error(&e));
        match failed_at {
            Some(at) => refusal.at(at),
            None => refusal,
        }
    })?;
    Ok(Ran {
        results: replies.iter().map(api_json::reply).collect(),
        changed: (editor.revision(), editor.revision_rewinds()) != before,
    })
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
