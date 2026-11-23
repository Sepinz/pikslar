//! A grammar-aware script generator for the red team fuzzer: whole programs built from the
//! language's grammar and the binding's host functions, and mutations of a corpus of example
//! scripts that splice generated statements in, swap numbers for edge values and break
//! the source in small ways. Deterministic: a fixed xorshift sequence from a seed.
#![allow(dead_code)]

pub mod corpus;

use pikslar_bind::{functions, Capabilities, FunctionInfo};

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Numbers at the edges of every range the API and the interpreter check.
const NUMBERS: [&str; 26] = [
    "0",
    "1",
    "-1",
    "2",
    "3",
    "7",
    "16",
    "23",
    "24",
    "31",
    "32",
    "63",
    "64",
    "255",
    "256",
    "65535",
    "65536",
    "4294967295",
    "4294967296",
    "4278190335",
    "16777215",
    "9223372036854775807",
    "(-9223372036854775807 - 1)",
    "1099511627776",
    "0.5",
    "-2.5",
];

const STRINGS: [&str; 8] = [
    "\"\"",
    "\"a\"",
    "\"Normal\"",
    "\"Multiply\"",
    "\"layer\"",
    "\" \"",
    "\"\\n\"",
    "\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
];

/// Values that name real objects of the test project, or nearly.
const HANDLES: [&str; 8] = [
    "layers()[0]",
    "layers()[1]",
    "frames()[0]",
    "frames()[1]",
    "layers()[len(layers()) - 1]",
    "frames()[len(frames()) - 1]",
    "swatches()[0]",
    "(layers()[0] + 100)",
];

const BUILTINS: [(&str, usize); 19] = [
    ("len", 1),
    ("push", 2),
    ("pop", 1),
    ("insert", 3),
    ("remove", 2),
    ("slice", 3),
    ("find", 2),
    ("repeat", 2),
    ("list", 2),
    ("str", 1),
    ("int", 1),
    ("float", 1),
    ("type", 1),
    ("abs", 1),
    ("min", 2),
    ("max", 2),
    ("floor", 1),
    ("round", 1),
    ("sqrt", 1),
];

const BINARY: [&str; 13] = [
    "+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">", ">=", "&&", "||",
];

/// What a generated program may name: its variables, and the host functions of its run.
pub struct Scope {
    pub vars: Vec<String>,
    pub hosts: Vec<FunctionInfo>,
    pub depth: usize,
    /// Loops around the statement being made, so `break` is only made inside one.
    pub loops: usize,
}

impl Scope {
    pub fn new(caps: Capabilities) -> Scope {
        let hosts = functions()
            .into_iter()
            .filter(|f| f.needs.is_none_or(|c| caps.has(c)))
            .collect();
        Scope {
            vars: Vec::new(),
            hosts,
            depth: 0,
            loops: 0,
        }
    }
}

/// One expression, at most `budget` levels deep.
pub fn expr(rng: &mut Rng, scope: &Scope, budget: usize) -> String {
    let leaf = budget == 0 || rng.chance(35);
    if leaf {
        return atom(rng, scope);
    }
    match rng.below(7) {
        0 | 1 => {
            let op = rng.pick(&BINARY);
            let (l, r) = (expr(rng, scope, budget - 1), expr(rng, scope, budget - 1));
            format!("({l} {op} {r})")
        }
        2 => format!("-{}", expr(rng, scope, budget - 1)),
        3 => format!("!{}", expr(rng, scope, budget - 1)),
        4 => {
            let items: Vec<String> = (0..rng.below(5))
                .map(|_| expr(rng, scope, budget - 1))
                .collect();
            format!("[{}]", items.join(", "))
        }
        5 => call(rng, scope, budget - 1),
        _ => format!(
            "{}[{}]",
            list_atom(rng, scope),
            expr(rng, scope, budget - 1)
        ),
    }
}

/// Something that is usually a list.
fn list_atom(rng: &mut Rng, scope: &Scope) -> String {
    let has_handles = scope.hosts.iter().any(|f| f.name == "layers");
    match rng.below(4) {
        0 if has_handles => rng
            .pick(&["layers()", "frames()", "swatches()"])
            .to_string(),
        1 if !scope.vars.is_empty() => rng.pick(&scope.vars).clone(),
        _ => "[1, 2, 3]".to_string(),
    }
}

/// A condition: usually a comparison, so it is a boolean.
pub fn cond(rng: &mut Rng, scope: &Scope, budget: usize) -> String {
    match rng.chance(80) {
        true => {
            let op = rng.pick(&["==", "!=", "<", "<=", ">", ">="]);
            let (l, r) = (expr(rng, scope, budget), expr(rng, scope, budget));
            format!("{l} {op} {r}")
        }
        false => expr(rng, scope, budget),
    }
}

