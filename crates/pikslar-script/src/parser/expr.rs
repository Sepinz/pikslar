//! Expressions, from loosest to tightest: `||`, `&&`, comparisons (which do not chain),
//! `+ -`, `* / %`, unary `- !`, indexing, then literals, names, calls and brackets.
//!
//! Every operator added to a chain counts as one more level of nesting, so the depth of the
//! tree, and so the interpreter's recursion over it, stays under [`MAX_NEST`].

use super::Parser;
use crate::ast::Scalar;
use crate::ast::{BinOp, Call, Expr, Target, UnOp};
use crate::error::{quoted, syntax, Pos, Trap};
use crate::lexer::{Kw, Tok, P};
use crate::limits::MAX_ARGS;

const COMPARE: [(P, BinOp); 6] = [
    (P::Eq, BinOp::Eq),
    (P::Ne, BinOp::Ne),
    (P::Lt, BinOp::Lt),
    (P::Le, BinOp::Le),
    (P::Gt, BinOp::Gt),
    (P::Ge, BinOp::Ge),
];
const ADD: [(P, BinOp); 2] = [(P::Plus, BinOp::Add), (P::Minus, BinOp::Sub)];
const MUL: [(P, BinOp); 3] = [
    (P::Star, BinOp::Mul),
    (P::Slash, BinOp::Div),
    (P::Percent, BinOp::Rem),
];

fn op_of(tok: &Tok<'_>, ops: &[(P, BinOp)]) -> Option<BinOp> {
    match tok {
        Tok::P(p) => ops.iter().find(|(q, _)| q == p).map(|(_, op)| *op),
        _ => None,
    }
}

type Sub<'a, 't> = fn(&mut Parser<'a, 't>) -> Result<Expr, Trap>;

impl<'a, 't> Parser<'a, 't> {
    pub(super) fn expr(&mut self) -> Result<Expr, Trap> {
        self.enter()?;
        let e = self.or();
        self.leave(1);
        e
    }

    /// A left-associative chain of `sub`s joined by `ops`; `join` builds each link.
    fn chain(
        &mut self,
        ops: &[(P, BinOp)],
        sub: Sub<'a, 't>,
        join: fn(BinOp, Expr, Expr, Pos) -> Expr,
    ) -> Result<Expr, Trap> {
        let mut left = sub(self)?;
        let mut levels = 0;
        while let Some(op) = op_of(&self.tok, ops) {
            let pos = self.pos;
            self.enter()?;
            levels += 1;
            self.advance()?;
            self.node()?;
            let right = sub(self)?;
            left = join(op, left, right, pos);
        }
        self.leave(levels);
        Ok(left)
    }

    fn or(&mut self) -> Result<Expr, Trap> {
        self.chain(&[(P::OrOr, BinOp::Eq)], Self::and, |_, l, r, p| {
            Expr::Or(Box::new((l, r)), p)
        })
    }

    fn and(&mut self) -> Result<Expr, Trap> {
        self.chain(&[(P::AndAnd, BinOp::Eq)], Self::compare, |_, l, r, p| {
            Expr::And(Box::new((l, r)), p)
        })
    }

    fn compare(&mut self) -> Result<Expr, Trap> {
        let left = self.add()?;
        let Some(op) = op_of(&self.tok, &COMPARE) else {
            return Ok(left);
        };
        let pos = self.pos;
        self.advance()?;
        self.node()?;
        let right = self.add()?;
        if op_of(&self.tok, &COMPARE).is_some() {
            return Err(syntax(
                self.pos,
                "comparisons do not chain; join them with && or ||",
            ));
        }
        Ok(binary(op, left, right, pos))
    }

    fn add(&mut self) -> Result<Expr, Trap> {
        self.chain(&ADD, Self::mul, binary)
    }

    fn mul(&mut self) -> Result<Expr, Trap> {
        self.chain(&MUL, Self::unary, binary)
    }

