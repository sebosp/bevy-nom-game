//! Runs the player's parser on the payload, and records what each `let` matched.
use std::sync::Arc;

use super::ScriptError;
use super::ast::*;
use super::parsers::{self, Parser};
use super::value::*;

/// The parsers run at most this many times: an endless `many0` stops.
const MAX_STEPS: u64 = 400_000;
/// The calls of the script's functions nest at most this deep.
const MAX_DEPTH: usize = 200;
/// The trace keeps this many events.
const MAX_EVENTS: usize = 400;

/// What a `let` of the parsing function bound: its name, the bytes consumed, the value.
#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,
    pub line: usize,
    /// All the bytes the parser consumed, `tag("x=")` of a `preceded` included.
    pub consumed: Span,
    /// The bytes of the value itself.
    pub span: Span,
    pub value: Val,
    /// The color of the binding, `None` for `_`.
    pub color: Option<usize>,
    /// The bytes of the items of a `Vec`, shaded in turn.
    pub items: Vec<Span>,
}

/// A line of the trace.
#[derive(Debug, Clone)]
pub enum Event {
    /// A binding of the parsing function, by its index.
    Bound(usize),
    /// `dbg_peek_hex` or `context` saw its parser fail.
    PeekError { context: String, pos: usize, kind: String, line: usize },
    /// `dbg_bytes_and_tail!(x)`: the bytes of `x` and the ones after.
    BytesAndTail { name: String, span: Span, line: usize },
    /// `dbg_displayable_and_tail!(x)`: the value of `x`.
    Displayable { name: String, value: String, line: usize },
    /// `println!`.
    Print { text: String, line: usize },
    /// A statement of the parsing function failed with this nom error.
    Failed { err: PErr, line: usize },
}

#[derive(Debug, Clone)]
pub enum RunError {
    Script(ScriptError),
    Parse { err: PErr, line: usize },
}

/// The result of running a script.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub bindings: Vec<Binding>,
    pub events: Vec<Event>,
    /// Where the parser stopped and the value it returned.
    pub result: Result<(usize, Val), RunError>,
    pub text: bool,
    pub entry: String,
}

enum Flow {
    Parse(PErr),
    Script(ScriptError),
}

impl From<ScriptError> for Flow {
    fn from(e: ScriptError) -> Self {
        Flow::Script(e)
    }
}

type R<T> = Result<T, Flow>;

pub struct Interp<'a> {
    prog: &'a Program,
    input: &'a [u8],
    /// The end of the input: `length_value` gives its parser a shorter one.
    end: usize,
    text: bool,
    frames: Vec<Vec<(String, Val)>>,
    depth: usize,
    steps: u64,
    line: usize,
    bindings: Vec<Binding>,
    events: Vec<Event>,
    colors: usize,
}

/// Runs `entry`, or the function `parse`, or the last function, on the input.
pub fn run(prog: &Program, input: &[u8], entry: Option<&str>) -> Outcome {
    let f = entry.and_then(|e| prog.get(e)).or_else(|| prog.get("parse")).or(prog.fns.last());
    let Some(f) = f else {
        let err = ScriptError { line: 1, col: 1, message: "write a function: fn parse(input: &str) -> IResult<&str, T> { ... }".into() };
        return Outcome { bindings: vec![], events: vec![], result: Err(RunError::Script(err)), text: true, entry: String::new() };
    };
    let mut it = Interp {
        prog,
        input,
        end: input.len(),
        text: f.is_text(),
        frames: vec![],
        depth: 0,
        steps: 0,
        line: f.line,
        bindings: vec![],
        events: vec![],
        colors: 0,
    };
    if it.text && std::str::from_utf8(input).is_err() {
        let err =
            ScriptError { line: f.line, col: 1, message: "this file is not UTF-8 text: take `input: &[u8]` instead of `&str`".into() };
        return Outcome { bindings: vec![], events: vec![], result: Err(RunError::Script(err)), text: false, entry: f.name.clone() };
    }
    let result = match it.call_fn(f, 0) {
        Ok(r) => Ok(r),
        Err(Flow::Parse(err)) => Err(RunError::Parse { err, line: it.line }),
        Err(Flow::Script(e)) => Err(RunError::Script(e)),
    };
    Outcome { bindings: it.bindings, events: it.events, result, text: it.text, entry: f.name.clone() }
}

