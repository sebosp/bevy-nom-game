//! The tokens to the syntax tree: a recursive descent with the precedence of Rust.
use super::ScriptError;
use super::ast::*;
use super::lexer::{Tok, Token};

pub fn parse(tokens: &[Token]) -> Result<Program, ScriptError> {
    let mut p = P { toks: tokens, pos: 0 };
    let mut fns = Vec::new();
    let mut uses = Vec::new();
    while !p.done() {
        if p.is_kw("use") || (p.is_kw("pub") && matches!(p.peek_at(1), Some(Tok::Ident(u)) if u == "use")) {
            p.eat_kw("pub");
            p.pos += 1;
            let text = p.skip_until(&[";"])?;
            p.expect(";", "at the end of the `use`")?;
            uses.extend(use_paths(&text));
            continue;
        }
        if let Some(f) = p.item()? {
            fns.push(f);
        }
    }
    Ok(Program { fns, uses })
}

/// `nom::{bytes::complete::tag, number::complete::{be_u16, u8}}` to its paths.
fn use_paths(text: &str) -> Vec<Vec<String>> {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let Some(open) = text.find('{') else {
        return vec![text.split("::").map(str::to_string).collect()];
    };
    let prefix = &text[..open];
    let inner = text[open + 1..].strip_suffix('}').unwrap_or(&text[open + 1..]);
    // The items at depth 0 of the braces.
    let mut items = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&inner[start..]);
    items.into_iter().filter(|i| !i.is_empty() && *i != "self" && *i != "*").flat_map(|i| use_paths(&format!("{prefix}{i}"))).collect()
}

struct P<'t> {
    toks: &'t [Token],
    pos: usize,
}

type R<T> = Result<T, ScriptError>;

