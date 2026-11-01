//! The nom parsers the scripts can use, made from the combinators and their arguments.
use std::sync::Arc;

use super::value::{Func, V, Val};

#[derive(Debug)]
pub enum Parser {
    Tag(Vec<u8>),
    TagNoCase(Vec<u8>),
    Take(usize),
    TakeUntil {
        tag: Vec<u8>,
        one: bool,
    },
    /// `take_while`, `take_while1`, `take_while_m_n`, and `take_till` with `till`.
    TakeWhile {
        pred: Func,
        min: usize,
        max: usize,
        till: bool,
        kind: &'static str,
    },
    IsA(Vec<u8>),
    IsNot(Vec<u8>),
    /// A parser without arguments: `alpha1`, `be_u16`, `eof`.
    Builtin(&'static str),
    Char(char),
    OneOf(Vec<char>),
    NoneOf(Vec<char>),
    Satisfy(Func),
    Count(Arc<Parser>, usize),
    Many {
        p: Arc<Parser>,
        min: usize,
        max: usize,
        kind: &'static str,
    },
    SepList {
        sep: Arc<Parser>,
        p: Arc<Parser>,
        one: bool,
    },
    ManyTill(Arc<Parser>, Arc<Parser>),
    LengthData(Arc<Parser>),
    LengthValue(Arc<Parser>, Arc<Parser>),
    LengthCount(Arc<Parser>, Arc<Parser>),
    /// A tuple of parsers, `(a, b, c)` or `tuple((a, b, c))`.
    Seq(Vec<Arc<Parser>>),
    Preceded(Arc<Parser>, Arc<Parser>),
    Terminated(Arc<Parser>, Arc<Parser>),
    Delimited(Arc<Parser>, Arc<Parser>, Arc<Parser>),
    SeparatedPair(Arc<Parser>, Arc<Parser>, Arc<Parser>),
    Alt(Vec<Arc<Parser>>),
    Opt(Arc<Parser>),
    Recognize(Arc<Parser>),
    Consumed(Arc<Parser>),
    Value(Val, Arc<Parser>),
    Not(Arc<Parser>),
    Peek(Arc<Parser>),
    Cond(bool, Arc<Parser>),
    Verify(Arc<Parser>, Func),
    Map(Arc<Parser>, Func),
    MapRes(Arc<Parser>, Func),
    MapOpt(Arc<Parser>, Func),
    AllConsuming(Arc<Parser>),
    Complete(Arc<Parser>),
    Cut(Arc<Parser>),
    Success(Val),
    Fail,
    Context(String, Arc<Parser>),
    /// `dbg_peek_hex(parser, "context")` of s2protocol: the bytes where it failed, in the trace.
    DbgPeekHex(Arc<Parser>, String),
    /// A function of the script.
    UserFn(String),
    /// A closure `|i| ...` used as a parser.
    Closure(Func),
}

/// The parsers without arguments, the names of nom.
pub const BUILTINS: &[&str] = &[
    "alpha0",
    "alpha1",
    "digit0",
    "digit1",
    "alphanumeric0",
    "alphanumeric1",
    "space0",
    "space1",
    "multispace0",
    "multispace1",
    "hex_digit0",
    "hex_digit1",
    "oct_digit0",
    "oct_digit1",
    "newline",
    "line_ending",
    "crlf",
    "tab",
    "anychar",
    "not_line_ending",
    "eof",
    "rest",
    "rest_len",
    // The numbers written in text, nom::character::complete.
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "float",
    "double",
    // The numbers in binary, nom::number::complete.
    "be_u8",
    "be_u16",
    "be_u24",
    "be_u32",
    "be_u64",
    "be_u128",
    "be_i8",
    "be_i16",
    "be_i24",
    "be_i32",
    "be_i64",
    "be_i128",
    "be_f32",
    "be_f64",
    "le_u8",
    "le_u16",
    "le_u24",
    "le_u32",
    "le_u64",
    "le_u128",
    "le_i8",
    "le_i16",
    "le_i24",
    "le_i32",
    "le_i64",
    "le_i128",
    "le_f32",
    "le_f64",
];

/// The combinators: functions that take arguments and give a parser.
pub const COMBINATORS: &[&str] = &[
    "tag",
    "tag_no_case",
    "take",
    "take_while",
    "take_while1",
    "take_while_m_n",
    "take_till",
    "take_till1",
    "take_until",
    "take_until1",
    "is_a",
    "is_not",
    "char",
    "one_of",
    "none_of",
    "satisfy",
    "count",
    "many0",
    "many1",
    "many_m_n",
    "separated_list0",
    "separated_list1",
    "many_till",
    "length_data",
    "length_value",
    "length_count",
    "tuple",
    "preceded",
    "terminated",
    "delimited",
    "separated_pair",
    "pair",
    "alt",
    "opt",
    "recognize",
    "consumed",
    "value",
    "not",
    "peek",
    "cond",
    "verify",
    "map",
    "map_res",
    "map_opt",
    "all_consuming",
    "complete",
    "cut",
    "success",
    "fail",
    "context",
    "dbg_peek_hex",
    "dbg_dmp",
];

/// The `&'static str` of a builtin or a combinator name.
pub fn builtin(name: &str) -> Option<&'static str> {
    BUILTINS.iter().copied().find(|b| *b == name)
}

