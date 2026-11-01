//! The values of the interpreter: the data parsed, the inputs, the parsers and the functions.
use std::ops::Range;
use std::sync::Arc;

use super::ast::{Expr, Pat};
use super::parsers::Parser;

/// A range of bytes of the payload.
pub type Span = Range<usize>;

#[derive(Debug, Clone)]
pub struct Val {
    pub v: V,
    /// The bytes this value was parsed from.
    pub span: Option<Span>,
}

#[derive(Debug, Clone)]
pub enum V {
    Unit,
    Bool(bool),
    /// The value and the type, `""` when it was never said.
    Int(i128, &'static str),
    Float(f64),
    Char(char),
    Str(String),
    Bytes(Vec<u8>),
    Tuple(Vec<Val>),
    List(Vec<Val>),
    Struct(String, Vec<(String, Val)>),
    Option(Option<Box<Val>>),
    Result(Result<Box<Val>, Box<Val>>),
    /// A nom error, the `Err` of a parser.
    Error(PErr),
    /// The input of a parser: a position in the payload.
    Input(usize),
    Parser(Arc<Parser>),
    Func(Func),
}

impl From<V> for Val {
    fn from(v: V) -> Self {
        Val { v, span: None }
    }
}

impl Val {
    pub fn unit() -> Val {
        V::Unit.into()
    }

    pub fn with_span(mut self, span: Span) -> Val {
        self.span = Some(span);
        self
    }

    /// The name of the kind of value, for the error messages.
    pub fn kind(&self) -> &'static str {
        match &self.v {
            V::Unit => "()",
            V::Bool(_) => "a bool",
            V::Int(..) => "an integer",
            V::Float(_) => "a float",
            V::Char(_) => "a char",
            V::Str(_) => "a string",
            V::Bytes(_) => "bytes",
            V::Tuple(_) => "a tuple",
            V::List(_) => "a Vec",
            V::Struct(..) => "a struct",
            V::Option(_) => "an Option",
            V::Result(_) => "a Result",
            V::Error(_) => "a nom error",
            V::Input(_) => "an input",
            V::Parser(_) => "a parser",
            V::Func(_) => "a function",
        }
    }

    pub fn as_int(&self) -> Option<i128> {
        match &self.v {
            V::Int(v, _) => Some(*v),
            V::Char(c) => Some(*c as i128),
            V::Bool(b) => Some(*b as i128),
            _ => None,
        }
    }

    /// The text of strings, bytes and chars.
    pub fn as_bytes(&self) -> Option<Vec<u8>> {
        match &self.v {
            V::Str(s) => Some(s.as_bytes().to_vec()),
            V::Bytes(b) => Some(b.clone()),
            V::Char(c) => Some(c.to_string().into_bytes()),
            V::Int(v, _) if (0..=255).contains(v) => Some(vec![*v as u8]),
            V::List(items) => items.iter().map(|i| i.as_int().and_then(|v| u8::try_from(v).ok())).collect(),
            _ => None,
        }
    }

    /// The same data, whatever the spans, the int types, `String` and `&str` or bytes.
    pub fn same(&self, other: &Val) -> bool {
        match (&self.v, &other.v) {
            (V::Unit, V::Unit) => true,
            (V::Bool(a), V::Bool(b)) => a == b,
            (V::Float(a), V::Float(b)) => (a - b).abs() < 1e-6,
            (V::Int(..) | V::Char(_), V::Int(..) | V::Char(_)) => self.as_int() == other.as_int(),
            (V::Str(_) | V::Bytes(_) | V::Char(_), V::Str(_) | V::Bytes(_) | V::Char(_)) => self.as_bytes() == other.as_bytes(),
            (V::Tuple(a), V::Tuple(b)) | (V::List(a), V::List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.same(y)),
            (V::List(_), V::Bytes(_)) | (V::Bytes(_), V::List(_)) => self.as_bytes().is_some() && self.as_bytes() == other.as_bytes(),
            (V::Struct(n1, f1), V::Struct(n2, f2)) => {
                n1 == n2 && f1.len() == f2.len() && f1.iter().all(|(k, v)| f2.iter().any(|(k2, v2)| k == k2 && v.same(v2)))
            }
            (V::Option(a), V::Option(b)) => match (a, b) {
                (Some(a), Some(b)) => a.same(b),
                (None, None) => true,
                _ => false,
            },
            (V::Result(Ok(a)), V::Result(Ok(b))) => a.same(b),
            _ => false,
        }
    }

