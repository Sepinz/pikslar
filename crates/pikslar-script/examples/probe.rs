//! Measurements for `docs/scripting-language.md`. Reports only; records no baselines.
//!
//! cargo run --release -p pikslar-script --example probe -- bench
//! cargo run --release -p pikslar-script --example probe -- hostile   (each case in its own process)
//!
//! `hostile` reports each case's stop time and the process's peak private bytes over its
//! private bytes just before the run, against a 16 MiB cap.

use pikslar_script::{
    compile, run, Host, HostCx, HostError, HostId, HostTable, Limits, RunReport, Value,
    RECOMMENDED_STACK,
};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

const SIDE: i64 = 256;

struct Canvas {
    px: Vec<u32>,
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
        let (x, y) = (int(&a[0])?, int(&a[1])?);
        let inside = (0..SIDE).contains(&x) && (0..SIDE).contains(&y);
        let i = (y * SIDE + x) as usize;
        match id.0 {
            0 if inside => self.px[i] = int(&a[2])? as u32,
            1 => {
                let n = (int(&a[2])?.clamp(0, SIDE) * int(&a[3])?.clamp(0, SIDE)) as u64;
                cx.charge(1 + n / 16)?;
                self.px.fill(int(&a[4])? as u32);
            }
            _ => {}
        }
        Ok(Value::Nil)
    }
}

fn table() -> HostTable {
    let mut t = HostTable::new();
    let _ = t.add("set_pixel", 3);
    let _ = t.add("fill_rect", 5);
    t
}

