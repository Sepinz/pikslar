//! Size probe: a tiny host that compiles and runs the script given as its argument, with a
//! `set_pixel` host function. Compare with `size_without`.

use pikslar_script::{compile, run, Host, HostCx, HostError, HostId, HostTable, Limits, Value};
use std::sync::atomic::AtomicBool;

struct Canvas(Vec<u32>);

impl Host for Canvas {
    fn call(
        &mut self,
        _id: HostId,
        a: &[Value],
        _cx: &mut HostCx<'_, '_>,
    ) -> Result<Value, HostError> {
        let n = |v: &Value| {
            v.as_int()
                .ok_or_else(|| HostError::new("expected an integer"))
        };
        let i = (n(&a[1])? * 256 + n(&a[0])?).rem_euclid(256 * 256) as usize;
        self.0[i] = n(&a[2])? as u32;
        Ok(Value::Nil)
    }
}

fn main() {
    let src = std::env::args().nth(1).unwrap_or_default();
    let mut px = Canvas(vec![0u32; 256 * 256]);
    let mut table = HostTable::new();
    let _ = table.add("set_pixel", 3);
    let limits = Limits::default();
    match compile(&src, &table, &limits) {
        Ok(p) => match run(&p, &mut px, &limits, &AtomicBool::new(false)).result {
            Ok(v) => println!("{v}"),
            Err(e) => println!("{e}"),
        },
        Err(e) => println!("{e}"),
    }
    println!("{}", px.0.iter().map(|&p| u64::from(p)).sum::<u64>());
}
