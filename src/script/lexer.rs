//! The tokens of the player's script, cut with nom: the game parses its parsers with the same
//! combinators it teaches.
use nom::branch::alt;
use nom::bytes::complete::{tag, take_until, take_while, take_while1};
use nom::character::complete::{anychar, char, multispace1, none_of, one_of, satisfy};
use nom::combinator::{map, opt, recognize, value};
use nom::multi::{many0, many1};
use nom::sequence::{delimited, pair, preceded};
use nom::{IResult, Parser};

use super::ScriptError;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    /// The value and the suffix, `4usize` is `(4, Some("usize"))`.
    Int(i128, Option<String>),
    Float(f64),
    Str(String),
    ByteStr(Vec<u8>),
    Char(char),
    Byte(u8),
    Lifetime(String),
    Punct(&'static str),
}

impl std::fmt::Display for Tok {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tok::Ident(s) => write!(f, "`{s}`"),
            Tok::Int(v, _) => write!(f, "`{v}`"),
            Tok::Float(v) => write!(f, "`{v}`"),
            Tok::Str(s) => write!(f, "{s:?}"),
            Tok::ByteStr(b) => write!(f, "b{:?}", String::from_utf8_lossy(b)),
            Tok::Char(c) => write!(f, "{c:?}"),
            Tok::Byte(b) => write!(f, "b'{}'", *b as char),
            Tok::Lifetime(l) => write!(f, "'{l}"),
            Tok::Punct(p) => write!(f, "`{p}`"),
        }
    }
}

/// A token and where it starts in the script.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub offset: usize,
    pub line: usize,
    pub col: usize,
}

/// Longest first: `..=` before `..` before `.`.
const PUNCTS: [&str; 41] = [
    "..=", "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "..", "+=", "-=", "(", ")", "{", "}", "[", "]", "<", ">", ",", ";", ":",
    ".", "=", "!", "&", "|", "+", "-", "*", "/", "%", "?", "#", "^", "@", "$", "~", "_",
];

fn skip(input: &str) -> IResult<&str, ()> {
    value(
        (),
        many0(alt((
            value((), multispace1),
            value((), pair(tag("//"), take_while(|c| c != '\n'))),
            value((), delimited(tag("/*"), take_until("*/"), tag("*/"))),
        ))),
    )
    .parse(input)
}

fn ident(input: &str) -> IResult<&str, Tok> {
    map(recognize(pair(satisfy(|c| c.is_alphabetic() || c == '_'), take_while(|c: char| c.is_alphanumeric() || c == '_'))), |s: &str| {
        if s == "_" { Tok::Punct("_") } else { Tok::Ident(s.to_string()) }
    })
    .parse(input)
}

const INT_SUFFIXES: [&str; 12] = ["usize", "isize", "u128", "i128", "u64", "i64", "u32", "i32", "u16", "i16", "u8", "i8"];

fn suffix(input: &str) -> IResult<&str, Option<String>> {
    opt(map(alt(INT_SUFFIXES.map(tag)), str::to_string)).parse(input)
}

fn number(input: &str) -> IResult<&str, Tok> {
    let radix = |prefix: &'static str, radix: u32| {
        map(preceded(tag(prefix), recognize(many1(satisfy(move |c: char| c.is_digit(radix) || c == '_')))), move |digits: &str| {
            (digits.replace('_', ""), radix)
        })
    };
    let (rest, (digits, radix)) = alt((
        radix("0x", 16),
        radix("0b", 2),
        radix("0o", 8),
        map(recognize(pair(satisfy(|c| c.is_ascii_digit()), take_while(|c: char| c.is_ascii_digit() || c == '_'))), |d: &str| {
            (d.replace('_', ""), 10)
        }),
    ))
    .parse(input)?;
    // A float: `1.5`, not the range `1..2` nor the method `1.max(2)`.
    if radix == 10
        && let Ok((after, frac)) =
            preceded(char::<&str, nom::error::Error<&str>>('.'), take_while1(|c: char| c.is_ascii_digit())).parse(rest)
    {
        let value: f64 = format!("{digits}.{frac}").parse().unwrap_or_default();
        let (after, _) = opt(alt((tag("f32"), tag("f64")))).parse(after)?;
        return Ok((after, Tok::Float(value)));
    }
    let (rest, suffix) = suffix(rest)?;
    match i128::from_str_radix(&digits, radix) {
        Ok(v) => Ok((rest, Tok::Int(v, suffix))),
        Err(_) => Err(nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Digit))),
    }
}

/// The character after a backslash.
fn escape(input: &str) -> IResult<&str, u32> {
    preceded(
        char('\\'),
        alt((
            value('\n' as u32, char('n')),
            value('\r' as u32, char('r')),
            value('\t' as u32, char('t')),
            value(0, char('0')),
            value('\\' as u32, char('\\')),
            value('\'' as u32, char('\'')),
            value('"' as u32, char('"')),
            map(preceded(char('x'), recognize((one_of("0123456789abcdefABCDEF"), one_of("0123456789abcdefABCDEF")))), |h: &str| {
                u32::from_str_radix(h, 16).unwrap_or_default()
            }),
            map(delimited(tag("u{"), take_while1(|c: char| c.is_ascii_hexdigit()), char('}')), |h: &str| {
                u32::from_str_radix(h, 16).unwrap_or_default()
            }),
        )),
    )
    .parse(input)
}

