//! Runs a compiled program by walking its tree. Locals live in one value stack, charged by
//! capacity against the memory cap; each call's frame is a window into it.

use crate::ast::{Call, Expr, ForEach, ForRange, Program, Stmt, StmtKind, Target, UnOp, While};
use crate::builtins::{self, want_int, want_list, Cx};
use crate::error::{ErrorKind, Fault, Pos, ScriptError, Trap};
use crate::fuel::Fuel;
use crate::host::{Host, HostCx, HostId};
use crate::limits::{Limits, STACK_BUDGET};
use crate::meter::Meter;
use crate::ops;
use crate::value::{index_fault, List, Str, Value};
use std::mem::size_of;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

/// The outcome of one run, with what it used.
#[derive(Debug)]
pub struct RunReport {
    pub result: Result<Value, ScriptError>,
    /// Fuel burnt, including by host calls.
    pub fuel_used: u64,
    /// Highest memory charged at once, in bytes, including the program itself.
    pub peak_memory: usize,
    /// Deepest native stack the run reached, in bytes, measured at each block it entered.
    pub stack_bytes: usize,
}

/// Runs `program`, calling `host` for host functions. The run stops with an error when its
/// fuel runs out, its memory would pass the cap, calls nest too deeply, or `cancel` is set.
pub fn run<H: Host + ?Sized>(
    program: &Program,
    host: &mut H,
    limits: &Limits,
    cancel: &AtomicBool,
) -> RunReport {
    let meter = Meter::new(limits.memory);
    let marker = 0u8;
    let mut vm = Vm {
        prog: program,
        host,
        stack: Vec::new(),
        stack_charge: 0,
        texts: Vec::new(),
        meter: meter.clone(),
        fuel: Fuel::new(limits.fuel, cancel),
        depth: 0,
        max_depth: limits.depth(),
        ret: Value::Nil,
        stack_top: address(&marker),
        stack_used: 0,
    };
    let result = vm.start().map_err(|e| *e);
    let (fuel_used, stack_bytes) = (vm.fuel.used(), vm.stack_used);
    drop(vm);
    RunReport {
        result,
        fuel_used,
        peak_memory: meter.peak(),
        stack_bytes,
    }
}

fn address(marker: &u8) -> usize {
    std::hint::black_box(marker as *const u8) as usize
}

enum Flow {
    Normal,
    Break,
    Continue,
    Return,
}

struct Vm<'p, 'h, 'c, H: ?Sized> {
    prog: &'p Program,
    host: &'h mut H,
    stack: Vec<Value>,
    stack_charge: usize,
    /// This run's value of each string literal, made the first time it is used.
    texts: Vec<Option<Str>>,
    meter: Rc<Meter>,
    fuel: Fuel<'c>,
    depth: u32,
    max_depth: u32,
    ret: Value,
    stack_top: usize,
    stack_used: usize,
}

impl<'p, H: Host + ?Sized> Vm<'p, '_, '_, H> {
    fn start(&mut self) -> Result<Value, Trap> {
        let n = self.prog.texts.len();
        let texts = n.saturating_mul(size_of::<Option<Str>>());
        self.meter
            .charge(self.prog.charge)
            .and_then(|()| self.meter.charge_alloc(texts).map(|_| ()))
            .map_err(|f| f.at(Pos::START))?;
        if self.texts.try_reserve_exact(n).is_err() {
            return Err(Fault::alloc_failed().at(Pos::START));
        }
        self.texts.resize(n, None);
        self.enter(0, 0, Pos::START)
    }

    /// The value of string literal `id`, made and charged on first use in this run.
    fn text(&mut self, id: u32, pos: Pos) -> Result<Value, Trap> {
        let id = id as usize;
        if let Some(Some(s)) = self.texts.get(id) {
            return Ok(Value::Str(s.clone()));
        }
        let text = &self.prog.texts[id];
        self.fuel
            .burn(crate::limits::byte_fuel(text.len()))
            .map_err(|f| f.at(pos))?;
        let s = Str::concat(&self.meter, &[text]).map_err(|f| f.at(pos))?;
        self.texts[id] = Some(s.clone());
        Ok(Value::Str(s))
    }

    /// Grows the value stack, charging the new buffer while the old one is still charged.
    fn grow_stack(&mut self) -> Result<(), Fault> {
        let len = self.stack.len();
        let cap = (len + 1)
            .max(self.stack.capacity().saturating_mul(2))
            .max(64);
        let bytes = cap
            .checked_mul(size_of::<Value>())
            .ok_or_else(Fault::memory)?;
        let charge = self.meter.charge_alloc(bytes)?;
        if self.stack.try_reserve_exact(cap - len).is_err() {
            self.meter.refund(charge);
            return Err(Fault::alloc_failed());
        }
        self.meter
            .refund(std::mem::replace(&mut self.stack_charge, charge));
        Ok(())
    }

    #[inline]
    fn push(&mut self, v: Value, pos: Pos) -> Result<(), Trap> {
        if self.stack.len() == self.stack.capacity() {
            self.grow_stack().map_err(|f| f.at(pos))?;
        }
        self.stack.push(v);
        Ok(())
    }