pub fn combinator(name: &str) -> Option<&'static str> {
    COMBINATORS.iter().copied().find(|b| *b == name)
}

/// What a combinator needs, for the error messages.
fn usage(name: &str) -> &'static str {
    match name {
        "tag" | "tag_no_case" => "tag(\"text\")",
        "take" => "take(4usize)",
        "take_while" | "take_while1" | "take_till" | "take_till1" => "take_while(|c| c != ',')",
        "take_while_m_n" => "take_while_m_n(1, 3, |c: char| c.is_ascii_digit())",
        "take_until" | "take_until1" => "take_until(\"end\")",
        "is_a" | "is_not" => "is_a(\"abc\")",
        "char" => "char(':')",
        "one_of" | "none_of" => "one_of(\"abc\")",
        "satisfy" => "satisfy(|c| c.is_ascii_digit())",
        "count" => "count(parser, n)",
        "many0" | "many1" | "opt" | "recognize" | "consumed" | "not" | "peek" | "all_consuming" | "complete" | "cut" => "many0(parser)",
        "many_m_n" => "many_m_n(1, 3, parser)",
        "separated_list0" | "separated_list1" => "separated_list1(tag(\",\"), parser)",
        "many_till" => "many_till(parser, end)",
        "length_data" => "length_data(be_u8)",
        "length_value" | "length_count" => "length_count(be_u8, parser)",
        "tuple" | "alt" => "alt((parser_a, parser_b))",
        "preceded" | "terminated" | "pair" => "preceded(tag(\"x=\"), digit1)",
        "delimited" | "separated_pair" => "delimited(char('('), digit1, char(')'))",
        "value" => "value(true, tag(\"yes\"))",
        "cond" => "cond(flag, parser)",
        "verify" | "map" | "map_res" | "map_opt" => "map_res(digit1, |s: &str| s.parse::<u32>())",
        "success" => "success(value)",
        "context" => "context(\"what\", parser)",
        "dbg_peek_hex" | "dbg_dmp" => "dbg_peek_hex(parser, \"what\")",
        _ => "",
    }
}

fn parser(v: &Val, name: &str) -> Result<Arc<Parser>, String> {
    match &v.v {
        V::Parser(p) => Ok(p.clone()),
        V::Str(s) => Ok(Arc::new(Parser::Tag(s.as_bytes().to_vec()))),
        V::Bytes(b) => Ok(Arc::new(Parser::Tag(b.clone()))),
        V::Char(c) => Ok(Arc::new(Parser::Char(*c))),
        V::Tuple(items) => Ok(Arc::new(Parser::Seq(items.iter().map(|i| parser(i, name)).collect::<Result<_, _>>()?))),
        V::Func(f @ Func::Closure(_)) => Ok(Arc::new(Parser::Closure(f.clone()))),
        _ => Err(format!("`{name}` expects a parser here, not {}: {}", v.kind(), usage(name))),
    }
}