/// A code point of a string or a character literal, escaped or not.
fn literal_char(quote: char) -> impl FnMut(&str) -> IResult<&str, u32> {
    move |input| alt((escape, map(none_of(if quote == '"' { "\"\\" } else { "'\\" }), |c| c as u32))).parse(input)
}

fn string(input: &str) -> IResult<&str, Tok> {
    map(delimited(char('"'), many0(literal_char('"')), char('"')), |chars| Tok::Str(chars.into_iter().filter_map(char::from_u32).collect()))
        .parse(input)
}

fn byte_string(input: &str) -> IResult<&str, Tok> {
    map(delimited(tag("b\""), many0(literal_char('"')), char('"')), |chars| Tok::ByteStr(chars.into_iter().map(|c| c as u8).collect()))
        .parse(input)
}

fn char_lit(input: &str) -> IResult<&str, Tok> {
    map(delimited(char('\''), literal_char('\''), char('\'')), |c| Tok::Char(char::from_u32(c).unwrap_or('\0'))).parse(input)
}

fn byte_lit(input: &str) -> IResult<&str, Tok> {
    map(delimited(tag("b'"), literal_char('\''), char('\'')), |c| Tok::Byte(c as u8)).parse(input)
}

fn lifetime(input: &str) -> IResult<&str, Tok> {
    map(
        preceded(char('\''), recognize(pair(satisfy(|c| c.is_alphabetic() || c == '_'), take_while(|c: char| c.is_alphanumeric())))),
        |l: &str| Tok::Lifetime(l.to_string()),
    )
    .parse(input)
}

fn punct(input: &str) -> IResult<&str, Tok> {
    for p in PUNCTS {
        if let Some(rest) = input.strip_prefix(p) {
            return Ok((rest, Tok::Punct(p)));
        }
    }
    Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag)))
}

fn token(input: &str) -> IResult<&str, Tok> {
    alt((byte_string, byte_lit, string, char_lit, lifetime, number, ident, punct)).parse(input)
}

/// The line and the column, from 1, of a byte offset.
pub fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let before = &src[..offset.min(src.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
    (line, col)
}

pub fn lex(src: &str) -> Result<Vec<Token>, ScriptError> {
    let mut tokens = Vec::new();
    let mut rest = src;
    loop {
        rest = skip(rest).map(|(r, _)| r).unwrap_or(rest);
        if rest.is_empty() {
            return Ok(tokens);
        }
        let offset = src.len() - rest.len();
        let (line, col) = line_col(src, offset);
        match token(rest) {
            Ok((after, tok)) => {
                tokens.push(Token { tok, offset, line, col });
                rest = after;
            }
            Err(_) => {
                let c = anychar::<&str, nom::error::Error<&str>>(rest).map(|(_, c)| c).unwrap_or(' ');
                let message = match c {
                    '"' => "this string is not closed".to_string(),
                    '\'' => "this character literal is not closed".to_string(),
                    _ => format!("unexpected character {c:?}"),
                };
                return Err(ScriptError { line, col, message });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        lex(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn lexes_a_let() {
        assert_eq!(
            toks("let (input, x) = tag(\"hi\\n\")(input)?; // done"),
            vec![
                Tok::Ident("let".into()),
                Tok::Punct("("),
                Tok::Ident("input".into()),
                Tok::Punct(","),
                Tok::Ident("x".into()),
                Tok::Punct(")"),
                Tok::Punct("="),
                Tok::Ident("tag".into()),
                Tok::Punct("("),
                Tok::Str("hi\n".into()),
                Tok::Punct(")"),
                Tok::Punct("("),
                Tok::Ident("input".into()),
                Tok::Punct(")"),
                Tok::Punct("?"),
                Tok::Punct(";"),
            ]
        );
    }

    #[test]
    fn lexes_literals() {
        assert_eq!(
            toks("4usize 0x20u8 b'\\0' b\"\\x89PNG\" 'a' &'a [u8] 1.5 1..=2 _"),
            vec![
                Tok::Int(4, Some("usize".into())),
                Tok::Int(32, Some("u8".into())),
                Tok::Byte(0),
                Tok::ByteStr(vec![0x89, b'P', b'N', b'G']),
                Tok::Char('a'),
                Tok::Punct("&"),
                Tok::Lifetime("a".into()),
                Tok::Punct("["),
                Tok::Ident("u8".into()),
                Tok::Punct("]"),
                Tok::Float(1.5),
                Tok::Int(1, None),
                Tok::Punct("..="),
                Tok::Int(2, None),
                Tok::Punct("_"),
            ]
        );
    }

    #[test]
    fn reports_where() {
        let err = lex("let x = 1;\n  \"open").unwrap_err();
        assert_eq!((err.line, err.col), (2, 3));
    }
}
