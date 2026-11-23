//! Parses a script and resolves every name: variables to slots, calls to builtins, host
//! functions or the script's own functions. The tree is charged against the memory cap as it
//! is built, and its nesting is bounded.

mod expr;

use crate::ast::{Expr, ForEach, ForRange, Function, If, Program, Stmt, StmtKind, While};
use crate::error::{quoted, syntax, ErrorKind, Pos, ScriptError, Trap};
use crate::host::HostTable;
use crate::lexer::{Kw, Lexer, Tok, P};
use crate::limits::{alloc_charge, Limits, MAX_ARGS, MAX_FUNCTIONS, MAX_LOCALS, MAX_NEST};
use std::collections::BTreeMap;
use std::mem::size_of;

const NODE: usize = if size_of::<Expr>() > size_of::<Stmt>() {
    size_of::<Expr>()
} else {
    size_of::<Stmt>()
};
/// What one tree node is charged. A node may own a box (one allocation), sits in a vector
/// that can hold up to three times its length while growing, or four slots when short, and
/// may leave a call record behind; a test checks the real heap stays under this.
pub(crate) const NODE_CHARGE: usize = 6 * NODE + 128;
/// What one function is charged, for its table entry and name lookup.
const FUNCTION_CHARGE: usize = 256 + 3 * size_of::<Function>();
/// Charged once, for the scope list and other parser bookkeeping bounded by the limits.
const PARSER_CHARGE: usize = 4 * MAX_LOCALS * 32 + 4096;

struct FnInfo {
    index: u32,
    defined: bool,
}

struct CallRecord<'a> {
    index: u32,
    args: usize,
    pos: Pos,
    name: &'a str,
}

pub(crate) struct Parser<'a, 't> {
    lex: Lexer<'a>,
    tok: Tok<'a>,
    pos: Pos,
    host: &'t HostTable,
    fns: BTreeMap<&'a str, FnInfo>,
    functions: Vec<Option<Function>>,
    texts: Vec<Box<str>>,
    calls: Vec<CallRecord<'a>>,
    scope: Vec<&'a str>,
    max_slots: usize,
    nest: u32,
    loops: u32,
    nodes: u64,
    charge: usize,
    cap: usize,
}

/// Compiles `src` for a host offering `host`.
pub fn compile(src: &str, host: &HostTable, limits: &Limits) -> Result<Program, ScriptError> {
    if src.len() > limits.source_bytes {
        return Err(*too_large(
            Pos::START,
            "the script is longer than the source limit",
        ));
    }
    let mut lex = Lexer::new(src);
    let (tok, pos) = lex.next().map_err(|e| *e)?;
    let mut p = Parser {
        lex,
        tok,
        pos,
        host,
        fns: BTreeMap::new(),
        functions: vec![None],
        texts: Vec::new(),
        calls: Vec::new(),
        scope: Vec::new(),
        max_slots: 0,
        nest: 0,
        loops: 0,
        nodes: 0,
        charge: 0,
        cap: limits.memory,
    };
    p.program().map_err(|e| *e)
}

fn too_large(pos: Pos, message: &str) -> Trap {
    Box::new(ScriptError {
        kind: ErrorKind::TooLarge,
        pos,
        message: message.into(),
    })
}