fn atom(rng: &mut Rng, scope: &Scope) -> String {
    match rng.below(10) {
        0..=3 => rng.pick(&NUMBERS).to_string(),
        4 => rng.pick(&STRINGS).to_string(),
        5 => rng.pick(&["true", "false", "nil"]).to_string(),
        6 if !scope.hosts.iter().any(|f| f.name == "layers") => "1".to_string(),
        6 => rng.pick(&HANDLES).to_string(),
        _ if scope.vars.is_empty() => "0".to_string(),
        _ => rng.pick(&scope.vars).clone(),
    }
}

/// A call to a builtin or a host function, with the right number of arguments.
fn call(rng: &mut Rng, scope: &Scope, budget: usize) -> String {
    let (name, arity) = match rng.chance(60) && !scope.hosts.is_empty() {
        true => {
            let f = rng.pick(&scope.hosts);
            (f.name, f.arity)
        }
        false => *rng.pick(&BUILTINS),
    };
    let args: Vec<String> = (0..arity).map(|_| arg(rng, scope, budget)).collect();
    format!("{name}({})", args.join(", "))
}

fn arg(rng: &mut Rng, scope: &Scope, budget: usize) -> String {
    let has_handles = scope.hosts.iter().any(|f| f.name == "layers");
    match rng.below(4) {
        0 if has_handles => rng.pick(&HANDLES).to_string(),
        1 => expr(rng, scope, budget),
        _ => atom(rng, scope),
    }
}

/// One statement. Loops are bounded or left to the fuel.
pub fn statement(rng: &mut Rng, scope: &mut Scope, budget: usize) -> String {
    let e = expr(rng, scope, budget);
    let nested = scope.depth < 4 && budget > 0;
    match rng.below(10) {
        0 | 1 => {
            let name = format!("v{}", scope.vars.len());
            scope.vars.push(name.clone());
            format!("let {name} = {e};")
        }
        2 if !scope.vars.is_empty() => format!("{} = {e};", rng.pick(&scope.vars).clone()),
        3 if !scope.vars.is_empty() => {
            let v = rng.pick(&scope.vars).clone();
            format!("{v}[{}] = {e};", expr(rng, scope, 1))
        }
        4 if nested => {
            let c = cond(rng, scope, budget);
            let then = block(rng, scope, budget, false);
            format!("if {c} {then} else {}", block(rng, scope, budget, false))
        }
        5 if nested => {
            let c = cond(rng, scope, budget);
            format!("while {c} {}", block(rng, scope, budget, true))
        }
        6 if nested => {
            let n = rng.pick(&["0", "1", "3", "64", "1000", "9223372036854775807"]);
            {
                let d = scope.depth;
                format!("for i{d} in 0..{n} {}", block(rng, scope, budget, true))
            }
        }
        7 if nested => {
            let (d, l) = (scope.depth, list_atom(rng, scope));
            format!("for it{d} in {l} {}", block(rng, scope, budget, true))
        }
        8 if scope.loops > 0 => rng.pick(&["break;", "continue;", "return 1;"]).to_string(),
        8 => "return 1;".to_string(),
        _ => format!("{};", call(rng, scope, budget)),
    }
}

fn block(rng: &mut Rng, scope: &mut Scope, budget: usize, is_loop: bool) -> String {
    let mark = scope.vars.len();
    scope.depth += 1;
    scope.loops += usize::from(is_loop);
    let body: Vec<String> = (0..rng.below(4))
        .map(|_| statement(rng, scope, budget.saturating_sub(1)))
        .collect();
    scope.depth -= 1;
    scope.loops -= usize::from(is_loop);
    scope.vars.truncate(mark);
    format!("{{ {} }}", body.join(" "))
}

/// A whole program: maybe a function or two, then statements.
pub fn program(rng: &mut Rng, caps: Capabilities) -> String {
    let mut scope = Scope::new(caps);
    let mut out = String::new();
    if rng.chance(30) {
        out.push_str("fn rec(n) { if n > 0 { return rec(n - 1) + 1; } return 0; }\n");
    }
    let depth = rng.pick(&[1, 2, 3, 5]);
    for _ in 0..1 + rng.below(12) {
        out.push_str(&statement(rng, &mut scope, *depth));
        out.push('\n');
    }
    if rng.chance(20) {
        out.push_str("return rec(1000);\n");
    }
    out.replace(
        "rec(",
        if out.starts_with("fn rec") {
            "rec("
        } else {
            "abs("
        },
    )
}