    /// Runs function `f` whose arguments are already on the stack from `base`.
    fn enter(&mut self, f: u32, base: usize, pos: Pos) -> Result<Value, Trap> {
        let prog = self.prog;
        let def = &prog.functions[f as usize];
        for _ in def.arity..def.slots {
            self.push(Value::Nil, pos)?;
        }
        let flow = self.block(&def.body, base)?;
        self.stack.truncate(base);
        Ok(match flow {
            Flow::Return => std::mem::take(&mut self.ret),
            _ => Value::Nil,
        })
    }

    /// How far the native stack has grown since the run began.
    fn stack_depth(&self) -> usize {
        let marker = 0u8;
        self.stack_top.saturating_sub(address(&marker))
    }

    fn block(&mut self, body: &'p [Stmt], base: usize) -> Result<Flow, Trap> {
        self.stack_used = self.stack_used.max(self.stack_depth());
        for s in body {
            self.fuel.burn(u64::from(s.cost)).map_err(|f| f.at(s.pos))?;
            match self.stmt(s, base)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn stmt(&mut self, s: &'p Stmt, base: usize) -> Result<Flow, Trap> {
        match &s.kind {
            StmtKind::Set(slot, e) => {
                let v = self.eval(e, base)?;
                self.stack[base + usize::from(*slot)] = v;
            }
            StmtKind::SetIndex(slot, pair) => {
                self.set_index(base + usize::from(*slot), pair, base, s.pos)?
            }
            StmtKind::Expr(e) => {
                self.eval(e, base)?;
            }
            StmtKind::If(i) => {
                let branch = if self.cond(&i.cond, base)? {
                    &i.then
                } else {
                    &i.other
                };
                return self.block(branch, base);
            }
            StmtKind::While(w) => return self.while_loop(w, base),
            StmtKind::ForRange(f) => return self.for_range(f, base),
            StmtKind::ForEach(f) => return self.for_each(f, base),
            StmtKind::Break => return Ok(Flow::Break),
            StmtKind::Continue => return Ok(Flow::Continue),
            StmtKind::Return(e) => {
                self.ret = match e {
                    Some(e) => self.eval(e, base)?,
                    None => Value::Nil,
                };
                return Ok(Flow::Return);
            }
        }
        Ok(Flow::Normal)
    }

    fn set_index(
        &mut self,
        at: usize,
        pair: &'p (Expr, Expr),
        base: usize,
        pos: Pos,
    ) -> Result<(), Trap> {
        let index = self.eval(&pair.0, base)?;
        let value = self.eval(&pair.1, base)?;
        let list = want_list(&self.stack[at]).map_err(|f| f.at(pos))?;
        let i = list_index(&index, list).map_err(|f| f.at(pair.0.pos()))?;
        list.set(i, value).map_err(|f| f.at(pos))
    }

    /// A loop body's flow: `Some` ends the loop with that flow.
    fn body(&mut self, body: &'p [Stmt], base: usize) -> Result<Option<Flow>, Trap> {
        Ok(match self.block(body, base)? {
            Flow::Break => Some(Flow::Normal),
            Flow::Return => Some(Flow::Return),
            Flow::Normal | Flow::Continue => None,
        })
    }

    fn while_loop(&mut self, w: &'p While, base: usize) -> Result<Flow, Trap> {
        loop {
            let pos = w.cond.pos();
            self.fuel
                .burn(u64::from(w.test_cost))
                .map_err(|f| f.at(pos))?;
            if !self.cond(&w.cond, base)? {
                return Ok(Flow::Normal);
            }
            if let Some(flow) = self.body(&w.body, base)? {
                return Ok(flow);
            }
        }
    }

    fn for_range(&mut self, f: &'p ForRange, base: usize) -> Result<Flow, Trap> {
        let from = self.eval(&f.from, base)?;
        let from = want_int(&from).map_err(|e| e.at(f.from.pos()))?;
        let to = self.eval(&f.to, base)?;
        let to = want_int(&to).map_err(|e| e.at(f.to.pos()))?;
        let slot = base + usize::from(f.var);
        let mut i = from;
        while i < to {
            self.fuel.burn(1).map_err(|e| e.at(f.from.pos()))?;
            self.stack[slot] = Value::Int(i);
            if let Some(flow) = self.body(&f.body, base)? {
                return Ok(flow);
            }
            i += 1;
        }
        Ok(Flow::Normal)
    }

    /// Visits items by place, reading the length afresh each turn, so a body that changes
    /// the list sees its changes.
    fn for_each(&mut self, f: &'p ForEach, base: usize) -> Result<Flow, Trap> {
        let list = self.eval(&f.list, base)?;
        let list = want_list(&list).map_err(|e| e.at(f.list.pos()))?.clone();
        let slot = base + usize::from(f.var);
        let mut i = 0;
        while let Some(v) = list.get(i) {
            self.fuel.burn(1).map_err(|e| e.at(f.list.pos()))?;
            self.stack[slot] = v;
            if let Some(flow) = self.body(&f.body, base)? {
                return Ok(flow);
            }
            i += 1;
        }
        Ok(Flow::Normal)
    }

    fn cond(&mut self, e: &'p Expr, base: usize) -> Result<bool, Trap> {
        match self.eval(e, base)? {
            Value::Bool(b) => Ok(b),
            other => {
                let msg = format!("expected true or false, not {}", other.type_name());
                Err(Fault::type_error(msg).at(e.pos()))
            }
        }
    }

    fn eval(&mut self, e: &'p Expr, base: usize) -> Result<Value, Trap> {
        match e {
            Expr::Lit(v, _) => Ok(v.value()),
            Expr::Text(id, pos) => self.text(*id, *pos),
            Expr::Local(slot, _) => Ok(self.stack[base + usize::from(*slot)].clone()),
            Expr::Binary(op, pair, pos) => {
                let l = self.eval(&pair.0, base)?;
                let r = self.eval(&pair.1, base)?;
                ops::binary(*op, l, r, &mut self.fuel, &self.meter).map_err(|f| f.at(*pos))
            }
            Expr::And(pair, _) => Ok(Value::Bool(
                self.cond(&pair.0, base)? && self.cond(&pair.1, base)?,
            )),
            Expr::Or(pair, _) => Ok(Value::Bool(
                self.cond(&pair.0, base)? || self.cond(&pair.1, base)?,
            )),
            Expr::Unary(op, inner, pos) => self.unary(*op, inner, base, *pos),
            Expr::Index(pair, pos) => {
                let list = self.eval(&pair.0, base)?;
                let index = self.eval(&pair.1, base)?;
                let list = want_list(&list).map_err(|f| f.at(*pos))?;
                let i = list_index(&index, list).map_err(|f| f.at(pair.1.pos()))?;
                list.get(i).ok_or_else(|| index_fault().at(*pos))
            }
            Expr::List(items, pos) => self.list(items, base, *pos),
            Expr::Call(c) => self.call(c, base),
        }
    }

    fn unary(&mut self, op: UnOp, inner: &'p Expr, base: usize, pos: Pos) -> Result<Value, Trap> {
        if op == UnOp::Not {
            return Ok(Value::Bool(!self.cond(inner, base)?));
        }
        match self.eval(inner, base)? {
            Value::Int(n) => n
                .checked_neg()
                .map(Value::Int)
                .ok_or_else(|| Fault::overflow().at(pos)),
            Value::Float(x) => Ok(Value::Float(-x)),
            other => Err(Fault::type_error(format!("cannot negate {}", other.type_name())).at(pos)),
        }
    }

    fn list(&mut self, items: &'p [Expr], base: usize, pos: Pos) -> Result<Value, Trap> {
        let list = List::with_capacity(&self.meter, items.len()).map_err(|f| f.at(pos))?;
        for item in items {
            let v = self.eval(item, base)?;
            list.push(v).map_err(|f| f.at(item.pos()))?;
        }
        Ok(Value::List(list))
    }

    fn call(&mut self, c: &'p Call, base: usize) -> Result<Value, Trap> {
        let args = self.stack.len();
        for a in &c.args {
            let v = self.eval(a, base)?;
            self.push(v, c.pos)?;
        }
        let out = match c.target {
            Target::User(f) => {
                if self.depth >= self.max_depth || self.stack_depth() > STACK_BUDGET {
                    let f = Fault::new(ErrorKind::CallDepth, "calls are nested too deeply");
                    return Err(f.at(c.pos));
                }
                // The locals a call fills with `nil`, and drops on return, cost a unit each: a
                // function of 255 locals that returns at once is far more work than its few
                // statements pay for. The top level makes its locals once, and is not charged.
                let locals = self.prog.functions[f as usize]
                    .slots
                    .saturating_sub(arity(c));
                self.fuel.burn(u64::from(locals)).map_err(|f| f.at(c.pos))?;
                self.depth += 1;
                let out = self.enter(f, args, c.pos);
                self.depth -= 1;
                out?
            }
            Target::Host(id) => {
                let mut cx = HostCx {
                    fuel: &mut self.fuel,
                    meter: &self.meter,
                };
                let r = self.host.call(HostId(id), &self.stack[args..], &mut cx);
                r.map_err(|e| e.into_fault().at(c.pos))?
            }
            Target::Builtin(b) => {
                let mut cx = Cx {
                    fuel: &mut self.fuel,
                    meter: &self.meter,
                };
                builtins::call(b, &self.stack[args..], &mut cx).map_err(|f| f.at(c.pos))?
            }
        };
        self.stack.truncate(args);
        Ok(out)
    }
}

/// How many arguments a call passes: its function's first slots.
fn arity(c: &Call) -> u16 {
    u16::try_from(c.args.len()).unwrap_or(u16::MAX)
}

fn list_index(index: &Value, list: &List) -> Result<usize, Fault> {
    usize::try_from(want_int(index)?)
        .ok()
        .filter(|&i| i < list.len())
        .ok_or_else(index_fault)
}