impl<'a> Parser<'a, '_> {
    fn program(&mut self) -> Result<Program, Trap> {
        self.add_charge(PARSER_CHARGE)?;
        let mut main = Vec::new();
        while self.tok != Tok::Eof {
            if self.tok == Tok::Kw(Kw::Fn) {
                self.function()?;
            } else {
                main.push(self.statement()?);
            }
        }
        self.functions[0] = Some(Function {
            arity: 0,
            slots: self.max_slots as u16,
            body: main,
        });
        self.check_calls()?;
        let functions = std::mem::take(&mut self.functions)
            .into_iter()
            .flatten()
            .collect();
        Ok(Program {
            functions,
            texts: std::mem::take(&mut self.texts),
            charge: self.charge,
        })
    }

    // ------------------------------------------------------------ tokens and charges

    fn advance(&mut self) -> Result<Tok<'a>, Trap> {
        let (tok, pos) = self.lex.next()?;
        self.pos = pos;
        Ok(std::mem::replace(&mut self.tok, tok))
    }

    fn eat(&mut self, p: P) -> Result<bool, Trap> {
        if self.tok == Tok::P(p) {
            self.advance()?;
            return Ok(true);
        }
        Ok(false)
    }

    fn expect(&mut self, p: P, what: &str) -> Result<(), Trap> {
        if self.eat(p)? {
            Ok(())
        } else {
            Err(syntax(self.pos, format!("expected {what}")))
        }
    }

    fn ident(&mut self, what: &str) -> Result<&'a str, Trap> {
        match self.tok {
            Tok::Ident(name) => {
                self.advance()?;
                Ok(name)
            }
            _ => Err(syntax(self.pos, format!("expected {what}"))),
        }
    }

    fn add_charge(&mut self, bytes: usize) -> Result<(), Trap> {
        self.charge = self
            .charge
            .checked_add(bytes)
            .filter(|&c| c <= self.cap)
            .ok_or_else(|| too_large(self.pos, "the script is too large for the memory cap"))?;
        Ok(())
    }

    /// Charges one tree node and counts it towards the statement's fuel cost.
    fn node(&mut self) -> Result<(), Trap> {
        self.nodes += 1;
        self.add_charge(NODE_CHARGE)
    }

    fn charge_text(&mut self, len: usize) -> Result<(), Trap> {
        // The text in the program, and its slot in the list of texts (which may hold up to
        // three slots per text while growing).
        let cost = alloc_charge(len)
            .and_then(|c| c.checked_add(3 * size_of::<Box<str>>()))
            .ok_or_else(|| too_large(self.pos, "string literal too large"))?;
        self.add_charge(cost)
    }

    /// Keeps a string literal in the program and refers to it.
    fn text(&mut self, s: String, pos: Pos) -> Result<Expr, Trap> {
        let id = u32::try_from(self.texts.len())
            .map_err(|_| too_large(pos, "too many string literals"))?;
        self.texts.push(s.into_boxed_str());
        Ok(Expr::Text(id, pos))
    }

    fn enter(&mut self) -> Result<(), Trap> {
        self.nest += 1;
        if self.nest > MAX_NEST {
            return Err(too_large(
                self.pos,
                "blocks and expressions are nested too deeply",
            ));
        }
        Ok(())
    }

    fn leave(&mut self, levels: u32) {
        self.nest -= levels;
    }

    // ------------------------------------------------------------ names

    fn declare(&mut self, name: &'a str, pos: Pos) -> Result<u16, Trap> {
        if self.scope.len() >= MAX_LOCALS {
            return Err(too_large(pos, "too many local variables in one function"));
        }
        self.scope.push(name);
        self.max_slots = self.max_slots.max(self.scope.len());
        Ok((self.scope.len() - 1) as u16)
    }

    fn local(&self, name: &str) -> Option<u16> {
        self.scope
            .iter()
            .rposition(|n| *n == name)
            .map(|i| i as u16)
    }

    fn reserved(&self, name: &str) -> bool {
        crate::builtins::lookup(name).is_some() || self.host.lookup(name).is_some()
    }

    fn fn_index(&mut self, name: &'a str, pos: Pos) -> Result<u32, Trap> {
        if let Some(info) = self.fns.get(name) {
            return Ok(info.index);
        }
        if self.functions.len() > MAX_FUNCTIONS {
            return Err(too_large(pos, "too many functions"));
        }
        self.add_charge(FUNCTION_CHARGE)?;
        let index = self.functions.len() as u32;
        self.functions.push(None);
        self.fns.insert(
            name,
            FnInfo {
                index,
                defined: false,
            },
        );
        Ok(index)
    }

    fn record_call(&mut self, name: &'a str, args: usize, pos: Pos) -> Result<u32, Trap> {
        let index = self.fn_index(name, pos)?;
        self.calls.push(CallRecord {
            index,
            args,
            pos,
            name,
        });
        Ok(index)
    }

    fn check_calls(&self) -> Result<(), Trap> {
        for c in &self.calls {
            match self
                .functions
                .get(c.index as usize)
                .and_then(Option::as_ref)
            {
                None => {
                    return Err(syntax(
                        c.pos,
                        format!("unknown function '{}'", quoted(c.name)),
                    ))
                }
                Some(f) if usize::from(f.arity) != c.args => {
                    let msg = format!(
                        "'{}' takes {} arguments, not {}",
                        quoted(c.name),
                        f.arity,
                        c.args
                    );
                    return Err(syntax(c.pos, msg));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ functions

    fn function(&mut self) -> Result<(), Trap> {
        self.advance()?;
        let pos = self.pos;
        let name = self.ident("a function name")?;
        if self.reserved(name) {
            return Err(syntax(
                pos,
                format!("'{}' is a builtin or host function", quoted(name)),
            ));
        }
        let index = self.fn_index(name, pos)?;
        if let Some(info) = self.fns.get_mut(name) {
            if std::mem::replace(&mut info.defined, true) {
                return Err(syntax(pos, format!("'{}' is defined twice", quoted(name))));
            }
        }
        let outer = (std::mem::take(&mut self.scope), self.max_slots);
        self.max_slots = 0;
        let arity = self.params()?;
        let body = self.block()?;
        self.functions[index as usize] = Some(Function {
            arity,
            slots: self.max_slots as u16,
            body,
        });
        (self.scope, self.max_slots) = outer;
        Ok(())
    }

    fn params(&mut self) -> Result<u16, Trap> {
        self.expect(P::LParen, "'('")?;
        let mut n = 0u16;
        while !self.eat(P::RParen)? {
            if n > 0 {
                self.expect(P::Comma, "',' or ')'")?;
            }
            let pos = self.pos;
            let name = self.ident("a parameter name")?;
            if self.local(name).is_some() {
                return Err(syntax(
                    pos,
                    format!("parameter '{}' appears twice", quoted(name)),
                ));
            }
            if usize::from(n) >= MAX_ARGS {
                return Err(syntax(
                    pos,
                    format!("a function takes at most {MAX_ARGS} parameters"),
                ));
            }
            self.declare(name, pos)?;
            n += 1;
        }
        Ok(n)
    }

    // ------------------------------------------------------------ statements

    fn block(&mut self) -> Result<Vec<Stmt>, Trap> {
        self.enter()?;
        self.expect(P::LBrace, "'{'")?;
        let mark = self.scope.len();
        let mut body = Vec::new();
        while !self.eat(P::RBrace)? {
            if self.tok == Tok::Eof {
                return Err(syntax(self.pos, "missing '}'"));
            }
            body.push(self.statement()?);
        }
        self.scope.truncate(mark);
        self.leave(1);
        Ok(body)
    }

    /// Parses `parse`, returning what it built and how many nodes it added.
    fn counted<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, Trap>,
    ) -> Result<(T, u32), Trap> {
        let before = self.nodes;
        let out = parse(self)?;
        Ok((out, u32::try_from(self.nodes - before).unwrap_or(u32::MAX)))
    }

    fn statement(&mut self) -> Result<Stmt, Trap> {
        let pos = self.pos;
        self.node()?;
        let (kind, nodes) = match self.tok {
            Tok::Kw(Kw::Let) => self.let_stmt()?,
            Tok::Kw(Kw::If) => self.if_stmt()?,
            Tok::Kw(Kw::While) => self.while_stmt()?,
            Tok::Kw(Kw::For) => self.for_stmt()?,
            Tok::Kw(Kw::Break | Kw::Continue) => self.jump()?,
            Tok::Kw(Kw::Return) => self.return_stmt()?,
            Tok::Kw(Kw::Fn) => {
                return Err(syntax(
                    pos,
                    "functions can only be defined at the top level",
                ))
            }
            _ => self.expr_stmt()?,
        };
        Ok(Stmt {
            kind,
            pos,
            cost: nodes.saturating_add(1),
        })
    }

    fn let_stmt(&mut self) -> Result<(StmtKind, u32), Trap> {
        self.advance()?;
        let pos = self.pos;
        let name = self.ident("a variable name")?;
        self.expect(P::Assign, "'='")?;
        // The value is read before the name exists, so `let x = x + 1;` uses an outer `x`.
        let (value, nodes) = self.counted(Self::expr)?;
        self.expect(P::Semi, "';'")?;
        let slot = self.declare(name, pos)?;
        Ok((StmtKind::Set(slot, value), nodes))
    }

    fn if_stmt(&mut self) -> Result<(StmtKind, u32), Trap> {
        self.advance()?;
        let (cond, nodes) = self.counted(Self::expr)?;
        let then = self.block()?;
        let other = if self.tok == Tok::Kw(Kw::Else) {
            self.advance()?;
            if self.tok == Tok::Kw(Kw::If) {
                self.enter()?;
                let s = self.statement()?;
                self.leave(1);
                vec![s]
            } else {
                self.block()?
            }
        } else {
            Vec::new()
        };
        Ok((StmtKind::If(Box::new(If { cond, then, other })), nodes))
    }

    fn loop_body(&mut self) -> Result<Vec<Stmt>, Trap> {
        self.loops += 1;
        let body = self.block();
        self.loops -= 1;
        body
    }

    fn while_stmt(&mut self) -> Result<(StmtKind, u32), Trap> {
        self.advance()?;
        let (cond, nodes) = self.counted(Self::expr)?;
        let body = self.loop_body()?;
        let test_cost = nodes.saturating_add(1);
        Ok((
            StmtKind::While(Box::new(While {
                cond,
                body,
                test_cost,
            })),
            0,
        ))
    }

    fn for_stmt(&mut self) -> Result<(StmtKind, u32), Trap> {
        self.advance()?;
        let pos = self.pos;
        let name = self.ident("a loop variable")?;
        if self.tok != Tok::Kw(Kw::In) {
            return Err(syntax(self.pos, "expected 'in'"));
        }
        self.advance()?;
        let (from, mut nodes) = self.counted(Self::expr)?;
        let to = if self.eat(P::DotDot)? {
            let (to, n) = self.counted(Self::expr)?;
            nodes = nodes.saturating_add(n);
            Some(to)
        } else {
            None
        };
        let mark = self.scope.len();
        let var = self.declare(name, pos)?;
        let body = self.loop_body()?;
        self.scope.truncate(mark);
        let kind = match to {
            Some(to) => StmtKind::ForRange(Box::new(ForRange {
                var,
                from,
                to,
                body,
            })),
            None => StmtKind::ForEach(Box::new(ForEach {
                var,
                list: from,
                body,
            })),
        };
        Ok((kind, nodes))
    }

    fn jump(&mut self) -> Result<(StmtKind, u32), Trap> {
        let pos = self.pos;
        let kind = match self.advance()? {
            Tok::Kw(Kw::Break) => StmtKind::Break,
            _ => StmtKind::Continue,
        };
        if self.loops == 0 {
            return Err(syntax(pos, "'break' and 'continue' belong inside a loop"));
        }
        self.expect(P::Semi, "';'")?;
        Ok((kind, 0))
    }

    fn return_stmt(&mut self) -> Result<(StmtKind, u32), Trap> {
        self.advance()?;
        if self.eat(P::Semi)? {
            return Ok((StmtKind::Return(None), 0));
        }
        let (value, nodes) = self.counted(Self::expr)?;
        self.expect(P::Semi, "';'")?;
        Ok((StmtKind::Return(Some(value)), nodes))
    }

    fn expr_stmt(&mut self) -> Result<(StmtKind, u32), Trap> {
        let pos = self.pos;
        let (target, mut nodes) = self.counted(Self::expr)?;
        if !self.eat(P::Assign)? {
            self.expect(P::Semi, "';'")?;
            return Ok((StmtKind::Expr(target), nodes));
        }
        let (value, n) = self.counted(Self::expr)?;
        nodes = nodes.saturating_add(n);
        self.expect(P::Semi, "';'")?;
        let kind = match target {
            Expr::Local(slot, _) => StmtKind::Set(slot, value),
            Expr::Index(pair, _) => match *pair {
                (Expr::Local(slot, _), index) => StmtKind::SetIndex(slot, Box::new((index, value))),
                _ => return Err(syntax(pos, "only a variable's list items can be assigned")),
            },
            _ => {
                return Err(syntax(
                    pos,
                    "only a variable or a list item can be assigned",
                ))
            }
        };
        Ok((kind, nodes))
    }
}
