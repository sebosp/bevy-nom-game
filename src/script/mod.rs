//! The player's parsers: a subset of Rust with the parsers and combinators of nom 8, read and run
//! by the game itself, on every platform the browser included.
pub mod ast;
pub mod interp;
pub mod lexer;
pub mod parser;
pub mod parsers;
pub mod value;

pub use interp::{Binding, Event, Outcome, RunError};
pub use value::{PErr, Span, V, Val};

/// An error in the script itself: it does not read, or does not run.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

pub fn compile(src: &str) -> Result<ast::Program, ScriptError> {
    parser::parse(&lexer::lex(src)?)
}

/// Reads and runs a script on an input.
pub fn run(src: &str, input: &[u8]) -> Outcome {
    match compile(src) {
        Ok(prog) => interp::run(&prog, input, None),
        Err(e) => Outcome { bindings: vec![], events: vec![], result: Err(RunError::Script(e)), text: true, entry: String::new() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The value returned, or the error as text.
    fn value(src: &str, input: &[u8]) -> Result<(usize, String), String> {
        match run(src, input).result {
            Ok((rest, v)) => Ok((rest, v.show(200))),
            Err(RunError::Script(e)) => Err(format!("script: {e}")),
            Err(RunError::Parse { err, line }) => Err(format!("parse line {line}: {} at {}", err.kind, err.pos)),
        }
    }

    fn text_fn(body: &str) -> String {
        format!("fn parse(input: &str) -> IResult<&str, T> {{\n{body}\n}}")
    }

    fn bin_fn(body: &str) -> String {
        format!("fn parse(input: &[u8]) -> IResult<&[u8], T> {{\n{body}\n}}")
    }

    #[test]
    fn hello_world() {
        let src = text_fn("let (input, greeting) = tag(\"hello world\")(input)?;\nOk((input, Hello { greeting: greeting.to_string() }))");
        assert_eq!(value(&src, b"hello world"), Ok((11, "Hello { greeting: \"hello world\" }".into())));
        let out = run(&src, b"hello world");
        assert_eq!(out.bindings.len(), 1);
        assert_eq!(out.bindings[0].name, "greeting");
        assert_eq!(out.bindings[0].consumed, 0..11);
        assert_eq!(out.bindings[0].color, Some(0));
    }

    #[test]
    fn tag_failure_says_where() {
        let src = text_fn("let (input, _) = tag(\"hello \")(input)?;\nlet (input, name) = digit1(input)?;\nOk((input, name))");
        assert_eq!(value(&src, b"hello Ada"), Err("parse line 3: Digit at 6".into()));
        let out = run(&src, b"hello Ada");
        assert!(out.events.iter().any(|e| matches!(e, Event::Failed { line: 3, .. })));
        // The `_` is recorded without a color.
        assert_eq!(out.bindings[0].color, None);
    }

    #[test]
    fn nom8_parse_method_and_tuples() {
        let src = text_fn("let (input, (key, _, value)) = (alpha1, char(':'), alpha1).parse(input)?;\nOk((input, Pair { key, value }))");
        assert_eq!(value(&src, b"name:Ada"), Ok((8, "Pair { key: \"name\", value: \"Ada\" }".into())));
        let out = run(&src, b"name:Ada");
        let spans: Vec<_> = out.bindings.iter().map(|b| (b.name.clone(), b.span.clone())).collect();
        assert_eq!(spans, vec![("key".to_string(), 0..4), ("_".to_string(), 4..5), ("value".to_string(), 5..8)]);
    }

    #[test]
    fn numbers_in_text() {
        let src = text_fn(
            "let (input, x) = preceded(tag(\"x=\"), i32)(input)?;\nlet (input, y) = preceded(tag(\",y=\"), i32)(input)?;\nOk((input, P { x, y }))",
        );
        assert_eq!(value(&src, b"x=10,y=-3"), Ok((9, "P { x: 10, y: -3 }".into())));
        let out = run(&src, b"x=10,y=-3");
        // The value is the number, the consumed bytes the tag too.
        assert_eq!(out.bindings[0].span, 2..4);
        assert_eq!(out.bindings[0].consumed, 0..4);
        assert_eq!(value(&text_fn("let (input, n) = u8(input)?;\nOk((input, n))"), b"300"), Err("parse line 2: Digit at 0".into()));
    }

    #[test]
    fn count_records_with_a_function() {
        let src = r#"
            fn person(input: &str) -> IResult<&str, Person> {
                let (input, name) = alpha1(input)?;
                let (input, _) = space1(input)?;
                let (input, age) = u32(input)?;
                let (input, _) = newline(input)?;
                Ok((input, Person { name: name.to_string(), age }))
            }
            fn parse(input: &str) -> IResult<&str, People> {
                let (input, n) = terminated(u32, newline)(input)?;
                let (input, people) = count(person, n as usize)(input)?;
                Ok((input, People { people }))
            }
        "#;
        let input = b"2\nada 36\nbob 41\n";
        assert_eq!(
            value(src, input),
            Ok((input.len(), "People { people: [Person { name: \"ada\", age: 36 }, Person { name: \"bob\", age: 41 }] }".into()))
        );
        let out = run(src, input);
        // Only the bindings of `parse`, the list with the bytes of each person.
        assert_eq!(out.bindings.len(), 2);
        assert_eq!(out.bindings[1].items, vec![2..9, 9..16]);
    }

    #[test]
    fn length_prefixed_text_and_binary() {
        let src = text_fn("let (input, len) = u32(input)?;\nlet (input, payload) = take(len)(input)?;\nOk((input, Frame { payload }))");
        assert_eq!(value(&src, b"5HELLO"), Ok((6, "Frame { payload: \"HELLO\" }".into())));
        let src = bin_fn("let (input, payload) = length_data(be_u16)(input)?;\nOk((input, payload))");
        assert_eq!(value(&src, b"\x00\x03abcd"), Ok((5, "[0x61, 0x62, 0x63]".into())));
        let src = bin_fn("let (input, v) = le_u32(input)?;\nlet (input, w) = be_i16(input)?;\nOk((input, (v, w)))");
        assert_eq!(value(&src, &[1, 0, 0, 0, 0xff, 0xfe]), Ok((6, "(1, -2)".into())));
    }

    #[test]
    fn use_lines_choose_the_number_parser() {
        let src = "use nom::number::complete::u8;\n".to_string() + &bin_fn("let (input, v) = u8(input)?;\nOk((input, v))");
        assert_eq!(value(&src, &[7]), Ok((1, "7".into())));
        let src = "use nom::{character::complete::u8, bytes::complete::tag};\n".to_string()
            + &bin_fn("let (input, v) = u8(input)?;\nOk((input, v))");
        assert_eq!(value(&src, b"42"), Ok((2, "42".into())));
    }

    #[test]
    fn closures_and_maps() {
        let src = text_fn(
            "let (input, word) = take_while1(|c: char| c != ' ' && c != ',')(input)?;\n\
             let (input, n) = map_res(preceded(char(','), digit1), |s: &str| s.parse::<u16>())(input)?;\n\
             let (input, up) = map(rest, |s: &str| s.to_uppercase())(input)?;\n\
             Ok((input, (word, n, up)))",
        );
        assert_eq!(value(&src, b"abc,42xyz"), Ok((9, "(\"abc\", 42, \"XYZ\")".into())));
        let src = bin_fn(
            "let (input, s) = take_while(|b| b != 0)(input)?;\nlet (input, _) = tag(b\"\\0\")(input)?;\nOk((input, String::from_utf8(s.to_vec()).unwrap()))",
        );
        assert_eq!(value(&src, b"Zerus\0"), Ok((6, "\"Zerus\"".into())));
    }

    #[test]
    fn many_and_lists() {
        let src = text_fn("let (input, items) = separated_list1(char(','), alpha1)(input)?;\nOk((input, items))");
        assert_eq!(value(&src, b"a,bc,d;"), Ok((6, "[\"a\", \"bc\", \"d\"]".into())));
        let src = text_fn("let (input, items) = many0(alpha0)(input)?;\nOk((input, items))");
        assert_eq!(value(&src, b"123"), Err("parse line 2: Many0 at 0".into()));
        let src =
            text_fn("let (input, (words, end)) = many_till(terminated(alpha1, space0), tag(\".\"))(input)?;\nOk((input, (words, end)))");
        assert_eq!(value(&src, b"a b ."), Ok((5, "([\"a\", \"b\"], \".\")".into())));
    }

    #[test]
    fn alt_opt_and_cut() {
        let src = text_fn(
            "let (input, v) = alt((tag(\"yes\"), tag(\"no\")))(input)?;\nlet (input, d) = opt(digit1)(input)?;\nOk((input, (v, d)))",
        );
        assert_eq!(value(&src, b"no"), Ok((2, "(\"no\", None)".into())));
        assert_eq!(value(&src, b"maybe"), Err("parse line 2: Tag at 0".into()));
        let src = text_fn("let (input, v) = alt((preceded(tag(\"a\"), cut(digit1)), alpha1))(input)?;\nOk((input, v))");
        assert_eq!(value(&src, b"ab"), Err("parse line 2: Digit at 1".into()));
    }

    #[test]
    fn length_value_limits_the_input() {
        let src = bin_fn(
            "let (input, inner) = length_value(be_u8, rest)(input)?;\nlet (input, tail) = rest(input)?;\nOk((input, (inner, tail)))",
        );
        assert_eq!(value(&src, b"\x02abc"), Ok((4, "([0x61, 0x62], [0x63])".into())));
    }

    #[test]
    fn overflow_is_reported_like_rust() {
        let src = bin_fn("let (input, len) = be_u16(input)?;\nlet (input, body) = take(len - 8)(input)?;\nOk((input, body))");
        let err = value(&src, &[0, 4]).unwrap_err();
        assert!(err.contains("subtract with overflow"), "{err}");
    }

    #[test]
    fn endless_loops_stop() {
        let src = "fn parse(input: &str) -> IResult<&str, T> { parse(input) }";
        let err = value(src, b"x").unwrap_err();
        assert!(err.contains("calls itself"), "{err}");
    }

    #[test]
    fn debug_macros_and_context_reach_the_trace() {
        let src = text_fn(
            "let (input, name) = dbg_peek_hex(alpha1, \"the name\")(input)?;\ndbg_bytes_and_tail!(name);\nlet (input, n) = context(\"the age\", digit1)(input)?;\nOk((input, (name, n)))",
        );
        let out = run(&src, b"ada x");
        assert!(out.events.iter().any(|e| matches!(e, Event::BytesAndTail { name, span, .. } if name == "name" && *span == (0..3))));
        assert!(out.events.iter().any(|e| matches!(e, Event::PeekError { context, pos: 3, .. } if context == "the age")));
    }

    #[test]
    fn unknown_names_are_explained() {
        let err = value(&text_fn("let (input, x) = alpah1(input)?;\nOk((input, x))"), b"a").unwrap_err();
        assert!(err.contains("`alpah1` is not defined"), "{err}");
        let err = value(&text_fn("let (input, x) = tag(\"a\", \"b\")(input)?;\nOk((input, x))"), b"a").unwrap_err();
        assert!(err.contains("takes 1 argument"), "{err}");
    }

    #[test]
    fn binary_payload_needs_bytes() {
        let err = value(&text_fn("Ok((input, ()))"), &[0xff, 0xfe]).unwrap_err();
        assert!(err.contains("not UTF-8"), "{err}");
    }
}
