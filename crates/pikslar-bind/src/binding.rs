//! The host the interpreter calls: each host function, given the run's API script.

use crate::functions::Func;
use crate::pending::{Cel, Pending};
use crate::values::refusal;
use pikslar_api::{ApiError, Op, Reply, Script};
use pikslar_script::{Host, HostCx, HostError, HostId, Value};

/// At most this much `print` output is kept for one run.
pub const MOST_PRINTED: usize = 64 << 10;

/// The call a refusal is put down to when it was writing pixels `set_pixel` kept waiting.
pub const WAITING: &str = "set_pixel (waiting pixels)";

pub struct Binding<'s, 'a> {
    script: &'s mut Script<'a>,
    fns: Vec<Func>,
    pub(crate) pending: Pending,
    /// Why the API refused the operation that ended the run, if it did.
    pub(crate) refused: Option<ApiError>,
    /// True when that refusal was of pixels kept waiting, written by a later call.
    pub(crate) waiting_refused: bool,
    pub(crate) printed: String,
    /// The files the run may ask the user for, if the host gave a picker.
    pub(crate) files: Option<crate::files::FileAccess>,
    /// Files read and writes waiting for the end of the run, held as script values.
    pub(crate) held: crate::files::Held,
    /// How much the run has grown the document.
    pub(crate) growth: crate::growth::Growth,
}

impl Host for Binding<'_, '_> {
    fn call(
        &mut self,
        id: HostId,
        args: &[Value],
        cx: &mut HostCx<'_, '_>,
    ) -> Result<Value, HostError> {
        let func = self
            .fns
            .get(usize::from(id.0))
            .copied()
            .ok_or_else(|| HostError::new("no such host function"))?;
        let args = crate::values::Args {
            name: func.name,
            values: args,
        };
        (func.run)(self, &args, cx)
    }
}

impl<'s, 'a> Binding<'s, 'a> {
    pub fn new(script: &'s mut Script<'a>, fns: Vec<Func>) -> Self {
        Binding {
            script,
            fns,
            pending: Pending::default(),
            refused: None,
            waiting_refused: false,
            printed: String::new(),
            files: None,
            held: crate::files::Held::default(),
            growth: crate::growth::Growth::default(),
        }
    }

    /// The document's width, height, layer count and frame count, for pricing.
    pub fn size(&self) -> (usize, usize, usize, usize) {
        self.script.size()
    }

    pub(crate) fn script_held(&self) -> usize {
        self.script.held()
    }

    pub(crate) fn script_held_cost(&self) -> usize {
        self.script.held_cost()
    }

    /// How many cels duplicating this layer copies, for pricing the copy.
    pub fn cels_copied(&self, layer: i64) -> Option<usize> {
        self.script.cels_copied(layer)
    }

    /// How many swatches the document has, for pricing reads of them.
    pub fn swatch_count(&self) -> usize {
        self.script.swatch_count()
    }

    /// How many colors the document's table holds, for pricing palette edits.
    pub fn colors(&self) -> usize {
        self.script.colors()
    }

    /// Keeps why the API refused, for the run's outcome, and gives the script its error.
    pub fn refuse(&mut self, name: &str, error: ApiError) -> HostError {
        let host = refusal(name, &error);
        self.refused = Some(error);
        host
    }

    /// Runs an operation after the waiting pixels are written, as every operation that could
    /// see or change them must.
    pub fn op(&mut self, name: &str, op: Op) -> Result<Reply, HostError> {
        self.flush()?;
        self.pending.forget();
        self.op_now(name, op)
    }

    /// Runs an operation that neither reads pixels or swatches nor changes anything, so
    /// waiting pixels can go on waiting.
    pub fn op_now(&mut self, name: &str, op: Op) -> Result<Reply, HostError> {
        match self.script.call(op) {
            Ok(reply) => Ok(reply),
            Err(error) => Err(self.refuse(name, error)),
        }
    }

    /// Writes the waiting pixels, one `PutPixels` a block. Their fuel was burnt as they were
    /// set. The cel stays known, so more pixels can wait for it. A refusal is put down to
    /// [`WAITING`], not to the call that happened to write them.
    pub fn flush(&mut self) -> Result<(), HostError> {
        let Some((cel, blocks)) = self.pending.take() else {
            return Ok(());
        };
        for b in blocks {
            let put = Op::PutPixels {
                layer: cel.layer,
                frame: cel.frame,
                x: crate::values::as_i64(b.x),
                y: crate::values::as_i64(b.y),
                width: crate::values::as_i64(b.width),
                colors: b.colors,
            };
            if let Err(e) = self.op_now(WAITING, put) {
                self.waiting_refused = true;
                return Err(e);
            }
        }
        Ok(())
    }

    /// Checks that a cel exists and can be drawn on, and starts keeping pixels for it: a
    /// locked layer, or one that is not a paint layer, is refused here, at the `set_pixel`
    /// call, not later when the pixels are written. Anything that could change that flushes
    /// and forgets the cel first.
    pub fn start_cel(&mut self, name: &str, layer: i64, frame: i64) -> Result<Cel, HostError> {
        self.flush()?;
        if let Err(error) = self.script.draw_refusal(layer, frame) {
            return Err(self.refuse(name, error));
        }
        let (width, height, _, _) = self.size();
        let cel = Cel {
            layer,
            frame,
            width,
            height,
        };
        self.pending.start(cel);
        Ok(cel)
    }

    /// Adds `text` to what the run printed, as far as [`MOST_PRINTED`].
    pub fn print(&mut self, text: &str) {
        let room = MOST_PRINTED.saturating_sub(self.printed.len());
        let mut end = text.len().min(room);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.printed.push_str(&text[..end]);
    }
}