fn bytes(v: &Val, name: &str) -> Result<Vec<u8>, String> {
    v.as_bytes().ok_or_else(|| format!("`{name}` expects a string or bytes, not {}: {}", v.kind(), usage(name)))
}

fn count(v: &Val, name: &str) -> Result<usize, String> {
    match v.as_int() {
        Some(n) if n >= 0 => usize::try_from(n).map_err(|_| format!("`{name}`: {n} is too large")),
        Some(n) => Err(format!("`{name}` expects a positive count, not {n}")),
        None => Err(format!("`{name}` expects a number, not {}: {}", v.kind(), usage(name))),
    }
}

fn func(v: &Val, name: &str) -> Result<Func, String> {
    match &v.v {
        V::Func(f) => Ok(f.clone()),
        _ => Err(format!("`{name}` expects a function or a closure, not {}: {}", v.kind(), usage(name))),
    }
}

fn chars(v: &Val, name: &str) -> Result<Vec<char>, String> {
    match &v.v {
        V::Str(s) => Ok(s.chars().collect()),
        V::Char(c) => Ok(vec![*c]),
        V::Bytes(b) => Ok(b.iter().map(|&b| b as char).collect()),
        _ => Err(format!("`{name}` expects a string, not {}: {}", v.kind(), usage(name))),
    }
}

/// The parsers of a tuple argument, `alt((a, b))`.
fn parsers(v: &Val, name: &str) -> Result<Vec<Arc<Parser>>, String> {
    match &v.v {
        V::Tuple(items) | V::List(items) => items.iter().map(|i| parser(i, name)).collect(),
        _ => Err(format!("`{name}` expects a tuple of parsers: {}", usage(name))),
    }
}