    fn unary(&mut self) -> Result<Expr, Trap> {
        let op = match self.tok {
            Tok::P(P::Minus) => UnOp::Neg,
            Tok::P(P::Bang) => UnOp::Not,
            _ => return self.postfix(),
        };
        let pos = self.pos;
        self.advance()?;
        self.enter()?;
        let inner = self.unary();
        self.leave(1);
        Ok(match (op, inner?) {
            // Folded, so `-1` costs one node, as `1` does.
            (UnOp::Neg, Expr::Lit(Scalar::Int(n), _)) if n != i64::MIN => {
                Expr::Lit(Scalar::Int(-n), pos)
            }
            (UnOp::Neg, Expr::Lit(Scalar::Float(x), _)) => Expr::Lit(Scalar::Float(-x), pos),
            (op, e) => {
                self.node()?;
                Expr::Unary(op, Box::new(e), pos)
            }
        })
    }

    fn postfix(&mut self) -> Result<Expr, Trap> {
        let mut e = self.primary()?;
        let mut levels = 0;
        while self.tok == Tok::P(P::LBracket) {
            let pos = self.pos;
            self.enter()?;
            levels += 1;
            self.advance()?;
            self.node()?;
            let index = self.expr()?;
            self.expect(P::RBracket, "']'")?;
            e = Expr::Index(Box::new((e, index)), pos);
        }
        self.leave(levels);
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, Trap> {
        let pos = self.pos;
        self.node()?;
        let lit = match self.advance()? {
            Tok::Int(n) => Scalar::Int(n),
            Tok::Float(x) => Scalar::Float(x),
            Tok::Kw(Kw::True) => Scalar::Bool(true),
            Tok::Kw(Kw::False) => Scalar::Bool(false),
            Tok::Kw(Kw::Nil) => Scalar::Nil,
            Tok::Str(s) => {
                self.charge_text(s.len())?;
                return self.text(s, pos);
            }
            Tok::Ident(name) => return self.name(name, pos),
            Tok::P(P::LParen) => {
                let e = self.expr()?;
                self.expect(P::RParen, "')'")?;
                return Ok(e);
            }
            Tok::P(P::LBracket) => {
                let items = self.list_items(P::RBracket, "']'")?;
                return Ok(Expr::List(items, pos));
            }
            _ => return Err(syntax(pos, "expected a value")),
        };
        Ok(Expr::Lit(lit, pos))
    }

    fn name(&mut self, name: &'a str, pos: Pos) -> Result<Expr, Trap> {
        if self.eat(P::LParen)? {
            return self.call(name, pos);
        }
        match self.local(name) {
            Some(slot) => Ok(Expr::Local(slot, pos)),
            None => Err(syntax(pos, format!("unknown variable '{}'", quoted(name)))),
        }
    }

    /// Comma-separated expressions up to `close`, which is consumed.
    fn list_items(&mut self, close: P, what: &str) -> Result<Vec<Expr>, Trap> {
        let mut items = Vec::new();
        while !self.eat(close)? {
            if !items.is_empty() {
                self.expect(P::Comma, &format!("',' or {what}"))?;
            }
            items.push(self.expr()?);
        }
        Ok(items)
    }

    fn call(&mut self, name: &'a str, pos: Pos) -> Result<Expr, Trap> {
        let args = self.list_items(P::RParen, "')'")?;
        if args.len() > MAX_ARGS {
            return Err(syntax(
                pos,
                format!("a call passes at most {MAX_ARGS} arguments"),
            ));
        }
        let target = if let Some((b, arity)) = crate::builtins::lookup(name) {
            check_arity(name, arity, args.len(), pos)?;
            Target::Builtin(b)
        } else if let Some((id, arity)) = self.host.lookup(name) {
            check_arity(name, arity, args.len(), pos)?;
            Target::Host(id)
        } else {
            Target::User(self.record_call(name, args.len(), pos)?)
        };
        Ok(Expr::Call(Box::new(Call { target, args, pos })))
    }
}

fn binary(op: BinOp, l: Expr, r: Expr, pos: Pos) -> Expr {
    Expr::Binary(op, Box::new((l, r)), pos)
}

fn check_arity(name: &str, want: usize, got: usize, pos: Pos) -> Result<(), Trap> {
    if want == got {
        return Ok(());
    }
    Err(syntax(
        pos,
        format!("'{}' takes {want} arguments, not {got}", quoted(name)),
    ))
}
