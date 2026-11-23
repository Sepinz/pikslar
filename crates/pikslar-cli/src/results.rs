//! References to an earlier operation's result in the same list: `{"result": 2}` is the layer
//! or frame operation 2 made, and `{"result": 0, "place": 1}` is the frame (or layer) at place
//! 1 of the `document` operation 0 gave back. A list is fixed before it runs, so this is how
//! it names a frame it makes, or an existing frame, whose handle is not known beforehand.
//!
//! Allowed only where a layer id or frame handle is expected, and only to an earlier
//! operation. Each reference is checked before any operation runs: what the operation it
//! names gives back must be what the field wants. Only a place past the end of a document's
//! list is found when the list runs, and it fails the run like any other refusal.

use crate::fields::{Fields, IntOr, Refusal};
use pikslar_api::Reply;

/// What a field holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wants {
    Layer,
    Frame,
}

/// What an operation gives back that a later one can refer to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gives {
    /// A layer id: `add_layer`, `duplicate_layer`.
    Layer,
    /// A frame handle: `add_frame`.
    Frame,
    /// The document's frame handles and layer ids, by place: `document`.
    Document,
    /// Nothing to refer to.
    Nothing,
}

/// One reference, as read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reference {
    pub op: usize,
    pub place: Option<usize>,
}

/// Where references are looked up.
pub trait Earlier {
    /// The id or handle `reference` names for a field that wants `wants`.
    fn resolve(&self, reference: Reference, wants: Wants) -> Result<i64, Refusal>;
}

/// The words for what a reference field may hold.
const WANTED: &str = "a whole number or {\"result\": n}";

fn bad(message: String) -> Refusal {
    Refusal::new("bad_result", message)
}

fn noun(wants: Wants) -> &'static str {
    match wants {
        Wants::Layer => "a layer id",
        Wants::Frame => "a frame handle",
    }
}

fn index(f: &mut Fields, key: &str, what: &str) -> Result<Option<usize>, Refusal> {
    match f.opt_int(key)? {
        None => Ok(None),
        Some(n) => usize::try_from(n)
            .map(Some)
            .map_err(|_| Refusal::bad(format!("\"{key}\" in {what} must be 0 or more, not {n}"))),
    }
}

fn reference(value: &crate::json::Value, what: &str) -> Result<Reference, Refusal> {
    let what = format!("a result reference in {what}");
    let mut f = Fields::of(value, &what)?;
    let op = index(&mut f, "result", &what)?
        .ok_or_else(|| Refusal::bad(format!("{what} needs \"result\"")))?;
    let place = index(&mut f, "place", &what)?;
    f.finish()?;
    Ok(Reference { op, place })
}

/// A layer id or frame handle given as a number, or as a reference resolved by `earlier`.
pub fn opt_handle(
    f: &mut Fields,
    key: &str,
    wants: Wants,
    earlier: &dyn Earlier,
) -> Result<Option<i64>, Refusal> {
    match f.opt_int_or_object(key, WANTED)? {
        None => Ok(None),
        Some(IntOr::Int(n)) => Ok(Some(n)),
        Some(IntOr::Object(v)) => {
            let found = reference(v, f.what())?;
            earlier.resolve(found, wants).map(Some)
        }
    }
}

pub fn handle(
    f: &mut Fields,
    key: &str,
    wants: Wants,
    earlier: &dyn Earlier,
) -> Result<i64, Refusal> {
    match f.int_or_object(key, WANTED)? {
        IntOr::Int(n) => Ok(n),
        IntOr::Object(v) => {
            let found = reference(v, f.what())?;
            earlier.resolve(found, wants)
        }
    }
}

/// Checks references before a list runs, against what each earlier operation gives back.
/// A reference resolves to 0 here: the operation is read again with the real value when it
/// runs.
pub struct Plan<'a> {
    /// What each earlier operation gives back: the reading operation is at `gives.len()`.
    pub gives: &'a [Gives],
    /// Set when the operation holds a reference.
    pub referred: std::cell::Cell<bool>,
}

impl<'a> Plan<'a> {
    pub fn new(gives: &'a [Gives]) -> Self {
        Plan {
            gives,
            referred: std::cell::Cell::new(false),
        }
    }
}

fn given(gives: Gives) -> &'static str {
    match gives {
        Gives::Layer => "a layer id",
        Gives::Frame => "a frame handle",
        Gives::Document => "a document",
        Gives::Nothing => "nothing to refer to",
    }
}

impl Earlier for Plan<'_> {
    fn resolve(&self, r: Reference, wants: Wants) -> Result<i64, Refusal> {
        self.referred.set(true);
        let Some(&gives) = self.gives.get(r.op) else {
            return Err(bad(format!(
                "this refers to the result of operation {}; only an earlier operation's                  result can be used",
                r.op
            )));
        };
        let why = match (gives, r.place) {
            (Gives::Document, Some(_)) => return Ok(0),
            (Gives::Layer, None) if wants == Wants::Layer => return Ok(0),
            (Gives::Frame, None) if wants == Wants::Frame => return Ok(0),
            (Gives::Document, None) => {
                "a document; name one of its frames or layers with \"place\"".into()
            }
            (Gives::Layer | Gives::Frame, Some(_)) => format!(
                "{}, so \"place\" is only for a document operation's result",
                given(gives)
            ),
            _ => given(gives).into(),
        };
        Err(bad(format!(
            "this wants {} from operation {}, which gives {why}",
            noun(wants),
            r.op
        )))
    }
}

/// Resolves references while a list runs, from the replies so far.
pub struct Replies<'a>(pub &'a [Reply]);

fn number(n: u64) -> Result<i64, Refusal> {
    i64::try_from(n).map_err(|_| bad(format!("{n} is too large to pass on")))
}

impl Earlier for Replies<'_> {
    fn resolve(&self, r: Reference, wants: Wants) -> Result<i64, Refusal> {
        let wrong = || {
            bad(format!(
                "operation {}'s result is not {}",
                r.op,
                noun(wants)
            ))
        };
        match (self.0.get(r.op), r.place, wants) {
            (Some(Reply::Created(id)), None, Wants::Layer) => Ok(i64::from(*id)),
            (Some(Reply::Frame(handle)), None, Wants::Frame) => number(*handle),
            (Some(Reply::Document(doc)), Some(place), _) => {
                let (list, what): (Vec<u64>, _) = match wants {
                    Wants::Frame => (doc.frames.clone(), "frames"),
                    Wants::Layer => (
                        doc.layers.iter().map(|&id| u64::from(id)).collect(),
                        "layers",
                    ),
                };
                let found = list.get(place).copied().ok_or_else(|| {
                    bad(format!(
                        "operation {}'s document has {} {what}; there is no place {place}",
                        r.op,
                        list.len()
                    ))
                })?;
                number(found)
            }
            _ => Err(wrong()),
        }
    }
}

#[cfg(test)]
#[path = "results_tests.rs"]
mod tests;