/// Makes the parser of a combinator.
pub fn build(name: &'static str, args: &[Val]) -> Result<Parser, String> {
    let arity = match name {
        "fail" => 0,
        "tag" | "tag_no_case" | "take" | "take_while" | "take_while1" | "take_till" | "take_till1" | "take_until" | "take_until1"
        | "is_a" | "is_not" | "char" | "one_of" | "none_of" | "satisfy" | "many0" | "many1" | "length_data" | "tuple" | "alt" | "opt"
        | "recognize" | "consumed" | "not" | "peek" | "all_consuming" | "complete" | "cut" | "success" => 1,
        "take_while_m_n" | "many_m_n" | "delimited" | "separated_pair" => 3,
        _ => 2,
    };
    if args.len() != arity {
        let usage = usage(name);
        return Err(format!("`{name}` takes {arity} argument{}, not {}: {usage}", if arity == 1 { "" } else { "s" }, args.len()));
    }
    let a = |i: usize| &args[i];
    Ok(match name {
        "tag" => Parser::Tag(bytes(a(0), name)?),
        "tag_no_case" => Parser::TagNoCase(bytes(a(0), name)?),
        "take" => Parser::Take(count(a(0), name)?),
        "take_while" => Parser::TakeWhile { pred: func(a(0), name)?, min: 0, max: usize::MAX, till: false, kind: "TakeWhile1" },
        "take_while1" => Parser::TakeWhile { pred: func(a(0), name)?, min: 1, max: usize::MAX, till: false, kind: "TakeWhile1" },
        "take_while_m_n" => Parser::TakeWhile {
            pred: func(a(2), name)?,
            min: count(a(0), name)?,
            max: count(a(1), name)?,
            till: false,
            kind: "TakeWhileMN",
        },
        "take_till" => Parser::TakeWhile { pred: func(a(0), name)?, min: 0, max: usize::MAX, till: true, kind: "TakeTill1" },
        "take_till1" => Parser::TakeWhile { pred: func(a(0), name)?, min: 1, max: usize::MAX, till: true, kind: "TakeTill1" },
        "take_until" => Parser::TakeUntil { tag: bytes(a(0), name)?, one: false },
        "take_until1" => Parser::TakeUntil { tag: bytes(a(0), name)?, one: true },
        "is_a" => Parser::IsA(bytes(a(0), name)?),
        "is_not" => Parser::IsNot(bytes(a(0), name)?),
        "char" => match &a(0).v {
            V::Char(c) => Parser::Char(*c),
            V::Int(b, _) if (0..=255).contains(b) => Parser::Char(*b as u8 as char),
            _ => return Err(format!("`char` expects a char, not {}: char(':')", a(0).kind())),
        },
        "one_of" => Parser::OneOf(chars(a(0), name)?),
        "none_of" => Parser::NoneOf(chars(a(0), name)?),
        "satisfy" => Parser::Satisfy(func(a(0), name)?),
        "count" => Parser::Count(parser(a(0), name)?, count(a(1), name)?),
        "many0" => Parser::Many { p: parser(a(0), name)?, min: 0, max: usize::MAX, kind: "Many0" },
        "many1" => Parser::Many { p: parser(a(0), name)?, min: 1, max: usize::MAX, kind: "Many1" },
        "many_m_n" => Parser::Many { p: parser(a(2), name)?, min: count(a(0), name)?, max: count(a(1), name)?, kind: "ManyMN" },
        "separated_list0" => Parser::SepList { sep: parser(a(0), name)?, p: parser(a(1), name)?, one: false },
        "separated_list1" => Parser::SepList { sep: parser(a(0), name)?, p: parser(a(1), name)?, one: true },
        "many_till" => Parser::ManyTill(parser(a(0), name)?, parser(a(1), name)?),
        "length_data" => Parser::LengthData(parser(a(0), name)?),
        "length_value" => Parser::LengthValue(parser(a(0), name)?, parser(a(1), name)?),
        "length_count" => Parser::LengthCount(parser(a(0), name)?, parser(a(1), name)?),
        "tuple" => Parser::Seq(parsers(a(0), name)?),
        "preceded" => Parser::Preceded(parser(a(0), name)?, parser(a(1), name)?),
        "terminated" => Parser::Terminated(parser(a(0), name)?, parser(a(1), name)?),
        "pair" => Parser::Seq(vec![parser(a(0), name)?, parser(a(1), name)?]),
        "delimited" => Parser::Delimited(parser(a(0), name)?, parser(a(1), name)?, parser(a(2), name)?),
        "separated_pair" => Parser::SeparatedPair(parser(a(0), name)?, parser(a(1), name)?, parser(a(2), name)?),
        "alt" => Parser::Alt(parsers(a(0), name)?),
        "opt" => Parser::Opt(parser(a(0), name)?),
        "recognize" => Parser::Recognize(parser(a(0), name)?),
        "consumed" => Parser::Consumed(parser(a(0), name)?),
        "value" => Parser::Value(a(0).clone(), parser(a(1), name)?),
        "not" => Parser::Not(parser(a(0), name)?),
        "peek" => Parser::Peek(parser(a(0), name)?),
        "cond" => match &a(0).v {
            V::Bool(b) => Parser::Cond(*b, parser(a(1), name)?),
            _ => return Err(format!("`cond` expects a bool first, not {}", a(0).kind())),
        },
        "verify" => Parser::Verify(parser(a(0), name)?, func(a(1), name)?),
        "map" => Parser::Map(parser(a(0), name)?, func(a(1), name)?),
        "map_res" => Parser::MapRes(parser(a(0), name)?, func(a(1), name)?),
        "map_opt" => Parser::MapOpt(parser(a(0), name)?, func(a(1), name)?),
        "all_consuming" => Parser::AllConsuming(parser(a(0), name)?),
        "complete" => Parser::Complete(parser(a(0), name)?),
        "cut" => Parser::Cut(parser(a(0), name)?),
        "success" => Parser::Success(a(0).clone()),
        "fail" => Parser::Fail,
        "context" => match &a(0).v {
            V::Str(s) => Parser::Context(s.clone(), parser(a(1), name)?),
            _ => return Err("`context` expects a string first: context(\"what\", parser)".into()),
        },
        "dbg_peek_hex" | "dbg_dmp" => match &a(1).v {
            V::Str(s) => Parser::DbgPeekHex(parser(a(0), name)?, s.clone()),
            _ => return Err(format!("`{name}` expects a string second: {name}(parser, \"what\")")),
        },
        _ => return Err(format!("`{name}` is not a combinator")),
    })
}