impl Interp<'_> {
    fn err<T>(&self, message: impl Into<String>) -> R<T> {
        Err(Flow::Script(ScriptError { line: self.line, col: 1, message: message.into() }))
    }

    fn event(&mut self, e: Event) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(e);
        }
    }

    /// The bytes `a..b`, a `&str` or a `&[u8]`.
    fn slice(&self, a: usize, b: usize) -> Val {
        let bytes = &self.input[a..b];
        let v = if self.text { V::Str(String::from_utf8_lossy(bytes).into_owned()) } else { V::Bytes(bytes.to_vec()) };
        Val { v, span: Some(a..b) }
    }

    /// The element at `pos`: a char of the text, or a byte, and its length.
    fn elem(&self, pos: usize) -> Option<(char, usize)> {
        if pos >= self.end {
            return None;
        }
        if self.text {
            let rest = std::str::from_utf8(&self.input[pos..self.end]).ok()?;
            rest.chars().next().map(|c| (c, c.len_utf8()))
        } else {
            Some((self.input[pos] as char, 1))
        }
    }

    /// The element as the argument of a predicate: a `char`, or a `u8`.
    fn elem_val(&self, c: char) -> Val {
        if self.text { V::Char(c).into() } else { V::Int(c as i128, "u8").into() }
    }

    // ---- The functions of the script.

    fn call_fn(&mut self, f: &FnDef, pos: usize) -> R<(usize, Val)> {
        if self.depth >= MAX_DEPTH {
            return self.err(format!("`{}` calls itself without end", f.name));
        }
        self.depth += 1;
        self.frames.push(vec![(f.param.clone(), V::Input(pos).into())]);
        let saved_line = self.line;
        let result = self.fn_body(f);
        self.frames.pop();
        self.depth -= 1;
        // The line of the failure stays for the error of the top function.
        if result.is_ok() || self.depth > 0 {
            self.line = if self.depth == 0 { self.line } else { saved_line };
        }
        let value = result?;
        match value.v {
            V::Result(Ok(t)) => match t.v {
                V::Tuple(items) if items.len() == 2 => match (&items[0].v, &items[1]) {
                    (V::Input(end), value) => {
                        let mut value = value.clone();
                        if value.span.is_none() {
                            value.span = Some(pos..*end);
                        }
                        Ok((*end, value))
                    }
                    _ => self.err(format!("`{}` must return Ok((input, value)), the first is the rest of the input", f.name)),
                },
                _ => self.err(format!("`{}` must return Ok((input, value))", f.name)),
            },
            V::Result(Err(e)) => match e.v {
                V::Error(pe) => Err(Flow::Parse(pe)),
                _ => self.err(format!("`{}` returned an error that is not a nom error", f.name)),
            },
            _ => self.err(format!("`{}` must end with Ok((input, value)), not {}", f.name, value.kind())),
        }
    }

    fn fn_body(&mut self, f: &FnDef) -> R<Val> {
        for stmt in &f.body {
            self.stmt(stmt)?;
        }
        self.line = f.body.last().map(|s| s.line() + 1).unwrap_or(f.line);
        match &f.tail {
            Some(tail) => self.expr(tail),
            None => self.err(format!("`{}` must end with Ok((input, value)), without a `;`", f.name)),
        }
    }

    fn lookup(&self, name: &str) -> Option<&Val> {
        self.frames.last()?.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    fn bind(&mut self, pat: &Pat, val: Val) -> R<()> {
        match pat {
            Pat::Wild => Ok(()),
            Pat::Ident(name) => {
                if let Some(frame) = self.frames.last_mut() {
                    frame.push((name.clone(), val));
                }
                Ok(())
            }
            Pat::Tuple(pats) => match val.v {
                V::Tuple(items) if items.len() == pats.len() => {
                    for (p, v) in pats.iter().zip(items) {
                        self.bind(p, v)?;
                    }
                    Ok(())
                }
                V::Tuple(items) => self.err(format!("the pattern has {} parts, the value {}", pats.len(), items.len())),
                _ => self.err(format!("a tuple pattern needs a tuple, not {}", val.kind())),
            },
        }
    }

    fn stmt(&mut self, stmt: &Stmt) -> R<()> {
        self.line = stmt.line();
        match stmt {
            Stmt::Let { pat, expr, .. } => {
                let start = if self.depth == 1 { self.applied_input(expr) } else { None };
                let val = match self.expr(expr) {
                    Ok(v) => v,
                    Err(Flow::Parse(e)) => {
                        if self.depth == 1 {
                            self.event(Event::Failed { err: e.clone(), line: self.line });
                        }
                        return Err(Flow::Parse(e));
                    }
                    Err(e) => return Err(e),
                };
                if let Some(start) = start {
                    self.record(pat, &val, start);
                }
                self.bind(pat, val)
            }
            Stmt::Expr { expr, .. } => self.expr(expr).map(|_| ()),
        }
    }

    /// Where the parser of `let (rest, x) = parser(input)?;` starts.
    fn applied_input(&self, expr: &Expr) -> Option<usize> {
        let Expr::Try(inner) = expr else {
            return None;
        };
        let arg = match inner.as_ref() {
            Expr::Call { args, .. } if args.len() == 1 => &args[0],
            Expr::Method { name, args, .. } if (name == "parse" || name == "parse_complete") && args.len() == 1 => &args[0],
            _ => return None,
        };
        let name = arg.path_name()?;
        match self.lookup(name)?.v {
            V::Input(pos) => Some(pos),
            _ => None,
        }
    }

    /// The bindings of `let (rest, pattern) = ...` in the parsing function, with their colors.
    fn record(&mut self, pat: &Pat, val: &Val, start: usize) {
        let (Pat::Tuple(parts), V::Tuple(items)) = (pat, &val.v) else {
            return;
        };
        let (Some(V::Input(end)), Some(value_pat), Some(value)) = (items.first().map(|v| &v.v), parts.get(1), items.get(1)) else {
            return;
        };
        let end = *end;
        self.record_value(value_pat, value, start..end);
    }

    fn record_value(&mut self, pat: &Pat, value: &Val, consumed: Span) {
        match (pat, &value.v) {
            (Pat::Tuple(pats), V::Tuple(items)) if pats.len() == items.len() => {
                for (p, v) in pats.iter().zip(items) {
                    let span = v.span.clone().unwrap_or(consumed.clone());
                    self.record_value(p, v, span);
                }
            }
            _ => {
                let (name, color) = match pat {
                    Pat::Ident(name) => {
                        self.colors += 1;
                        (name.clone(), Some(self.colors - 1))
                    }
                    _ => ("_".to_string(), None),
                };
                let span = value.span.clone().unwrap_or(consumed.clone());
                let items = match &value.v {
                    V::List(items) => items.iter().filter_map(|i| i.span.clone()).collect(),
                    _ => vec![],
                };
                self.bindings.push(Binding { name, line: self.line, consumed, span, value: value.clone(), color, items });
                let i = self.bindings.len() - 1;
                self.event(Event::Bound(i));
            }
        }
    }

    // ---- Expressions.

    fn resolve(&self, segments: &[String], generic: &Option<String>) -> R<Val> {
        let name = segments.last().map(String::as_str).unwrap_or_default();
        if segments.len() == 1 {
            if let Some(v) = self.lookup(name) {
                return Ok(v.clone());
            }
            if let Some(f) = self.prog.get(name) {
                return Ok(V::Parser(Arc::new(Parser::UserFn(f.name.clone()))).into());
            }
        }
        let full = if segments.len() == 1 { self.prog.uses.iter().find(|u| u.last().is_some_and(|l| l == name)) } else { None };
        let path = full.map(Vec::as_slice).unwrap_or(segments);
        let in_number = path.iter().any(|s| s == "number");
        if in_number && matches!(name, "u16" | "u32" | "u64" | "u128" | "i16" | "i32" | "i64" | "i128") {
            return self.err(format!("nom::number::complete::{name} takes an endianness: use be_{name} or le_{name}"));
        }
        if in_number && matches!(name, "u8" | "i8") {
            return Ok(V::Parser(Arc::new(Parser::Builtin(if name == "u8" { "be_u8" } else { "be_i8" }))).into());
        }
        // `str::parse`, `u32::from`: the types of Rust, not the parsers.
        let qualified_type = segments.len() > 1
            && matches!(
                segments[segments.len() - 2].as_str(),
                "str"
                    | "String"
                    | "char"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "usize"
                    | "AsChar"
                    | "Vec"
                    | "Option"
                    | "Result"
                    | "f32"
                    | "f64"
                    | "u128"
                    | "i128"
                    | "isize"
            );
        if !qualified_type {
            if let Some(b) = parsers::builtin(name) {
                return Ok(V::Parser(Arc::new(Parser::Builtin(b))).into());
            }
            if let Some(c) = parsers::combinator(name) {
                return Ok(V::Func(Func::Combinator(c)).into());
            }
        }
        if name == "None" {
            return Ok(V::Option(None).into());
        }
        if name.starts_with(|c: char| c.is_lowercase() || c == '_') || segments.len() > 1 || matches!(name, "Ok" | "Err" | "Some") {
            if segments.len() == 1 && !KNOWN_FNS.contains(&name) {
                return self.err(format!("`{name}` is not defined: no `let` binds it, no fn of the script, no parser of nom"));
            }
            return Ok(V::Func(Func::Named { name: segments.join("::"), generic: generic.clone() }).into());
        }
        self.err(format!("`{}` is not defined", segments.join("::")))
    }

    fn expr(&mut self, e: &Expr) -> R<Val> {
        match e {
            Expr::Path { segments, generic } => self.resolve(segments, generic),
            Expr::Int(v) => Ok(V::Int(*v, "").into()),
            Expr::Float(v) => Ok(V::Float(*v).into()),
            Expr::Str(s) => Ok(V::Str(s.clone()).into()),
            Expr::ByteStr(b) => Ok(V::Bytes(b.clone()).into()),
            Expr::Char(c) => Ok(V::Char(*c).into()),
            Expr::Byte(b) => Ok(V::Int(*b as i128, "u8").into()),
            Expr::Bool(b) => Ok(V::Bool(*b).into()),
            Expr::Ref(inner) => self.expr(inner),
            Expr::Unary { op, expr } => {
                let v = self.expr(expr)?;
                match (op, &v.v) {
                    (UnOp::Deref, _) => Ok(v),
                    (UnOp::Not, V::Bool(b)) => Ok(V::Bool(!b).into()),
                    (UnOp::Not, V::Int(i, t)) => Ok(V::Int(wrap(!i, t), t).into()),
                    (UnOp::Neg, V::Int(i, t)) => Ok(V::Int(-i, t).into()),
                    (UnOp::Neg, V::Float(f)) => Ok(V::Float(-f).into()),
                    _ => self.err(format!("`{op:?}` does not apply to {}", v.kind())),
                }
            }
            Expr::Tuple(items) => {
                let vals = items.iter().map(|i| self.expr(i)).collect::<R<Vec<_>>>()?;
                Ok(if vals.is_empty() { Val::unit() } else { V::Tuple(vals).into() })
            }
            Expr::Array(items) => {
                let vals = items.iter().map(|i| self.expr(i)).collect::<R<Vec<_>>>()?;
                Ok(V::List(vals).into())
            }
            Expr::Repeat { value, count } => {
                let v = self.expr(value)?;
                let n = self.expr(count)?.as_int().unwrap_or(0).clamp(0, 1 << 16) as usize;
                Ok(V::List(vec![v; n]).into())
            }
            Expr::Binary { op, left, right } => self.binary(*op, left, right),
            Expr::Cast { expr, ty } => {
                let v = self.expr(expr)?;
                self.cast(v, ty)
            }
            Expr::Closure { params, body } => {
                let env = self.frames.last().cloned().unwrap_or_default();
                Ok(V::Func(Func::Closure(Arc::new(Closure { params: params.clone(), body: (**body).clone(), env }))).into())
            }
            Expr::Struct { name, fields } => {
                let vals = fields.iter().map(|(k, e)| Ok((k.clone(), self.expr(e)?))).collect::<R<Vec<_>>>()?;
                Ok(V::Struct(name.clone(), vals).into())
            }
            Expr::Field { recv, name } => {
                let v = self.expr(recv)?;
                match (&v.v, name.parse::<usize>()) {
                    (V::Tuple(items), Ok(i)) if i < items.len() => Ok(items[i].clone()),
                    (V::Struct(_, fields), _) => match fields.iter().find(|(k, _)| k == name) {
                        Some((_, v)) => Ok(v.clone()),
                        None => self.err(format!("no field `{name}`")),
                    },
                    _ => self.err(format!("no field `{name}` on {}", v.kind())),
                }
            }
            Expr::Try(inner) => {
                let v = self.expr(inner)?;
                match v.v {
                    V::Result(Ok(x)) => Ok(*x),
                    V::Result(Err(e)) => match e.v {
                        V::Error(pe) => Err(Flow::Parse(pe)),
                        other => self.err(format!("`?` on an error that is not a nom error: {}", Val::from(other).show(60))),
                    },
                    V::Option(Some(x)) => Ok(*x),
                    V::Option(None) => self.err("`?` on a None"),
                    _ => self.err(format!("`?` needs a Result, not {}", v.kind())),
                }
            }
            Expr::If { cond, then, otherwise } => {
                let c = self.expr(cond)?;
                match c.v {
                    V::Bool(true) => self.expr(then),
                    V::Bool(false) => match otherwise {
                        Some(o) => self.expr(o),
                        None => Ok(Val::unit()),
                    },
                    _ => self.err(format!("`if` needs a bool, not {}", c.kind())),
                }
            }
            Expr::Block { stmts, tail } => {
                for s in stmts {
                    self.stmt(s)?;
                }
                match tail {
                    Some(t) => self.expr(t),
                    None => Ok(Val::unit()),
                }
            }
            Expr::Range { start, end, inclusive } => {
                let s = self.expr(start)?;
                let e = self.expr(end)?;
                Ok(V::Struct(
                    "Range".into(),
                    vec![("start".into(), s), ("end".into(), e), ("inclusive".into(), V::Bool(*inclusive).into())],
                )
                .into())
            }
            Expr::Macro { name, args } => self.macro_call(name, args),
            Expr::Index { recv, index } => {
                let v = self.expr(recv)?;
                let i = self.expr(index)?;
                self.index(v, i)
            }
            Expr::Call { func, args } => {
                let f = self.expr(func)?;
                let vals = args.iter().map(|a| self.expr(a)).collect::<R<Vec<_>>>()?;
                self.apply(f, vals)
            }
            Expr::Method { recv, name, generic, args } => {
                let r = self.expr(recv)?;
                let vals = args.iter().map(|a| self.expr(a)).collect::<R<Vec<_>>>()?;
                self.method(r, name, generic.as_deref(), vals)
            }
        }
    }

    fn macro_call(&mut self, name: &str, args: &[Expr]) -> R<Val> {
        let line = self.line;
        match name {
            "vec" => {
                if let [Expr::Repeat { .. }] = args {
                    return self.expr(&args[0]);
                }
                let vals = args.iter().map(|a| self.expr(a)).collect::<R<Vec<_>>>()?;
                Ok(V::List(vals).into())
            }
            "dbg_bytes_and_tail" => {
                let Some(first) = args.first() else {
                    return self.err("dbg_bytes_and_tail!(name) needs a name");
                };
                let v = self.expr(first)?;
                let name = first.path_name().unwrap_or("?").to_string();
                match v.span {
                    Some(span) => self.event(Event::BytesAndTail { name, span, line }),
                    None => self.event(Event::Displayable { name, value: v.show(60), line }),
                }
                Ok(Val::unit())
            }
            "dbg_displayable_and_tail" | "dbg" => {
                for a in args {
                    let v = self.expr(a)?;
                    let name = a.path_name().unwrap_or("?").to_string();
                    self.event(Event::Displayable { name, value: v.show(80), line });
                }
                Ok(Val::unit())
            }
            "println" | "print" | "eprintln" | "eprint" | "format" => {
                let text = self.format(args)?;
                if name == "format" {
                    return Ok(V::Str(text).into());
                }
                self.event(Event::Print { text, line });
                Ok(Val::unit())
            }
            "matches" => self.err("matches! is not supported, compare with == instead"),
            _ => self.err(format!("the macro `{name}!` is not supported")),
        }
    }

    /// `format!("{} {name:?}", a)`.
    fn format(&mut self, args: &[Expr]) -> R<String> {
        let Some(Expr::Str(fmt)) = args.first() else {
            return self.err("the first argument is the format string, \"{}\"");
        };
        let mut vals = args[1..].iter().map(|a| self.expr(a)).collect::<R<Vec<_>>>()?.into_iter();
        let mut out = String::new();
        let mut chars = fmt.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    out.push('{');
                    continue;
                }
                let mut spec = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    spec.push(c);
                }
                let (name, fmt_spec) = spec.split_once(':').unwrap_or((&spec, ""));
                let v = if name.is_empty() { vals.next() } else { self.lookup(name).cloned() };
                let Some(v) = v else {
                    return self.err(format!("nothing to format for `{{{spec}}}`"));
                };
                let debug = fmt_spec.contains('?');
                out.push_str(&match (&v.v, debug) {
                    (V::Str(s), false) => s.clone(),
                    (V::Char(c), false) => c.to_string(),
                    _ => v.show(200),
                });
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                out.push('}');
            } else {
                out.push(c);
            }
        }
        Ok(out)
    }

    fn binary(&mut self, op: BinOp, left: &Expr, right: &Expr) -> R<Val> {
        let l = self.expr(left)?;
        // `&&` and `||` stop early, as in Rust.
        if let (BinOp::And | BinOp::Or, V::Bool(b)) = (op, &l.v)
            && (*b == (op == BinOp::Or))
        {
            return Ok(V::Bool(*b).into());
        }
        let r = self.expr(right)?;
        use BinOp::*;
        match (op, &l.v, &r.v) {
            (And | Or, V::Bool(_), V::Bool(b)) => Ok(V::Bool(*b).into()),
            (Eq, _, _) => Ok(V::Bool(l.same(&r)).into()),
            (Ne, _, _) => Ok(V::Bool(!l.same(&r)).into()),
            (Lt | Le | Gt | Ge, V::Float(a), V::Float(b)) => Ok(V::Bool(match op {
                Lt => a < b,
                Le => a <= b,
                Gt => a > b,
                _ => a >= b,
            })
            .into()),
            (Lt | Le | Gt | Ge, V::Str(a), V::Str(b)) => Ok(V::Bool(match op {
                Lt => a < b,
                Le => a <= b,
                Gt => a > b,
                _ => a >= b,
            })
            .into()),
            (Add | Sub | Mul | Div, V::Float(a), V::Float(b)) => Ok(V::Float(match op {
                Add => a + b,
                Sub => a - b,
                Mul => a * b,
                _ => a / b,
            })
            .into()),
            _ => {
                let (Some(a), Some(b)) = (l.as_int(), r.as_int()) else {
                    return self.err(format!("`{op:?}` between {} and {}", l.kind(), r.kind()));
                };
                let ty = match (&l.v, &r.v) {
                    (V::Int(_, t), _) if !t.is_empty() => t,
                    (_, V::Int(_, t)) => t,
                    _ => "",
                };
                let v = match op {
                    Lt => return Ok(V::Bool(a < b).into()),
                    Le => return Ok(V::Bool(a <= b).into()),
                    Gt => return Ok(V::Bool(a > b).into()),
                    Ge => return Ok(V::Bool(a >= b).into()),
                    Add => a + b,
                    Sub => a - b,
                    Mul => a.saturating_mul(b),
                    Div | Rem if b == 0 => return self.err("attempt to divide by zero"),
                    Div => a / b,
                    Rem => a % b,
                    BitAnd => a & b,
                    BitOr => a | b,
                    BitXor => a ^ b,
                    Shl => a.checked_shl(b.clamp(0, 127) as u32).unwrap_or(0),
                    Shr => a >> b.clamp(0, 127),
                    _ => unreachable!(),
                };
                if let Some((min, max)) = range_of(ty)
                    && (v < min || v > max)
                {
                    let what = match op {
                        Sub => "subtract",
                        Add => "add",
                        Mul => "multiply",
                        _ => "compute",
                    };
                    return self.err(format!("attempt to {what} with overflow: {a} {op:?} {b} does not fit in a {ty}"));
                }
                Ok(V::Int(v, ty).into())
            }
        }
    }

    fn index(&self, v: Val, i: Val) -> R<Val> {
        let len = match &v.v {
            V::List(l) => l.len(),
            V::Bytes(b) => b.len(),
            V::Str(s) => s.len(),
            _ => return self.err(format!("{} cannot be indexed", v.kind())),
        };
        let (a, b) = match &i.v {
            V::Struct(n, f) if n == "Range" => {
                let a = f[0].1.as_int().unwrap_or(0);
                let b = match f[1].1.as_int() {
                    Some(-1) => len as i128,
                    b => b.unwrap_or(0) + matches!(f[2].1.v, V::Bool(true)) as i128,
                };
                (a, b)
            }
            _ => match i.as_int() {
                Some(n) => {
                    if n < 0 || n as usize >= len {
                        return self.err(format!("index out of bounds: the len is {len} but the index is {n}"));
                    }
                    let n = n as usize;
                    return Ok(match v.v {
                        V::List(l) => l[n].clone(),
                        V::Bytes(b) => V::Int(b[n] as i128, "u8").into(),
                        V::Str(s) => V::Int(s.as_bytes()[n] as i128, "u8").into(),
                        _ => unreachable!(),
                    });
                }
                None => return self.err(format!("an index is a number or a range, not {}", i.kind())),
            },
        };
        if a < 0 || b < a || b as usize > len {
            return self.err(format!("range {a}..{b} out of bounds of a length {len}"));
        }
        let (a, b) = (a as usize, b as usize);
        Ok(match v.v {
            V::List(l) => V::List(l[a..b].to_vec()).into(),
            V::Bytes(bytes) => V::Bytes(bytes[a..b].to_vec()).into(),
            V::Str(s) => V::Str(String::from_utf8_lossy(&s.as_bytes()[a..b]).into_owned()).into(),
            _ => unreachable!(),
        })
    }

    fn cast(&self, v: Val, ty: &str) -> R<Val> {
        let ty = ty.trim();
        let span = v.span.clone();
        let out = match (&v.v, ty) {
            (V::Int(i, _), "char") => V::Char(char::from_u32(*i as u32).unwrap_or('\u{fffd}')),
            (V::Int(i, _), "f32" | "f64") => V::Float(*i as f64),
            (V::Float(f), t) if range_of(int_ty(t)).is_some() => V::Int(*f as i128, int_ty(t)),
            (V::Float(f), _) => V::Float(*f),
            (V::Char(c), t) => V::Int(wrap(*c as i128, int_ty(t)), int_ty(t)),
            (V::Bool(b), t) => V::Int(*b as i128, int_ty(t)),
            (V::Int(i, _), t) if range_of(int_ty(t)).is_some() => V::Int(wrap(*i, int_ty(t)), int_ty(t)),
            _ => return self.err(format!("cannot cast {} as {ty}", v.kind())),
        };
        Ok(Val { v: out, span })
    }

    // ---- Calls.

    fn apply(&mut self, f: Val, args: Vec<Val>) -> R<Val> {
        match f.v {
            V::Parser(p) => self.apply_parser(&p, &args),
            V::Func(Func::Combinator(name)) => match parsers::build(name, &args) {
                Ok(p) => Ok(V::Parser(Arc::new(p)).into()),
                Err(m) => self.err(m),
            },
            V::Func(func) => self.call(&func, args),
            _ => self.err(format!("{} is not a function", f.kind())),
        }
    }

    /// `parser(input)`: `Ok((rest, value))` or `Err(error)`.
    fn apply_parser(&mut self, p: &Arc<Parser>, args: &[Val]) -> R<Val> {
        let [arg] = args else {
            return self.err("a parser takes one argument, the input: parser(input)");
        };
        let V::Input(pos) = arg.v else {
            return self.err(format!("a parser takes the input, not {}: parser(input)", arg.kind()));
        };
        match self.run(p, pos) {
            Ok((end, v)) => Ok(V::Result(Ok(Box::new(V::Tuple(vec![V::Input(end).into(), v]).into()))).into()),
            Err(Flow::Parse(e)) => Ok(V::Result(Err(Box::new(V::Error(e).into()))).into()),
            Err(e) => Err(e),
        }
    }

    fn call(&mut self, f: &Func, args: Vec<Val>) -> R<Val> {
        match f {
            Func::Closure(c) => {
                if c.params.len() != args.len() {
                    return self.err(format!("the closure takes {} arguments, not {}", c.params.len(), args.len()));
                }
                // The variables of the function the closure was written in.
                self.frames.push(c.env.clone());
                let result = (|| {
                    for (p, a) in c.params.iter().zip(args) {
                        self.bind(p, a)?;
                    }
                    self.expr(&c.body)
                })();
                self.frames.pop();
                result
            }
            Func::Named { name, generic } => self.call_named(name, generic.as_deref(), args),
            Func::Combinator(name) => match parsers::build(name, &args) {
                Ok(p) => Ok(V::Parser(Arc::new(p)).into()),
                Err(m) => self.err(m),
            },
        }
    }

    /// A predicate on an element.
    fn test(&mut self, f: &Func, c: char) -> R<bool> {
        let arg = self.elem_val(c);
        let v = self.call(f, vec![arg])?;
        match v.v {
            V::Bool(b) => Ok(b),
            _ => self.err(format!("the predicate must return a bool, not {}", v.kind())),
        }
    }

    fn call_named(&mut self, name: &str, generic: Option<&str>, mut args: Vec<Val>) -> R<Val> {
        let last = name.rsplit("::").next().unwrap_or(name);
        let ty = name.rsplit("::").nth(1).unwrap_or("");
        let one = |args: &mut Vec<Val>| args.drain(..).next();
        match last {
            "Ok" | "Err" | "Some" => {
                let Some(v) = one(&mut args) else {
                    return self.err(format!("`{last}` takes one value"));
                };
                Ok(match last {
                    "Ok" => V::Result(Ok(Box::new(v))),
                    "Err" => V::Result(Err(Box::new(v))),
                    _ => V::Option(Some(Box::new(v))),
                }
                .into())
            }
            "identity" | "drop" => Ok(one(&mut args).unwrap_or_else(Val::unit)),
            "new" | "default" if args.is_empty() => match ty {
                "String" => Ok(V::Str(String::new()).into()),
                "Vec" => Ok(V::List(vec![]).into()),
                t if range_of(int_ty(t)).is_some() => Ok(V::Int(0, int_ty(t)).into()),
                _ => self.err(format!("`{name}` is not supported here")),
            },
            "from_utf8" | "from_utf8_lossy" => {
                let Some(v) = one(&mut args) else {
                    return self.err(format!("`{name}` takes the bytes"));
                };
                let span = v.span.clone();
                let bytes = v.as_bytes().unwrap_or_default();
                if last == "from_utf8_lossy" {
                    return Ok(Val { v: V::Str(String::from_utf8_lossy(&bytes).into_owned()), span });
                }
                Ok(match String::from_utf8(bytes) {
                    Ok(s) => V::Result(Ok(Box::new(Val { v: V::Str(s), span }))),
                    Err(e) => V::Result(Err(Box::new(V::Str(e.to_string()).into()))),
                }
                .into())
            }
            "from_str_radix" => {
                let (Some(s), Some(radix)) = (args.first().and_then(|s| s.as_bytes()), args.get(1).and_then(Val::as_int)) else {
                    return self.err(format!("{name}(text, radix)"));
                };
                let span = args[0].span.clone();
                let t = int_ty(ty);
                Ok(match i128::from_str_radix(&String::from_utf8_lossy(&s), radix as u32) {
                    Ok(v) if range_of(t).is_none_or(|(a, b)| v >= a && v <= b) => V::Result(Ok(Box::new(Val { v: V::Int(v, t), span }))),
                    _ => V::Result(Err(Box::new(V::Str("invalid digit found in string".into()).into()))),
                }
                .into())
            }
            "from_be_bytes" | "from_le_bytes" => {
                let Some(mut bytes) = args.first().and_then(Val::as_bytes) else {
                    return self.err(format!("{name}([..]) takes an array of bytes"));
                };
                if last == "from_le_bytes" {
                    bytes.reverse();
                }
                let v = bytes.iter().fold(0i128, |acc, b| (acc << 8) | *b as i128);
                Ok(Val { v: V::Int(v, int_ty(ty)), span: args[0].span.clone() })
            }
            "from" | "try_from" | "new" if range_of(int_ty(ty)).is_some() || ty == "char" || ty == "String" => {
                let Some(v) = one(&mut args) else {
                    return self.err(format!("`{name}` takes one value"));
                };
                if ty == "String" {
                    return self.method(v, "to_string", None, vec![]);
                }
                let out = self.cast(v, ty)?;
                Ok(if last == "try_from" { V::Result(Ok(Box::new(out))).into() } else { out })
            }
            // nom::character::is_digit and the like, on a byte.
            "is_digit" | "is_alphabetic" | "is_alphanumeric" | "is_space" | "is_newline" | "is_hex_digit" | "is_oct_digit"
                if ty.is_empty() || ty == "character" =>
            {
                let Some(v) = one(&mut args) else {
                    return self.err(format!("`{name}` takes a byte"));
                };
                let method = match last {
                    "is_digit" => "is_dec_digit",
                    "is_alphabetic" => "is_alpha",
                    "is_alphanumeric" => "is_alphanum",
                    other => other,
                };
                self.method(v, method, None, vec![])
            }
            _ if !args.is_empty() && !ty.is_empty() => {
                // `str::parse::<u32>(s)`, `char::is_alphabetic(c)`: the method of the first argument.
                let recv = args.remove(0);
                self.method(recv, last, generic, args)
            }
            _ => self.err(format!("`{name}` is not supported here")),
        }
    }

    fn method(&mut self, r: Val, name: &str, generic: Option<&str>, mut args: Vec<Val>) -> R<Val> {
        // A tuple of parsers is a parser in nom 8: `(alpha1, digit1).parse(input)`.
        if let V::Tuple(items) = &r.v
            && matches!(name, "parse" | "parse_complete" | "map" | "map_res" | "map_opt" | "verify")
            && items.iter().all(|i| matches!(i.v, V::Parser(_) | V::Str(_) | V::Char(_) | V::Tuple(_)))
        {
            return match parsers::build("tuple", std::slice::from_ref(&r)) {
                Ok(p) => self.method(V::Parser(Arc::new(p)).into(), name, generic, args),
                Err(m) => self.err(m),
            };
        }
        let span = r.span.clone();
        let keep = |v: V| Val { v, span: span.clone() };
        // The parsers: `parser.parse(input)`, `parser.map(f)`.
        if let V::Parser(p) = &r.v {
            let p = p.clone();
            return match (name, args.len()) {
                ("parse" | "parse_complete", 1) => self.apply_parser(&p, &args),
                ("map", 1) | ("map_res", 1) | ("map_opt", 1) | ("verify", 1) => {
                    let V::Func(f) = args.remove(0).v else {
                        return self.err(format!("`.{name}` takes a function"));
                    };
                    Ok(V::Parser(Arc::new(match name {
                        "map" => Parser::Map(p, f),
                        "map_res" => Parser::MapRes(p, f),
                        "map_opt" => Parser::MapOpt(p, f),
                        _ => Parser::Verify(p, f),
                    }))
                    .into())
                }
                ("and", 1) => match parsers::build("pair", &[r.clone(), args.remove(0)]) {
                    Ok(p) => Ok(V::Parser(Arc::new(p)).into()),
                    Err(m) => self.err(m),
                },
                ("or", 1) => match parsers::build("alt", &[V::Tuple(vec![r.clone(), args.remove(0)]).into()]) {
                    Ok(p) => Ok(V::Parser(Arc::new(p)).into()),
                    Err(m) => self.err(m),
                },
                _ => self.err(format!("parsers have no method `.{name}`: use `.parse(input)`")),
            };
        }
        if let (V::Func(f), "parse") = (&r.v, name) {
            let f = f.clone();
            return self.call(&f, args);
        }
        let int = r.as_int();
        let ch = match &r.v {
            V::Char(c) => Some(*c),
            V::Int(v, _) => u32::try_from(*v).ok().and_then(char::from_u32),
            _ => None,
        };
        let pred = |f: fn(&char) -> bool| -> R<Val> {
            match ch {
                Some(c) => Ok(V::Bool(f(&c)).into()),
                None => self.err(format!("`.{name}()` needs a char or a byte, not {}", r.kind())),
            }
        };
        match name {
            "to_string" | "to_owned" | "into" | "clone" | "as_ref" | "borrow" | "iter" | "into_iter" | "collect" | "as_slice"
            | "to_vec" | "as_str" | "cloned" | "copied" | "deref" => Ok(match (&r.v, name) {
                (V::Int(v, _), "to_string") => keep(V::Str(v.to_string())),
                (V::Char(c), "to_string") => keep(V::Str(c.to_string())),
                (V::Float(f), "to_string") => keep(V::Str(f.to_string())),
                (V::Bytes(_), "to_string") => return self.err("bytes have no `.to_string()`: String::from_utf8(bytes.to_vec())"),
                _ => r,
            }),
            "as_bytes" | "into_bytes" => Ok(keep(V::Bytes(r.as_bytes().unwrap_or_default()))),
            "len" => match &r.v {
                V::Str(s) => Ok(V::Int(s.len() as i128, "usize").into()),
                V::Bytes(b) => Ok(V::Int(b.len() as i128, "usize").into()),
                V::List(l) => Ok(V::Int(l.len() as i128, "usize").into()),
                _ => self.err(format!("{} has no `.len()`", r.kind())),
            },
            "is_empty" => match &r.v {
                V::Str(s) => Ok(V::Bool(s.is_empty()).into()),
                V::Bytes(b) => Ok(V::Bool(b.is_empty()).into()),
                V::List(l) => Ok(V::Bool(l.is_empty()).into()),
                _ => self.err(format!("{} has no `.is_empty()`", r.kind())),
            },
            "parse" => {
                let text = match &r.v {
                    V::Str(s) => s.clone(),
                    V::Char(c) => c.to_string(),
                    _ => return self.err(format!("`.parse()` is a method of strings, not of {}", r.kind())),
                };
                let Some(ty) = generic.map(str::trim) else {
                    return self.err("say the type: .parse::<u32>()");
                };
                Ok(V::Result(match ty {
                    "f32" | "f64" => {
                        text.parse::<f64>().map(|f| Box::new(keep(V::Float(f)))).map_err(|e| Box::new(V::Str(e.to_string()).into()))
                    }
                    "String" => Ok(Box::new(keep(V::Str(text)))),
                    "char" => match text.chars().collect::<Vec<_>>()[..] {
                        [c] => Ok(Box::new(keep(V::Char(c)))),
                        _ => Err(Box::new(V::Str("too many characters in string".into()).into())),
                    },
                    "bool" => text.parse::<bool>().map(|b| Box::new(keep(V::Bool(b)))).map_err(|e| Box::new(V::Str(e.to_string()).into())),
                    t => {
                        let t = int_ty(t);
                        if range_of(t).is_none() {
                            return self.err(format!("cannot parse into {ty}"));
                        }
                        match text.parse::<i128>() {
                            Ok(v) if range_of(t).is_some_and(|(a, b)| v >= a && v <= b) => Ok(Box::new(keep(V::Int(v, t)))),
                            Ok(_) => Err(Box::new(V::Str(format!("number too large to fit in target type {t}")).into())),
                            Err(e) => Err(Box::new(V::Str(e.to_string()).into())),
                        }
                    }
                })
                .into())
            }
            "unwrap" | "expect" | "unwrap_or" | "unwrap_or_default" | "unwrap_or_else" => match r.v {
                V::Result(Ok(v)) | V::Option(Some(v)) => Ok(*v),
                V::Result(Err(e)) if name == "unwrap" || name == "expect" => {
                    self.err(format!("called `Result::unwrap()` on an `Err` value: {}", e.show(60)))
                }
                V::Option(None) if name == "unwrap" || name == "expect" => self.err("called `Option::unwrap()` on a `None` value"),
                _ if name == "unwrap_or" && !args.is_empty() => Ok(args.remove(0)),
                _ if name == "unwrap_or_default" => Ok(V::Int(0, "").into()),
                _ => self.err(format!("`.{name}` on {}", r.kind())),
            },
            "ok" => match r.v {
                V::Result(Ok(v)) => Ok(V::Option(Some(v)).into()),
                V::Result(Err(_)) => Ok(V::Option(None).into()),
                _ => self.err("`.ok()` is a method of Result"),
            },
            "is_ok" | "is_some" => Ok(V::Bool(matches!(r.v, V::Result(Ok(_)) | V::Option(Some(_)))).into()),
            "is_err" | "is_none" => Ok(V::Bool(matches!(r.v, V::Result(Err(_)) | V::Option(None))).into()),
            "map" if matches!(r.v, V::Option(_) | V::Result(_) | V::List(_)) => {
                let Some(V::Func(f)) = args.pop().map(|a| a.v) else {
                    return self.err("`.map` takes a function");
                };
                match r.v {
                    V::Option(Some(v)) => Ok(V::Option(Some(Box::new(self.call(&f, vec![*v])?))).into()),
                    V::Result(Ok(v)) => Ok(V::Result(Ok(Box::new(self.call(&f, vec![*v])?))).into()),
                    V::List(items) => {
                        let out = items.into_iter().map(|i| self.call(&f, vec![i])).collect::<R<Vec<_>>>()?;
                        Ok(keep(V::List(out)))
                    }
                    other => Ok(other.into()),
                }
            }
            "trim" | "trim_start" | "trim_end" | "to_uppercase" | "to_lowercase" | "to_ascii_uppercase" | "to_ascii_lowercase" => {
                match &r.v {
                    V::Str(s) => Ok(keep(V::Str(match name {
                        "trim" => s.trim().to_string(),
                        "trim_start" => s.trim_start().to_string(),
                        "trim_end" => s.trim_end().to_string(),
                        "to_uppercase" | "to_ascii_uppercase" => s.to_uppercase(),
                        _ => s.to_lowercase(),
                    }))),
                    V::Char(c) => Ok(keep(V::Char(if name.contains("upper") { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }))),
                    _ => self.err(format!("`.{name}()` is a method of strings")),
                }
            }
            "contains" | "starts_with" | "ends_with" => {
                let Some(arg) = args.first() else {
                    return self.err(format!("`.{name}` takes one argument"));
                };
                if let V::Struct(n, fields) = &r.v
                    && n == "Range"
                {
                    let (Some(a), Some(b), Some(x)) = (fields[0].1.as_int(), fields[1].1.as_int(), arg.as_int()) else {
                        return self.err("a range of numbers or chars");
                    };
                    let inclusive = matches!(fields[2].1.v, V::Bool(true));
                    return Ok(V::Bool(x >= a && if inclusive { x <= b } else { x < b }).into());
                }
                if let V::List(items) = &r.v {
                    return Ok(V::Bool(items.iter().any(|i| i.same(arg))).into());
                }
                let (Some(hay), Some(needle)) = (r.as_bytes(), arg.as_bytes()) else {
                    return self.err(format!("`.{name}` on {}", r.kind()));
                };
                Ok(V::Bool(match name {
                    "contains" => needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle),
                    "starts_with" => hay.starts_with(&needle),
                    _ => hay.ends_with(&needle),
                })
                .into())
            }
            "first" | "last" => match &r.v {
                V::List(items) => Ok(V::Option((if name == "first" { items.first() } else { items.last() }).cloned().map(Box::new)).into()),
                _ => self.err(format!("`.{name}()` is a method of Vec")),
            },
            "sum" => match &r.v {
                V::List(items) => Ok(V::Int(items.iter().filter_map(Val::as_int).sum(), "").into()),
                _ => self.err("`.sum()` is a method of Vec"),
            },
            "get" => match (&r.v, args.first().and_then(Val::as_int)) {
                (V::List(items), Some(i)) => Ok(V::Option(items.get(i as usize).cloned().map(Box::new)).into()),
                _ => self.err("`.get(i)` is a method of Vec"),
            },
            "min" | "max" | "pow" | "checked_sub" | "checked_add" | "saturating_sub" | "wrapping_sub" | "wrapping_add" | "abs"
            | "count_ones" => {
                let (Some(a), V::Int(_, t)) = (int, &r.v) else {
                    return self.err(format!("`.{name}` is a method of integers"));
                };
                let t: &'static str = t;
                let b = args.first().and_then(Val::as_int).unwrap_or(0);
                let fits = |v: i128| range_of(t).is_none_or(|(lo, hi)| v >= lo && v <= hi);
                Ok(match name {
                    "min" => V::Int(a.min(b), t),
                    "max" => V::Int(a.max(b), t),
                    "pow" => V::Int(a.saturating_pow(b as u32), t),
                    "abs" => V::Int(a.abs(), t),
                    "count_ones" => V::Int((a as u128).count_ones() as i128, "u32"),
                    "checked_sub" => V::Option(fits(a - b).then(|| Box::new(V::Int(a - b, t).into()))),
                    "checked_add" => V::Option(fits(a + b).then(|| Box::new(V::Int(a + b, t).into()))),
                    "saturating_sub" => V::Int(range_of(t).map_or(a - b, |(lo, _)| (a - b).max(lo)), t),
                    "wrapping_sub" => V::Int(wrap(a - b, t), t),
                    _ => V::Int(wrap(a + b, t), t),
                }
                .into())
            }
            "to_digit" => {
                let radix = args.first().and_then(Val::as_int).unwrap_or(10) as u32;
                match ch {
                    Some(c) => Ok(V::Option(c.to_digit(radix).map(|d| Box::new(V::Int(d as i128, "u32").into()))).into()),
                    None => self.err("`.to_digit()` is a method of char"),
                }
            }
            "is_digit" => {
                let radix = args.first().and_then(Val::as_int).unwrap_or(10) as u32;
                match ch {
                    Some(c) => Ok(V::Bool(c.is_digit(radix)).into()),
                    None => self.err("`.is_digit()` is a method of char"),
                }
            }
            "as_char" => match ch {
                Some(c) => Ok(keep(V::Char(c))),
                None => self.err("`.as_char()` on a char or a byte"),
            },
            "is_ascii_digit" | "is_dec_digit" => pred(char::is_ascii_digit),
            "is_ascii_alphabetic" | "is_alpha" => pred(char::is_ascii_alphabetic),
            "is_ascii_alphanumeric" | "is_alphanum" => pred(char::is_ascii_alphanumeric),
            "is_ascii_whitespace" => pred(char::is_ascii_whitespace),
            "is_ascii_hexdigit" | "is_hex_digit" => pred(char::is_ascii_hexdigit),
            "is_oct_digit" => pred(|c| ('0'..='7').contains(c)),
            "is_space" => pred(|c| *c == ' ' || *c == '\t'),
            "is_newline" => pred(|c| *c == '\n'),
            "is_ascii_punctuation" => pred(char::is_ascii_punctuation),
            "is_ascii_graphic" => pred(char::is_ascii_graphic),
            "is_ascii_uppercase" => pred(char::is_ascii_uppercase),
            "is_ascii_lowercase" => pred(char::is_ascii_lowercase),
            "is_ascii_control" => pred(char::is_ascii_control),
            "is_ascii" => pred(char::is_ascii),
            "is_alphabetic" => pred(|c| c.is_alphabetic()),
            "is_numeric" => pred(|c| c.is_numeric()),
            "is_alphanumeric" => pred(|c| c.is_alphanumeric()),
            "is_whitespace" => pred(|c| c.is_whitespace()),
            "is_uppercase" => pred(|c| c.is_uppercase()),
            "is_lowercase" => pred(|c| c.is_lowercase()),
            _ => self.err(format!("{} has no method `.{name}` here", r.kind())),
        }
    }

    // ---- The parsers.

    fn step(&mut self) -> R<()> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return self.err("the parser ran too long: a many0 or a recursion that never ends?");
        }
        Ok(())
    }

    /// Runs a parser at `pos`: the end and the value, its span set to the bytes consumed.
    fn run(&mut self, p: &Parser, pos: usize) -> R<(usize, Val)> {
        self.step()?;
        let (end, mut v) = self.run_inner(p, pos)?;
        if v.span.is_none() {
            v.span = Some(pos..end);
        }
        Ok((end, v))
    }

    fn fail<T>(&self, pos: usize, kind: &str) -> R<T> {
        Err(Flow::Parse(PErr::new(pos, kind)))
    }

    /// The elements from `pos` while `f` holds, at most `max`: the end and the count.
    fn scan(&mut self, pos: usize, max: usize, mut f: impl FnMut(&mut Self, char) -> R<bool>) -> R<(usize, usize)> {
        let mut end = pos;
        let mut n = 0;
        while n < max {
            let Some((c, len)) = self.elem(end) else { break };
            if !f(self, c)? {
                break;
            }
            end += len;
            n += 1;
        }
        Ok((end, n))
    }

    fn builtin(&mut self, name: &'static str, pos: usize) -> R<(usize, Val)> {
        let class = |f: fn(char) -> bool, min: usize, kind: &'static str| (f, min, kind);
        let simple = match name {
            "alpha0" => Some(class(|c| c.is_ascii_alphabetic(), 0, "Alpha")),
            "alpha1" => Some(class(|c| c.is_ascii_alphabetic(), 1, "Alpha")),
            "digit0" => Some(class(|c| c.is_ascii_digit(), 0, "Digit")),
            "digit1" => Some(class(|c| c.is_ascii_digit(), 1, "Digit")),
            "alphanumeric0" => Some(class(|c| c.is_ascii_alphanumeric(), 0, "AlphaNumeric")),
            "alphanumeric1" => Some(class(|c| c.is_ascii_alphanumeric(), 1, "AlphaNumeric")),
            "space0" => Some(class(|c| c == ' ' || c == '\t', 0, "Space")),
            "space1" => Some(class(|c| c == ' ' || c == '\t', 1, "Space")),
            "multispace0" => Some(class(|c| matches!(c, ' ' | '\t' | '\r' | '\n'), 0, "MultiSpace")),
            "multispace1" => Some(class(|c| matches!(c, ' ' | '\t' | '\r' | '\n'), 1, "MultiSpace")),
            "hex_digit0" => Some(class(|c| c.is_ascii_hexdigit(), 0, "HexDigit")),
            "hex_digit1" => Some(class(|c| c.is_ascii_hexdigit(), 1, "HexDigit")),
            "oct_digit0" => Some(class(|c| ('0'..='7').contains(&c), 0, "OctDigit")),
            "oct_digit1" => Some(class(|c| ('0'..='7').contains(&c), 1, "OctDigit")),
            "not_line_ending" => Some(class(|c| c != '\n' && c != '\r', 0, "Tag")),
            "rest" => Some(class(|_| true, 0, "Eof")),
            _ => None,
        };
        if let Some((f, min, kind)) = simple {
            let (end, n) = self.scan(pos, usize::MAX, |_, c| Ok(f(c)))?;
            if n < min {
                return self.fail(pos, kind);
            }
            return Ok((end, self.slice(pos, end)));
        }
        let at = |s: &Self, b: &[u8]| s.input[pos..s.end].starts_with(b);
        match name {
            "newline" | "tab" => {
                let c = if name == "newline" { '\n' } else { '\t' };
                match self.elem(pos) {
                    Some((x, _)) if x == c => Ok((pos + 1, V::Char(c).into())),
                    _ => self.fail(pos, "Char"),
                }
            }
            "line_ending" | "crlf" => {
                if at(self, b"\r\n") {
                    Ok((pos + 2, self.slice(pos, pos + 2)))
                } else if name == "line_ending" && at(self, b"\n") {
                    Ok((pos + 1, self.slice(pos, pos + 1)))
                } else {
                    self.fail(pos, "CrLf")
                }
            }
            "anychar" => match self.elem(pos) {
                Some((c, len)) => Ok((pos + len, V::Char(c).into())),
                None => self.fail(pos, "Eof"),
            },
            "eof" => {
                if pos >= self.end {
                    Ok((pos, self.slice(pos, pos)))
                } else {
                    self.fail(pos, "Eof")
                }
            }
            "rest_len" => Ok((pos, V::Int((self.end - pos) as i128, "usize").into())),
            "float" | "double" => {
                let s = &self.input[pos..self.end];
                let mut i = 0;
                if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
                    i += 1;
                }
                let digits = |i: &mut usize| {
                    let start = *i;
                    while *i < s.len() && s[*i].is_ascii_digit() {
                        *i += 1;
                    }
                    *i > start
                };
                let int = digits(&mut i);
                let mut frac = false;
                if i < s.len() && s[i] == b'.' {
                    i += 1;
                    frac = digits(&mut i);
                }
                if !int && !frac {
                    return self.fail(pos, "Float");
                }
                if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
                    let save = i;
                    i += 1;
                    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
                        i += 1;
                    }
                    if !digits(&mut i) {
                        i = save;
                    }
                }
                let v: f64 = String::from_utf8_lossy(&s[..i]).parse().unwrap_or_default();
                Ok((pos + i, V::Float(v).into()))
            }
            _ if name.starts_with("be_") || name.starts_with("le_") => {
                let ty = &name[3..];
                let size = match ty {
                    "u8" | "i8" => 1,
                    "u16" | "i16" => 2,
                    "u24" | "i24" => 3,
                    "u32" | "i32" | "f32" => 4,
                    "u64" | "i64" | "f64" => 8,
                    _ => 16,
                };
                if pos + size > self.end {
                    return self.fail(pos, "Eof");
                }
                let mut bytes = self.input[pos..pos + size].to_vec();
                if name.starts_with("le_") {
                    bytes.reverse();
                }
                let raw = bytes.iter().fold(0u128, |acc, b| (acc << 8) | *b as u128);
                let v = match ty {
                    "f32" => V::Float(f32::from_bits(raw as u32) as f64),
                    "f64" => V::Float(f64::from_bits(raw as u64)),
                    t if t.starts_with('i') => {
                        let bits = size * 8;
                        let v = if bits == 128 { raw as i128 } else { ((raw << (128 - bits)) as i128) >> (128 - bits) };
                        V::Int(v, int_ty(if t == "i24" { "i32" } else { t }))
                    }
                    t => V::Int(raw as i128, int_ty(if t == "u24" { "u32" } else { t })),
                };
                Ok((pos + size, v.into()))
            }
            _ => {
                // The numbers in text: `u32` of nom::character::complete.
                let ty = int_ty(name);
                let Some((lo, hi)) = range_of(ty) else {
                    return self.err(format!("`{name}` is not a parser"));
                };
                let signed = name.starts_with('i');
                let mut end = pos;
                let mut neg = false;
                if signed && let Some((c @ ('+' | '-'), _)) = self.elem(end) {
                    neg = c == '-';
                    end += 1;
                }
                let (digits_end, n) = self.scan(end, usize::MAX, |_, c| Ok(c.is_ascii_digit()))?;
                if n == 0 {
                    return self.fail(pos, "Digit");
                }
                let text = String::from_utf8_lossy(&self.input[end..digits_end]).into_owned();
                match text.parse::<i128>() {
                    Ok(v) => {
                        let v = if neg { -v } else { v };
                        if v < lo || v > hi {
                            return self.fail(pos, "Digit");
                        }
                        Ok((digits_end, V::Int(v, ty).into()))
                    }
                    Err(_) => self.fail(pos, "Digit"),
                }
            }
        }
    }

    fn run_list(&mut self, p: &Parser, pos: usize, min: usize, max: usize, kind: &'static str) -> R<(usize, Val)> {
        let mut items = Vec::new();
        let mut at = pos;
        while items.len() < max {
            match self.run(p, at) {
                Ok((end, v)) => {
                    if end == at {
                        // nom stops a loop whose parser consumes nothing.
                        return self.fail(at, kind);
                    }
                    items.push(v);
                    at = end;
                }
                Err(Flow::Parse(e)) if !e.failure => {
                    if items.len() < min {
                        return Err(Flow::Parse(e));
                    }
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok((at, V::List(items).into()))
    }

    fn length(&mut self, len: &Parser, pos: usize) -> R<(usize, usize)> {
        let (at, n) = self.run(len, pos)?;
        match n.as_int() {
            Some(n) if n >= 0 => Ok((at, n as usize)),
            _ => self.err(format!("the length parser must give a positive number, not {}", n.show(30))),
        }
    }

    fn run_inner(&mut self, p: &Parser, pos: usize) -> R<(usize, Val)> {
        match p {
            Parser::Tag(t) => {
                if self.input[pos..self.end].starts_with(t) {
                    Ok((pos + t.len(), self.slice(pos, pos + t.len())))
                } else {
                    self.fail(pos, "Tag")
                }
            }
            Parser::TagNoCase(t) => {
                let rest = &self.input[pos..self.end];
                if rest.len() >= t.len() && rest[..t.len()].eq_ignore_ascii_case(t) {
                    Ok((pos + t.len(), self.slice(pos, pos + t.len())))
                } else {
                    self.fail(pos, "Tag")
                }
            }
            Parser::Take(n) => {
                let (end, got) = self.scan(pos, *n, |_, _| Ok(true))?;
                if got < *n {
                    return self.fail(pos, "Eof");
                }
                Ok((end, self.slice(pos, end)))
            }
            Parser::TakeUntil { tag, one } => {
                let rest = &self.input[pos..self.end];
                let found = if tag.is_empty() { Some(0) } else { rest.windows(tag.len()).position(|w| w == tag.as_slice()) };
                match found {
                    Some(0) if *one => self.fail(pos, "TakeUntil"),
                    Some(i) => Ok((pos + i, self.slice(pos, pos + i))),
                    None => self.fail(pos, "TakeUntil"),
                }
            }
            Parser::TakeWhile { pred, min, max, till, kind } => {
                let (end, n) = self.scan(pos, *max, |s, c| Ok(s.test(pred, c)? != *till))?;
                if n < *min {
                    return self.fail(pos, kind);
                }
                Ok((end, self.slice(pos, end)))
            }
            Parser::IsA(set) | Parser::IsNot(set) => {
                let is_a = matches!(p, Parser::IsA(_));
                let set: Vec<char> =
                    if self.text { String::from_utf8_lossy(set).chars().collect() } else { set.iter().map(|&b| b as char).collect() };
                let (end, n) = self.scan(pos, usize::MAX, |_, c| Ok(set.contains(&c) == is_a))?;
                if n == 0 {
                    return self.fail(pos, if is_a { "IsA" } else { "IsNot" });
                }
                Ok((end, self.slice(pos, end)))
            }
            Parser::Builtin(name) => self.builtin(name, pos),
            Parser::Char(c) => match self.elem(pos) {
                Some((x, len)) if x == *c => Ok((pos + len, V::Char(*c).into())),
                _ => self.fail(pos, "Char"),
            },
            Parser::OneOf(set) | Parser::NoneOf(set) => {
                let one = matches!(p, Parser::OneOf(_));
                match self.elem(pos) {
                    Some((x, len)) if set.contains(&x) == one => Ok((pos + len, V::Char(x).into())),
                    _ => self.fail(pos, if one { "OneOf" } else { "NoneOf" }),
                }
            }
            Parser::Satisfy(f) => match self.elem(pos) {
                Some((x, len)) if self.test(f, x)? => Ok((pos + len, V::Char(x).into())),
                _ => self.fail(pos, "Satisfy"),
            },
            Parser::Count(p, n) => {
                let mut items = Vec::with_capacity((*n).min(4096));
                let mut at = pos;
                for _ in 0..*n {
                    let (end, v) = self.run(p, at)?;
                    items.push(v);
                    at = end;
                }
                Ok((at, V::List(items).into()))
            }
            Parser::Many { p, min, max, kind } => {
                let (end, v) = self.run_list(p, pos, *min, *max, kind)?;
                if let V::List(items) = &v.v
                    && items.len() < *min
                {
                    return self.fail(pos, kind);
                }
                Ok((end, v))
            }
            Parser::SepList { sep, p, one } => {
                let mut items = Vec::new();
                let mut at = pos;
                match self.run(p, at) {
                    Ok((end, v)) => {
                        items.push(v);
                        at = end;
                    }
                    Err(Flow::Parse(e)) if !e.failure && !*one => return Ok((pos, V::List(items).into())),
                    Err(e) => return Err(e),
                }
                loop {
                    let after_sep = match self.run(sep, at) {
                        Ok((end, _)) => end,
                        Err(Flow::Parse(e)) if !e.failure => break,
                        Err(e) => return Err(e),
                    };
                    if after_sep == at {
                        return self.fail(at, "SeparatedList");
                    }
                    match self.run(p, after_sep) {
                        Ok((end, v)) => {
                            items.push(v);
                            at = end;
                        }
                        Err(Flow::Parse(e)) if !e.failure => break,
                        Err(e) => return Err(e),
                    }
                }
                Ok((at, V::List(items).into()))
            }
            Parser::ManyTill(p, end_p) => {
                let mut items = Vec::new();
                let mut at = pos;
                loop {
                    match self.run(end_p, at) {
                        Ok((end, v)) => return Ok((end, V::Tuple(vec![V::List(items).into(), v]).into())),
                        Err(Flow::Parse(e)) if !e.failure => {}
                        Err(e) => return Err(e),
                    }
                    let (end, v) = self.run(p, at)?;
                    if end == at {
                        return self.fail(at, "ManyTill");
                    }
                    items.push(v);
                    at = end;
                }
            }
            Parser::LengthData(len) => {
                let (at, n) = self.length(len, pos)?;
                if at + n > self.end {
                    return self.fail(at, "Eof");
                }
                Ok((at + n, self.slice(at, at + n)))
            }
            Parser::LengthValue(len, p) => {
                let (at, n) = self.length(len, pos)?;
                if at + n > self.end {
                    return self.fail(at, "Eof");
                }
                let saved = self.end;
                self.end = at + n;
                let result = self.run(p, at);
                self.end = saved;
                let (_, v) = result?;
                Ok((at + n, v))
            }
            Parser::LengthCount(len, p) => {
                let (at, n) = self.length(len, pos)?;
                let count = Parser::Count(p.clone(), n);
                self.run(&count, at)
            }
            Parser::Seq(ps) => {
                let mut items = Vec::new();
                let mut at = pos;
                for p in ps {
                    let (end, v) = self.run(p, at)?;
                    items.push(v);
                    at = end;
                }
                Ok((at, V::Tuple(items).into()))
            }
            Parser::Preceded(a, b) => {
                let (at, _) = self.run(a, pos)?;
                self.run(b, at)
            }
            Parser::Terminated(a, b) => {
                let (at, v) = self.run(a, pos)?;
                let (end, _) = self.run(b, at)?;
                Ok((end, v))
            }
            Parser::Delimited(a, b, c) => {
                let (at, _) = self.run(a, pos)?;
                let (at, v) = self.run(b, at)?;
                let (end, _) = self.run(c, at)?;
                Ok((end, v))
            }
            Parser::SeparatedPair(a, s, b) => {
                let (at, x) = self.run(a, pos)?;
                let (at, _) = self.run(s, at)?;
                let (end, y) = self.run(b, at)?;
                Ok((end, V::Tuple(vec![x, y]).into()))
            }
            Parser::Alt(ps) => {
                let mut last = PErr::new(pos, "Alt");
                for p in ps {
                    match self.run(p, pos) {
                        Err(Flow::Parse(e)) if !e.failure => last = e,
                        other => return other,
                    }
                }
                Err(Flow::Parse(last))
            }
            Parser::Opt(p) => match self.run(p, pos) {
                Ok((end, v)) => {
                    let span = v.span.clone();
                    Ok((end, Val { v: V::Option(Some(Box::new(v))), span }))
                }
                Err(Flow::Parse(e)) if !e.failure => Ok((pos, V::Option(None).into())),
                Err(e) => Err(e),
            },
            Parser::Recognize(p) => {
                let (end, _) = self.run(p, pos)?;
                Ok((end, self.slice(pos, end)))
            }
            Parser::Consumed(p) => {
                let (end, v) = self.run(p, pos)?;
                Ok((end, V::Tuple(vec![self.slice(pos, end), v]).into()))
            }
            Parser::Value(v, p) => {
                let (end, _) = self.run(p, pos)?;
                Ok((end, Val { v: v.v.clone(), span: Some(pos..end) }))
            }
            Parser::Not(p) => match self.run(p, pos) {
                Ok(_) => self.fail(pos, "Not"),
                Err(Flow::Parse(e)) if !e.failure => Ok((pos, Val::unit())),
                Err(e) => Err(e),
            },
            Parser::Peek(p) => {
                let (_, v) = self.run(p, pos)?;
                Ok((pos, v))
            }
            Parser::Cond(b, p) => {
                if *b {
                    let (end, v) = self.run(p, pos)?;
                    let span = v.span.clone();
                    Ok((end, Val { v: V::Option(Some(Box::new(v))), span }))
                } else {
                    Ok((pos, V::Option(None).into()))
                }
            }
            Parser::Verify(p, f) => {
                let (end, v) = self.run(p, pos)?;
                let ok = self.call(f, vec![v.clone()])?;
                match ok.v {
                    V::Bool(true) => Ok((end, v)),
                    V::Bool(false) => self.fail(pos, "Verify"),
                    _ => self.err("the function of `verify` must return a bool"),
                }
            }
            Parser::Map(p, f) => {
                let (end, v) = self.run(p, pos)?;
                let span = v.span.clone();
                let mut out = self.call(f, vec![v])?;
                if out.span.is_none() {
                    out.span = span;
                }
                Ok((end, out))
            }
            Parser::MapRes(p, f) | Parser::MapOpt(p, f) => {
                let (end, v) = self.run(p, pos)?;
                let span = v.span.clone();
                let out = self.call(f, vec![v])?;
                match out.v {
                    V::Result(Ok(x)) | V::Option(Some(x)) => {
                        let mut x = *x;
                        if x.span.is_none() {
                            x.span = span;
                        }
                        Ok((end, x))
                    }
                    V::Result(Err(_)) => self.fail(pos, "MapRes"),
                    V::Option(None) => self.fail(pos, "MapOpt"),
                    _ => self.err(format!("the function of map_res must return a Result (map_opt an Option), not {}", out.kind())),
                }
            }
            Parser::AllConsuming(p) => {
                let (end, v) = self.run(p, pos)?;
                if end != self.end {
                    return self.fail(end, "Eof");
                }
                Ok((end, v))
            }
            Parser::Complete(p) => self.run(p, pos),
            Parser::Cut(p) => match self.run(p, pos) {
                Err(Flow::Parse(mut e)) => {
                    e.failure = true;
                    Err(Flow::Parse(e))
                }
                other => other,
            },
            Parser::Success(v) => Ok((pos, v.clone())),
            Parser::Fail => self.fail(pos, "Fail"),
            Parser::Context(ctx, p) | Parser::DbgPeekHex(p, ctx) => match self.run(p, pos) {
                Err(Flow::Parse(mut e)) => {
                    self.event(Event::PeekError { context: ctx.clone(), pos, kind: e.kind.clone(), line: self.line });
                    e.contexts.push(ctx.clone());
                    Err(Flow::Parse(e))
                }
                other => other,
            },
            Parser::UserFn(name) => {
                let Some(f) = self.prog.get(name) else {
                    return self.err(format!("no function `{name}`"));
                };
                self.call_fn(f, pos)
            }
            Parser::Closure(f) => {
                let out = self.call(f, vec![V::Input(pos).into()])?;
                match out.v {
                    V::Result(Ok(t)) => match t.v {
                        V::Tuple(items) if items.len() == 2 => match items[0].v {
                            V::Input(end) => Ok((end, items[1].clone())),
                            _ => self.err("a closure parser returns Ok((input, value))"),
                        },
                        _ => self.err("a closure parser returns Ok((input, value))"),
                    },
                    V::Result(Err(e)) => match e.v {
                        V::Error(pe) => Err(Flow::Parse(pe)),
                        _ => self.err("a closure parser returns a nom error"),
                    },
                    _ => self.err(format!("a closure parser returns a Result, not {}", out.kind())),
                }
            }
        }
    }
}

/// The functions known by their bare name.
const KNOWN_FNS: &[&str] = &[
    "Ok",
    "Err",
    "Some",
    "identity",
    "drop",
    "is_digit",
    "is_alphabetic",
    "is_alphanumeric",
    "is_space",
    "is_newline",
    "is_hex_digit",
    "is_oct_digit",
];

/// The `&'static str` of an integer type, `""` for another type.
pub fn int_ty(t: &str) -> &'static str {
    match t.trim() {
        "u8" => "u8",
        "u16" => "u16",
        "u32" => "u32",
        "u64" => "u64",
        "u128" => "u128",
        "usize" => "usize",
        "i8" => "i8",
        "i16" => "i16",
        "i32" => "i32",
        "i64" => "i64",
        "i128" => "i128",
        "isize" => "isize",
        _ => "",
    }
}

/// The bounds of an integer type.
pub fn range_of(t: &str) -> Option<(i128, i128)> {
    Some(match t {
        "u8" => (0, u8::MAX as i128),
        "u16" => (0, u16::MAX as i128),
        "u32" => (0, u32::MAX as i128),
        "u64" | "usize" => (0, u64::MAX as i128),
        "u128" => (0, i128::MAX),
        "i8" => (i8::MIN as i128, i8::MAX as i128),
        "i16" => (i16::MIN as i128, i16::MAX as i128),
        "i32" => (i32::MIN as i128, i32::MAX as i128),
        "i64" | "isize" => (i64::MIN as i128, i64::MAX as i128),
        "i128" => (i128::MIN, i128::MAX),
        _ => return None,
    })
}

/// `v as t`: the bits that fit.
fn wrap(v: i128, t: &str) -> i128 {
    let bits = match t {
        "u8" | "i8" => 8,
        "u16" | "i16" => 16,
        "u32" | "i32" => 32,
        "u64" | "i64" | "usize" | "isize" => 64,
        _ => return v,
    };
    let mask = (1i128 << bits) - 1;
    let u = v & mask;
    if t.starts_with('i') && u >> (bits - 1) == 1 { u - (1i128 << bits) } else { u }
}