impl P<'_> {
    fn done(&self) -> bool {
        self.pos >= self.toks.len()
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }

    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.pos + n).map(|t| &t.tok)
    }

    fn line(&self) -> usize {
        self.toks.get(self.pos).or(self.toks.last()).map(|t| t.line).unwrap_or(1)
    }

    fn err<T>(&self, message: impl Into<String>) -> R<T> {
        let (line, col) = match self.toks.get(self.pos) {
            Some(t) => (t.line, t.col),
            None => self.toks.last().map(|t| (t.line, t.col + 1)).unwrap_or((1, 1)),
        };
        Err(ScriptError { line, col, message: message.into() })
    }

    fn found(&self) -> String {
        match self.peek() {
            Some(t) => format!("found {t}"),
            None => "the script ends".to_string(),
        }
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s == kw)
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, p: &str, what: &str) -> R<()> {
        if self.eat(p) { Ok(()) } else { self.err(format!("expected `{p}` {what}, {}", self.found())) }
    }

    fn ident(&mut self, what: &str) -> R<String> {
        match self.peek() {
            Some(Tok::Ident(s)) => {
                let s = s.clone();
                self.pos += 1;
                Ok(s)
            }
            _ => self.err(format!("expected {what}, {}", self.found())),
        }
    }

    /// Skips balanced tokens up to one of `stops` at depth 0, returns them as text.
    fn skip_until(&mut self, stops: &[&str]) -> R<String> {
        let mut depth = 0i32;
        let mut text = String::new();
        while let Some(tok) = self.peek() {
            if depth == 0 && matches!(tok, Tok::Punct(p) if stops.contains(p)) {
                return Ok(text);
            }
            match tok {
                Tok::Punct("(" | "[" | "{" | "<") => depth += 1,
                Tok::Punct(")" | "]" | "}" | ">") => {
                    depth -= 1;
                    if depth < 0 {
                        return Ok(text);
                    }
                }
                _ => {}
            }
            let piece = match tok {
                Tok::Ident(s) => s.clone(),
                Tok::Punct(p) => p.to_string(),
                Tok::Lifetime(l) => format!("'{l} "),
                Tok::Int(v, s) => format!("{v}{}", s.clone().unwrap_or_default()),
                other => other.to_string(),
            };
            if text.chars().last().is_some_and(|c| c.is_alphanumeric()) && piece.starts_with(|c: char| c.is_alphanumeric()) {
                text.push(' ');
            }
            text.push_str(&piece);
            self.pos += 1;
        }
        Ok(text)
    }

    /// Skips a `{ ... }`.
    fn skip_braces(&mut self) -> R<()> {
        self.expect("{", "")?;
        self.skip_until(&["}"])?;
        self.expect("}", "")
    }

    /// A top level item: a function, or a `use`, a `struct`, an attribute, skipped.
    fn item(&mut self) -> R<Option<FnDef>> {
        if self.eat("#") {
            self.eat("!");
            self.expect("[", "after `#`")?;
            self.skip_until(&["]"])?;
            self.expect("]", "to close the attribute")?;
            return Ok(None);
        }
        self.eat_kw("pub");
        if self.eat("(") {
            self.skip_until(&[")"])?;
            self.expect(")", "")?;
        }
        if self.eat_kw("type") || self.eat_kw("const") || self.eat_kw("static") {
            self.skip_until(&[";"])?;
            self.expect(";", "at the end of the item")?;
            return Ok(None);
        }
        if self.eat_kw("struct") || self.eat_kw("enum") || self.eat_kw("impl") || self.eat_kw("mod") || self.eat_kw("trait") {
            self.skip_until(&["{", ";", "("])?;
            if self.is("(") {
                self.skip_until(&[";"])?;
                self.expect(";", "after the tuple struct")?;
            } else if !self.eat(";") {
                self.skip_braces()?;
            }
            return Ok(None);
        }
        if self.eat_kw("fn") {
            return self.fn_def().map(Some);
        }
        self.err(format!("expected a `fn`, {}", self.found()))
    }

    fn fn_def(&mut self) -> R<FnDef> {
        let line = self.line();
        let name = self.ident("the name of the function")?;
        if self.eat("<") {
            self.skip_until(&[">"])?;
            self.expect(">", "to close the generics")?;
        }
        self.expect("(", "before the input argument")?;
        self.eat_kw("mut");
        let param = self.ident("the input argument, like `input: &str`")?;
        self.expect(":", "after the name of the input argument")?;
        let param_type = self.skip_until(&[",", ")"])?;
        self.eat(",");
        if !self.is(")") {
            return self.err("a parser function takes one argument, its input");
        }
        self.expect(")", "after the input argument")?;
        if self.eat("->") {
            self.skip_until(&["{"])?;
        }
        if self.eat_kw("where") {
            self.skip_until(&["{"])?;
        }
        let (body, tail) = self.block()?;
        Ok(FnDef { name, param, param_type, body, tail: tail.map(|t| *t), line })
    }

    /// `{ stmt* tail? }`.
    fn block(&mut self) -> R<(Vec<Stmt>, Option<Box<Expr>>)> {
        self.expect("{", "to start the block")?;
        let mut stmts = Vec::new();
        loop {
            if self.eat("}") {
                return Ok((stmts, None));
            }
            if self.done() {
                return self.err("expected `}` to close the block, the script ends");
            }
            let line = self.line();
            if self.eat(";") {
                continue;
            }
            if self.eat_kw("let") {
                self.eat_kw("mut");
                let pat = self.pattern()?;
                if self.eat(":") {
                    self.skip_until(&["="])?;
                }
                self.expect("=", "after the pattern of the `let`")?;
                let expr = self.expr()?;
                if !self.eat(";") {
                    return self.err(format!("expected `;` after the `let`, {}", self.found()));
                }
                stmts.push(Stmt::Let { pat, expr, line });
                continue;
            }
            let returned = self.eat_kw("return");
            let expr = self.expr()?;
            if returned {
                self.eat(";");
                self.skip_until(&["}"])?;
                self.expect("}", "to close the block")?;
                return Ok((stmts, Some(Box::new(expr))));
            }
            if self.eat(";") {
                stmts.push(Stmt::Expr { expr, line });
            } else if self.eat("}") {
                return Ok((stmts, Some(Box::new(expr))));
            } else if matches!(expr, Expr::If { .. } | Expr::Block { .. }) {
                stmts.push(Stmt::Expr { expr, line });
            } else {
                return self.err(format!("expected `;` or `}}` after the expression, {}", self.found()));
            }
        }
    }

    fn pattern(&mut self) -> R<Pat> {
        if self.eat("_") {
            return Ok(Pat::Wild);
        }
        if self.eat("&") {
            return self.pattern();
        }
        if self.eat("(") {
            let mut pats = Vec::new();
            while !self.eat(")") {
                pats.push(self.pattern()?);
                if !self.eat(",") {
                    self.expect(")", "to close the pattern")?;
                    break;
                }
            }
            return Ok(Pat::Tuple(pats));
        }
        self.eat_kw("mut");
        let name = self.ident("a name, `_` or a tuple `(a, b)`")?;
        Ok(Pat::Ident(name))
    }

    fn expr(&mut self) -> R<Expr> {
        self.expr_with(true)
    }

    /// `structs`: whether `Name { .. }` is a struct literal, not in the condition of an `if`.
    fn expr_with(&mut self, structs: bool) -> R<Expr> {
        if self.is("|") || self.is("||") {
            return self.closure();
        }
        self.eat_kw("move");
        if self.is("|") || self.is("||") {
            return self.closure();
        }
        self.binary(0, structs)
    }

    fn closure(&mut self) -> R<Expr> {
        let mut params = Vec::new();
        if !self.eat("||") {
            self.expect("|", "")?;
            while !self.eat("|") {
                params.push(self.pattern()?);
                if self.eat(":") {
                    self.skip_until(&[",", "|"])?;
                }
                if !self.eat(",") {
                    self.expect("|", "after the arguments of the closure")?;
                    break;
                }
            }
        }
        if self.eat("->") {
            self.skip_until(&["{"])?;
        }
        let body = self.expr()?;
        Ok(Expr::Closure { params, body: Box::new(body) })
    }

    /// The operator ahead and its precedence, higher binds tighter.
    fn bin_op(&self) -> Option<(BinOp, u8, usize)> {
        let Some(Tok::Punct(p)) = self.peek() else {
            return None;
        };
        // `<<` and `>>` are two tokens side by side: `>>` closes two generics too.
        let adjacent = |q: &str| {
            matches!(self.peek_at(1), Some(Tok::Punct(r)) if *r == q) && self.toks[self.pos + 1].offset == self.toks[self.pos].offset + 1
        };
        Some(match *p {
            "||" => (BinOp::Or, 1, 1),
            "&&" => (BinOp::And, 2, 1),
            "==" => (BinOp::Eq, 3, 1),
            "!=" => (BinOp::Ne, 3, 1),
            "<" if adjacent("<") => (BinOp::Shl, 7, 2),
            ">" if adjacent(">") => (BinOp::Shr, 7, 2),
            "<" => (BinOp::Lt, 3, 1),
            "<=" => (BinOp::Le, 3, 1),
            ">" => (BinOp::Gt, 3, 1),
            ">=" => (BinOp::Ge, 3, 1),
            "|" => (BinOp::BitOr, 4, 1),
            "^" => (BinOp::BitXor, 5, 1),
            "&" => (BinOp::BitAnd, 6, 1),
            "+" => (BinOp::Add, 8, 1),
            "-" => (BinOp::Sub, 8, 1),
            "*" => (BinOp::Mul, 9, 1),
            "/" => (BinOp::Div, 9, 1),
            "%" => (BinOp::Rem, 9, 1),
            _ => return None,
        })
    }

    fn binary(&mut self, min: u8, structs: bool) -> R<Expr> {
        let mut left = self.range_or_cast(structs)?;
        while let Some((op, prec, width)) = self.bin_op() {
            if prec < min {
                break;
            }
            self.pos += width;
            let right = self.binary(prec + 1, structs)?;
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn range_or_cast(&mut self, structs: bool) -> R<Expr> {
        let mut e = self.unary(structs)?;
        while self.eat_kw("as") {
            let ty = self.skip_until(&[
                ")", ",", ";", "}", "]", "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "&", "|", "..",
            ])?;
            e = Expr::Cast { expr: Box::new(e), ty };
        }
        if self.is("..") || self.is("..=") {
            let inclusive = self.is("..=");
            self.pos += 1;
            // `a..` up to the end, in an index: the end is -1.
            let end = if self.is("]") { Expr::Int(-1) } else { self.unary(structs)? };
            e = Expr::Range { start: Box::new(e), end: Box::new(end), inclusive };
        }
        Ok(e)
    }

    fn unary(&mut self, structs: bool) -> R<Expr> {
        if self.eat("-") {
            let e = self.unary(structs)?;
            return Ok(match e {
                Expr::Int(v) => Expr::Int(-v),
                Expr::Float(v) => Expr::Float(-v),
                e => Expr::Unary { op: UnOp::Neg, expr: Box::new(e) },
            });
        }
        if self.eat("!") {
            return Ok(Expr::Unary { op: UnOp::Not, expr: Box::new(self.unary(structs)?) });
        }
        if self.eat("*") {
            return Ok(Expr::Unary { op: UnOp::Deref, expr: Box::new(self.unary(structs)?) });
        }
        if self.eat("&&") || self.eat("&") {
            self.eat_kw("mut");
            return Ok(Expr::Ref(Box::new(self.unary(structs)?)));
        }
        self.postfix(structs)
    }

    fn args(&mut self, close: &str) -> R<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.eat(close) {
            args.push(self.expr()?);
            if !self.eat(",") {
                if !self.eat(close) {
                    return self.err(format!("expected `,` or `{close}` in the list, {}", self.found()));
                }
                break;
            }
        }
        Ok(args)
    }

    /// `::<T>`, the type as text.
    fn turbofish(&mut self) -> R<Option<String>> {
        if self.is("::") && matches!(self.peek_at(1), Some(Tok::Punct("<"))) {
            self.pos += 2;
            let ty = self.skip_until(&[">"])?;
            self.expect(">", "to close the type")?;
            return Ok(Some(ty));
        }
        Ok(None)
    }

    fn postfix(&mut self, structs: bool) -> R<Expr> {
        let mut e = self.primary(structs)?;
        loop {
            if self.eat("?") {
                e = Expr::Try(Box::new(e));
            } else if self.is("[") {
                self.pos += 1;
                if self.eat("..") {
                    if self.eat("]") {
                        continue;
                    }
                    let end = self.expr()?;
                    self.expect("]", "to close the index")?;
                    e = Expr::Index {
                        recv: Box::new(e),
                        index: Box::new(Expr::Range { start: Box::new(Expr::Int(0)), end: Box::new(end), inclusive: false }),
                    };
                    continue;
                }
                let index = self.expr()?;
                self.expect("]", "to close the index")?;
                e = Expr::Index { recv: Box::new(e), index: Box::new(index) };
            } else if self.is("(") {
                self.pos += 1;
                let args = self.args(")")?;
                e = Expr::Call { func: Box::new(e), args };
            } else if self.eat(".") {
                match self.peek().cloned() {
                    Some(Tok::Int(v, _)) => {
                        self.pos += 1;
                        e = Expr::Field { recv: Box::new(e), name: v.to_string() };
                    }
                    Some(Tok::Float(v)) => {
                        // `t.0.1` lexes as `t` `.` `0.1`.
                        self.pos += 1;
                        let s = v.to_string();
                        let (a, b) = s.split_once('.').unwrap_or((&s, "0"));
                        e = Expr::Field { recv: Box::new(Expr::Field { recv: Box::new(e), name: a.into() }), name: b.into() };
                    }
                    Some(Tok::Ident(name)) => {
                        self.pos += 1;
                        let generic = self.turbofish()?;
                        if self.eat("(") {
                            let args = self.args(")")?;
                            e = Expr::Method { recv: Box::new(e), name, generic, args };
                        } else {
                            e = Expr::Field { recv: Box::new(e), name };
                        }
                    }
                    _ => return self.err(format!("expected a method or a field after `.`, {}", self.found())),
                }
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self, structs: bool) -> R<Expr> {
        let Some(tok) = self.peek().cloned() else {
            return self.err("expected an expression, the script ends");
        };
        self.pos += 1;
        Ok(match tok {
            Tok::Int(v, _) => Expr::Int(v),
            Tok::Float(v) => Expr::Float(v),
            Tok::Str(s) => Expr::Str(s),
            Tok::ByteStr(b) => Expr::ByteStr(b),
            Tok::Char(c) => Expr::Char(c),
            Tok::Byte(b) => Expr::Byte(b),
            Tok::Punct("(") => {
                let mut items = self.args(")")?;
                // `(a)` is `a`, `(a,)` a tuple.
                let trailing = matches!(self.toks.get(self.pos.wrapping_sub(2)).map(|t| &t.tok), Some(Tok::Punct(",")));
                if items.len() == 1 && !trailing { items.remove(0) } else { Expr::Tuple(items) }
            }
            Tok::Punct("[") => {
                if self.eat("]") {
                    return Ok(Expr::Array(vec![]));
                }
                let first = self.expr()?;
                if self.eat(";") {
                    let count = self.expr()?;
                    self.expect("]", "to close the array")?;
                    Expr::Repeat { value: Box::new(first), count: Box::new(count) }
                } else if self.eat(",") {
                    let mut items = vec![first];
                    items.extend(self.args("]")?);
                    Expr::Array(items)
                } else {
                    self.expect("]", "to close the array")?;
                    Expr::Array(vec![first])
                }
            }
            Tok::Punct("{") => {
                self.pos -= 1;
                let (stmts, tail) = self.block()?;
                Expr::Block { stmts, tail }
            }
            Tok::Punct("<") => {
                // `<u32>::from_str_radix`, `<&str>::parse`: the type as a path segment.
                let ty = self.skip_until(&[">"])?;
                self.expect(">", "to close the type")?;
                let mut segments = vec![ty];
                while self.eat("::") {
                    segments.push(self.ident("a name after `::`")?);
                }
                let generic = self.turbofish()?;
                Expr::Path { segments, generic }
            }
            Tok::Ident(kw) if kw == "true" => Expr::Bool(true),
            Tok::Ident(kw) if kw == "false" => Expr::Bool(false),
            Tok::Ident(kw) if kw == "if" => {
                let cond = self.expr_with(false)?;
                let (stmts, tail) = self.block()?;
                let then = Expr::Block { stmts, tail };
                let otherwise = if self.eat_kw("else") {
                    if self.is_kw("if") {
                        Some(Box::new(self.primary(structs)?))
                    } else {
                        let (stmts, tail) = self.block()?;
                        Some(Box::new(Expr::Block { stmts, tail }))
                    }
                } else {
                    None
                };
                Expr::If { cond: Box::new(cond), then: Box::new(then), otherwise }
            }
            Tok::Ident(first) => {
                let mut segments = vec![first];
                let mut generic = None;
                loop {
                    if let Some(g) = self.turbofish()? {
                        generic = Some(g);
                        continue;
                    }
                    if self.eat("::") {
                        segments.push(self.ident("a name after `::`")?);
                    } else {
                        break;
                    }
                }
                if self.eat("!") {
                    let name = segments.join("::");
                    let close = match self.peek() {
                        Some(Tok::Punct("(")) => ")",
                        Some(Tok::Punct("[")) => "]",
                        Some(Tok::Punct("{")) => "}",
                        _ => return self.err(format!("expected `(` after the macro `{name}!`, {}", self.found())),
                    };
                    self.pos += 1;
                    let args = self.args(close)?;
                    return Ok(Expr::Macro { name, args });
                }
                let last = segments.last().cloned().unwrap_or_default();
                if structs && self.is("{") && last.starts_with(|c: char| c.is_uppercase()) {
                    self.pos += 1;
                    let mut fields = Vec::new();
                    while !self.eat("}") {
                        if self.eat("..") {
                            self.expr()?;
                            continue;
                        }
                        let field = match self.peek().cloned() {
                            Some(Tok::Int(v, _)) => {
                                self.pos += 1;
                                v.to_string()
                            }
                            _ => self.ident("the name of a field")?,
                        };
                        let value = if self.eat(":") { self.expr()? } else { Expr::path(&field) };
                        fields.push((field, value));
                        if !self.eat(",") {
                            self.expect("}", "to close the struct")?;
                            break;
                        }
                    }
                    return Ok(Expr::Struct { name: last, fields });
                }
                Expr::Path { segments, generic }
            }
            other => {
                self.pos -= 1;
                return self.err(format!("expected an expression, found {other}"));
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::lexer::lex;
    use super::*;

    fn program(src: &str) -> Program {
        parse(&lex(src).unwrap()).unwrap()
    }

    #[test]
    fn parses_a_parser_function() {
        let p = program(
            r#"
            use nom::bytes::complete::tag;
            #[derive(Debug)]
            struct Hello { greeting: String }
            pub fn parse<'a>(input: &'a str) -> IResult<&'a str, Hello> {
                let (input, greeting) = tag("hello")(input)?;
                let (input, (a, _)) = (alpha1, space0).parse(input)?;
                Ok((input, Hello { greeting: greeting.to_string(), a }))
            }
            "#,
        );
        assert_eq!(p.fns.len(), 1);
        let f = &p.fns[0];
        assert_eq!(f.param, "input");
        assert!(f.is_text());
        assert_eq!(f.body.len(), 2);
        assert!(matches!(&f.body[1], Stmt::Let { pat: Pat::Tuple(p), .. } if p.len() == 2));
        assert!(matches!(&f.tail, Some(Expr::Call { .. })));
    }

    #[test]
    fn precedence_and_closures() {
        let p = program("fn f(i: &[u8]) -> R { take_while(|c| c >= 0x20 && c != b'|' || c == 1 << 3)(i) }");
        let Some(Expr::Call { func, .. }) = &p.fns[0].tail else { panic!() };
        let Expr::Call { args, .. } = func.as_ref() else { panic!() };
        let Expr::Closure { body, .. } = &args[0] else { panic!() };
        assert!(matches!(body.as_ref(), Expr::Binary { op: BinOp::Or, .. }));
    }

    #[test]
    fn errors_point_at_the_token() {
        let err = parse(&lex("fn f(input: &str) -> R {\n  let (input, x) = alpha1(input)?\n  Ok((input, x))\n}").unwrap()).unwrap_err();
        assert_eq!(err.line, 3);
        assert!(err.message.contains("expected `;`"), "{}", err.message);
    }
}
