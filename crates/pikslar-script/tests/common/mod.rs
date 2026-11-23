//! Shared helpers: a small canvas host like the one the spike measured, and short runners.
#![allow(dead_code)]

use pikslar_script::{
    compile, run, Host, HostCx, HostError, HostId, HostTable, Limits, RunReport, ScriptError, Value,
};
use std::sync::atomic::AtomicBool;

pub const W: i64 = 256;
pub const H: i64 = 256;

/// A 256 x 256 canvas with `get_pixel`, `set_pixel`, `fill_rect` and `print`.
pub struct Canvas {
    pub px: Vec<u32>,
    pub printed: Vec<String>,
}

pub const PRINT: HostId = HostId(0);
pub const GET: HostId = HostId(1);
pub const SET: HostId = HostId(2);
pub const FILL: HostId = HostId(3);

pub fn table() -> HostTable {
    let mut t = HostTable::new();
    for (name, arity) in [
        ("print", 1),
        ("get_pixel", 2),
        ("set_pixel", 3),
        ("fill_rect", 5),
    ] {
        t.add(name, arity).expect("valid name");
    }
    t
}

impl Canvas {
    pub fn new() -> Canvas {
        Canvas {
            px: vec![0; (W * H) as usize],
            printed: Vec::new(),
        }
    }

    fn at(x: i64, y: i64) -> Option<usize> {
        ((0..W).contains(&x) && (0..H).contains(&y)).then(|| (y * W + x) as usize)
    }
}

fn int(v: &Value) -> Result<i64, HostError> {
    v.as_int()
        .ok_or_else(|| HostError::new("expected an integer"))
}

impl Host for Canvas {
    fn call(
        &mut self,
        id: HostId,
        a: &[Value],
        cx: &mut HostCx<'_, '_>,
    ) -> Result<Value, HostError> {
        match id {
            PRINT => self.printed.push(a[0].to_string()),
            GET => {
                let i = Canvas::at(int(&a[0])?, int(&a[1])?);
                return Ok(Value::Int(i.map_or(0, |i| i64::from(self.px[i]))));
            }
            SET => {
                if let Some(i) = Canvas::at(int(&a[0])?, int(&a[1])?) {
                    self.px[i] = int(&a[2])? as u32;
                }
            }
            FILL => {
                let (x, y, w, h, c) = (
                    int(&a[0])?,
                    int(&a[1])?,
                    int(&a[2])?,
                    int(&a[3])?,
                    int(&a[4])?,
                );
                let x0 = x.clamp(0, W);
                let x1 = x.saturating_add(w).clamp(0, W);
                let y0 = y.clamp(0, H);
                let y1 = y.saturating_add(h).clamp(0, H);
                // The host pays for its own work: one unit per pixel row of 16.
                let area = ((x1 - x0).max(0) * (y1 - y0).max(0)) as u64;
                cx.charge(1 + area / 16)?;
                for yy in y0..y1 {
                    let row = (yy * W) as usize;
                    self.px[row + x0 as usize..row + x1 as usize].fill(c as u32);
                }
            }
            _ => return Err(HostError::new("unknown host function")),
        }
        Ok(Value::Nil)
    }
}

pub fn report_with(
    src: &str,
    limits: &Limits,
    host: &mut Canvas,
    cancel: &AtomicBool,
) -> RunReport {
    match compile(src, &table(), limits) {
        Ok(p) => run(&p, host, limits, cancel),
        Err(e) => RunReport {
            result: Err(e),
            fuel_used: 0,
            peak_memory: 0,
            stack_bytes: 0,
        },
    }
}

pub fn report(src: &str, limits: &Limits) -> RunReport {
    report_with(src, limits, &mut Canvas::new(), &AtomicBool::new(false))
}

/// Runs `src` with default limits and returns what it returned.
pub fn eval(src: &str) -> Result<Value, ScriptError> {
    report(src, &Limits::default()).result
}

/// Runs `src` and returns what it printed.
pub fn printed(src: &str) -> Vec<String> {
    let mut c = Canvas::new();
    let r = report_with(src, &Limits::default(), &mut c, &AtomicBool::new(false));
    if let Err(e) = r.result {
        panic!("script failed: {e}");
    }
    c.printed
}

pub fn int_of(src: &str) -> i64 {
    match eval(src) {
        Ok(Value::Int(n)) => n,
        other => panic!("expected an integer from {src:?}, got {other:?}"),
    }
}

pub fn str_of(src: &str) -> String {
    match eval(src) {
        Ok(v) => v.to_string(),
        Err(e) => panic!("{src:?} failed: {e}"),
    }
}

pub fn err_of(src: &str) -> ScriptError {
    match eval(src) {
        Ok(v) => panic!("expected an error from {src:?}, got {v:?}"),
        Err(e) => e,
    }
}

/// Runs `f` on a thread with the recommended stack, as a host would.
pub fn on_script_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(pikslar_script::RECOMMENDED_STACK)
        .spawn(f)
        .expect("thread starts")
        .join()
        .expect("thread finishes")
}