    /// Rust's `{:?}`, cut after `max` characters.
    pub fn show(&self, max: usize) -> String {
        let mut s = String::new();
        self.write(&mut s, max);
        if s.chars().count() > max {
            s = s.chars().take(max.saturating_sub(1)).collect::<String>() + "…";
        }
        s
    }

    fn write(&self, out: &mut String, max: usize) {
        if out.len() > max * 4 {
            return;
        }
        let list = |out: &mut String, items: &[Val], open: &str, close: &str| {
            out.push_str(open);
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                item.write(out, max);
                if out.len() > max * 4 {
                    out.push_str(", …");
                    break;
                }
            }
            out.push_str(close);
        };
        match &self.v {
            V::Unit => out.push_str("()"),
            V::Bool(b) => out.push_str(&b.to_string()),
            V::Int(v, _) => out.push_str(&v.to_string()),
            V::Float(v) => out.push_str(&format!("{v:?}")),
            V::Char(c) => out.push_str(&format!("{c:?}")),
            V::Str(s) => out.push_str(&format!("{s:?}")),
            V::Bytes(b) => {
                out.push('[');
                for (i, byte) in b.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format!("0x{byte:02x}"));
                    if i > max / 4 {
                        out.push_str(", …");
                        break;
                    }
                }
                out.push(']');
            }
            V::Tuple(items) => list(out, items, "(", ")"),
            V::List(items) => list(out, items, "[", "]"),
            V::Struct(name, fields) => {
                out.push_str(name);
                out.push_str(" { ");
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(k);
                    out.push_str(": ");
                    v.write(out, max);
                }
                out.push_str(" }");
            }
            V::Option(None) => out.push_str("None"),
            V::Option(Some(v)) => {
                out.push_str("Some(");
                v.write(out, max);
                out.push(')');
            }
            V::Result(Ok(v)) => {
                out.push_str("Ok(");
                v.write(out, max);
                out.push(')');
            }
            V::Result(Err(v)) => {
                out.push_str("Err(");
                v.write(out, max);
                out.push(')');
            }
            V::Error(e) => out.push_str(&format!("Error({})", e.kind)),
            V::Input(pos) => out.push_str(&format!("input@{pos}")),
            V::Parser(_) => out.push_str("<parser>"),
            V::Func(_) => out.push_str("<fn>"),
        }
    }
}

/// A closure of the script, with the variables it sees.
#[derive(Debug)]
pub struct Closure {
    pub params: Vec<Pat>,
    pub body: Expr,
    pub env: Vec<(String, Val)>,
}

#[derive(Debug, Clone)]
pub enum Func {
    Closure(Arc<Closure>),
    /// A function of Rust or nom, by its name: `str::parse`, `is_digit`, `Some`.
    Named {
        name: String,
        generic: Option<String>,
    },
    /// A combinator that makes a parser from its arguments: `tag`, `count`, `preceded`.
    Combinator(&'static str),
}

/// The error of a parser: where, and the `ErrorKind` of nom.
#[derive(Debug, Clone, PartialEq)]
pub struct PErr {
    pub pos: usize,
    pub kind: String,
    /// The contexts given with `context(..)` and `dbg_peek_hex(..)`, innermost first.
    pub contexts: Vec<String>,
    /// `nom::Err::Failure` after a `cut`, the alternatives are not tried.
    pub failure: bool,
}

impl PErr {
    pub fn new(pos: usize, kind: impl Into<String>) -> PErr {
        PErr { pos, kind: kind.into(), contexts: vec![], failure: false }
    }
}