fn run_src(src: &str, limits: &Limits, canvas: &mut Canvas) -> RunReport {
    match compile(src, &table(), limits) {
        Ok(p) => run(&p, canvas, limits, &AtomicBool::new(false)),
        Err(e) => RunReport {
            result: Err(e),
            fuel_used: 0,
            peak_memory: 0,
            stack_bytes: 0,
        },
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn timed(src: &str, reps: usize) -> (f64, RunReport) {
    let limits = Limits {
        fuel: u64::MAX,
        memory: 64 << 20,
        ..Limits::default()
    };
    let p = compile(src, &table(), &limits).unwrap_or_else(|e| panic!("{e}"));
    let mut canvas = Canvas {
        px: vec![0; (SIDE * SIDE) as usize],
    };
    let mut times = Vec::new();
    let mut last = None;
    for _ in 0..reps {
        let t = Instant::now();
        let r = run(&p, &mut canvas, &limits, &AtomicBool::new(false));
        times.push(t.elapsed().as_secs_f64() * 1e3);
        if let Err(e) = &r.result {
            panic!("{e}");
        }
        last = Some(r);
    }
    (median(times), last.unwrap_or_else(|| unreachable!()))
}

const PIXEL_WHILE: &str = "let y = 0; while y < 256 { let x = 0; while x < 256 { set_pixel(x, y, x + y * 256); x = x + 1; } y = y + 1; }";
const PIXEL_FOR: &str = "for y in 0..256 { for x in 0..256 { set_pixel(x, y, x + y * 256); } }";
const LIST: &str = "let l = []; for i in 0..100000 { push(l, i); } return len(l);";
const FILL: &str = "fill_rect(0, 0, 256, 256, 16711935);";

fn bench() {
    for (name, src, reps) in [
        ("pixel_while", PIXEL_WHILE, 11),
        ("pixel_for", PIXEL_FOR, 11),
        ("list_100k", LIST, 11),
        ("fill_rect", FILL, 201),
    ] {
        let (ms, r) = timed(src, reps);
        println!(
            "{name}_ms={ms:.3} fuel={} peak_memory={}",
            r.fuel_used, r.peak_memory
        );
    }
    let t = Instant::now();
    let p = compile(PIXEL_WHILE, &table(), &Limits::default());
    println!(
        "compile_pixel_us={:.1} ok={}",
        t.elapsed().as_secs_f64() * 1e6,
        p.is_ok()
    );
    let deep = "fn f(n) { if n == 0 { return 0; } return f(n - 1) + 1; } return f(250);";
    let limits = Limits {
        call_depth: 256,
        ..Limits::default()
    };
    let r = run_src(deep, &limits, &mut Canvas { px: Vec::new() });
    println!(
        "stack_250_calls={} B ok={}",
        r.stack_bytes,
        r.result.is_ok()
    );
}

const HOSTILE: [(&str, &str); 14] = [
    ("infinite loop", "while true { }"),
    ("loop in nested functions", "fn a() { b(); } fn b() { while true { let x = 1; } } a();"),
    ("string doubling", "let s = \"x\"; while true { s = s + s; }"),
    ("unbounded list", "let l = []; while true { push(l, 1); }"),
    ("many small strings", "let l = []; let i = 0; while true { push(l, str(i)); i = i + 1; }"),
    ("deep recursion", "fn f(n) { return f(n + 1); } f(0);"),
    ("frames each holding memory", "fn f(n) { let l = list(140000, n); f(n + 1); return len(l); } f(0);"),
    ("huge single allocation", "let s = repeat(\"ab\", 1000000000000);"),
    ("churn", "let b = repeat(\"x\", 1000); while true { let s = repeat(b, 1000); }"),
    ("small then large", "let l = []; for i in 0..50000 { push(l, str(i)); } l = nil; let s = repeat(\"x\", 15000000);"),
    ("search a long string", "let s = repeat(\"x\", 1000000); while true { let i = find(s, \"xxxxy\"); }"),
    ("quadratic list work", "let l = list(100000, 1); while true { insert(l, 0, 2); remove(l, 0); }"),
    ("bulk host work", "while true { fill_rect(0, 0, 256, 256, 1); }"),
    ("open a file", "let f = open(\"C:/Windows/win.ini\");"),
];

fn hostile_one(i: usize) {
    let Some((name, src)) = HOSTILE.get(i) else {
        return;
    };
    let limits = Limits {
        fuel: 10_000_000,
        memory: 16 << 20,
        ..Limits::default()
    };
    let before = memory::private();
    let t = Instant::now();
    let r = run_src(
        src,
        &limits,
        &mut Canvas {
            px: vec![0; (SIDE * SIDE) as usize],
        },
    );
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let peak = memory::peak_private().saturating_sub(before);
    let outcome = match &r.result {
        Ok(_) => "ran to completion".to_string(),
        Err(e) => format!("{:?}: {}", e.kind, e.message),
    };
    println!(
        "| {name} | {outcome} | {ms:.1} | {} | {:.2} |",
        peak / 1024,
        peak as f64 / (16 << 20) as f64
    );
}

fn hostile_all() {
    println!("| Script | Stopped by | ms | Peak private over start, KB | Of the cap |");
    println!("|---|---|---|---|---|");
    let Ok(me) = std::env::current_exe() else {
        return;
    };
    for i in 0..HOSTILE.len() {
        let out = std::process::Command::new(&me)
            .args(["hostile-one", &i.to_string()])
            .output();
        if let Ok(out) = out {
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).cloned().unwrap_or_default();
    let n = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(0);
    let worker = std::thread::Builder::new()
        .stack_size(RECOMMENDED_STACK)
        .spawn(move || match mode.as_str() {
            "bench" => bench(),
            "hostile" => hostile_all(),
            "hostile-one" => hostile_one(n),
            _ => eprintln!("modes: bench | hostile"),
        });
    if let Ok(w) = worker {
        let _ = w.join();
    }
}

#[cfg(windows)]
mod memory {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }

    fn read() -> Counters {
        let mut c = Counters {
            cb: std::mem::size_of::<Counters>() as u32,
            ..Counters::default()
        };
        // SAFETY: `c` is a correctly sized PROCESS_MEMORY_COUNTERS_EX that lives for the call.
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        c
    }

    pub fn private() -> usize {
        read().private
    }

    /// Peak committed private bytes over the process's life.
    pub fn peak_private() -> usize {
        read().peak_pagefile
    }
}

#[cfg(not(windows))]
mod memory {
    pub fn private() -> usize {
        0
    }

    pub fn peak_private() -> usize {
        0
    }
}
