//! The levels: a file, the struct to fill, the parser to write. Each has a solution, the tests
//! run them all.
use crate::script::{self, RunError, V, Val};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chapter {
    Text,
    Numbers,
    Records,
    Framing,
    Binary,
}

impl Chapter {
    pub fn title(self) -> &'static str {
        match self {
            Chapter::Text => "Text",
            Chapter::Numbers => "Numbers",
            Chapter::Records => "Records",
            Chapter::Framing => "Length prefixes",
            Chapter::Binary => "Binary",
        }
    }
}

pub struct Level {
    /// Stable, for the saves.
    pub key: &'static str,
    pub chapter: Chapter,
    pub title: &'static str,
    /// The name of the file to parse.
    pub file: &'static str,
    pub payload: &'static [u8],
    /// The struct the parser returns, in Rust.
    pub target: &'static str,
    pub brief: &'static str,
    /// The protocols that do the same, and the facts of nom.
    pub notes: &'static str,
    pub hints: &'static [&'static str],
    pub starter: &'static str,
    pub solution: &'static str,
}

impl Level {
    /// Binary files are shown as xxd, text as text.
    pub fn binary(&self) -> bool {
        self.chapter == Chapter::Binary
    }

    /// The value the solution returns.
    pub fn expected(&self) -> Option<Val> {
        script::run(self.solution, self.payload).result.ok().map(|(_, v)| v)
    }
}

/// How a run compares with the level.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub solved: bool,
    /// Per field of the struct: the name, the value expected, the value returned, if equal.
    pub fields: Vec<FieldCheck>,
    /// What is still wrong, in one line.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldCheck {
    pub name: String,
    pub expected: String,
    pub got: Option<String>,
    pub ok: bool,
}

pub fn check(level: &Level, expected: &Val, outcome: &script::Outcome) -> Check {
    let V::Struct(name, want) = &expected.v else {
        return Check { solved: false, fields: vec![], status: "the solution of this level does not return a struct".into() };
    };
    let returned = outcome.result.as_ref().ok();
    let got_fields = match returned.map(|(_, v)| &v.v) {
        Some(V::Struct(n, f)) if n == name => Some(f),
        _ => None,
    };
    let fields: Vec<FieldCheck> = want
        .iter()
        .map(|(k, v)| {
            let got = got_fields.and_then(|f| f.iter().find(|(gk, _)| gk == k)).map(|(_, gv)| gv);
            FieldCheck { name: k.clone(), expected: v.show(48), got: got.map(|g| g.show(48)), ok: got.is_some_and(|g| g.same(v)) }
        })
        .collect();
    let status = match &outcome.result {
        Err(RunError::Script(e)) => e.to_string(),
        Err(RunError::Parse { err, line }) => format!("line {line}: the parser failed with ErrorKind::{} at byte {}", err.kind, err.pos),
        Ok((_, v)) if got_fields.is_none() => match &v.v {
            V::Struct(n, _) => format!("the parser returns a {n}, the level wants a {name}"),
            _ => format!("the parser returns {}, the level wants a {name} {{ .. }}", v.show(40)),
        },
        Ok(_) if fields.iter().any(|f| !f.ok) => {
            let wrong: Vec<_> = fields.iter().filter(|f| !f.ok).map(|f| f.name.as_str()).collect();
            format!("not yet: {}", wrong.join(", "))
        }
        Ok((rest, _)) if *rest < level.payload.len() => {
            format!("{} bytes are left after the parser, parse them all", level.payload.len() - rest)
        }
        Ok(_) => "solved".into(),
    };
    let solved = status == "solved";
    Check { solved, fields, status }
}

pub fn get(key: &str) -> Option<(usize, &'static Level)> {
    LEVELS.iter().enumerate().find(|(_, l)| l.key == key)
}

pub static LEVELS: &[Level] = &[
    Level {
        key: "hello",
        chapter: Chapter::Text,
        title: "Hello, world",
        file: "hello.txt",
        payload: b"hello world",
        target: "struct Hello {\n    greeting: String,\n}",
        brief: "Match the whole file with `tag` and return it in `greeting`.",
        notes: "A nom parser is a function from its input to `IResult<I, O>`: `Ok((rest, output))` with the input left over, or `Err` with where it failed. `?` returns the error at once, so each `let` reads the next piece or stops the parser.\n\n`tag(\"...\")` matches exactly these characters.",
        hints: &["tag(\"hello world\") matches the 11 characters of the file."],
        starter: "use nom::{IResult, bytes::complete::tag};\n\nfn parse(input: &str) -> IResult<&str, Hello> {\n    // tag(\"...\") matches exactly these characters.\n    let (input, greeting) = tag(\"hello\")(input)?;\n    Ok((input, Hello { greeting: greeting.to_string() }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Hello> {\n    let (input, greeting) = tag(\"hello world\")(input)?;\n    Ok((input, Hello { greeting: greeting.to_string() }))\n}\n",
    },
    Level {
        key: "greeting",
        chapter: Chapter::Text,
        title: "Hello, whoever",
        file: "greeting.txt",
        payload: b"hello Ferris",
        target: "struct Greeting {\n    name: String,\n}",
        brief: "The greeting is always `hello `, the name changes from file to file: keep the name.",
        notes: "`tag` is for what never changes. What changes is a class of characters: `alpha1` takes one or more ASCII letters, `digit1` digits, `alphanumeric1` both, `space1` spaces and tabs.\n\nThe `0` and `1` variants say how many at least: `alpha0` also matches nothing at all.",
        hints: &["First `tag(\"hello \")`, its value is not needed: `let (input, _) = ...`.", "Then `alpha1(input)?` for the name."],
        starter: "use nom::{IResult, bytes::complete::tag, character::complete::alpha1};\n\nfn parse(input: &str) -> IResult<&str, Greeting> {\n    let (input, _) = tag(\"hello \")(input)?;\n    // The name: one or more letters.\n    Ok((input, Greeting { name: String::new() }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Greeting> {\n    let (input, _) = tag(\"hello \")(input)?;\n    let (input, name) = alpha1(input)?;\n    Ok((input, Greeting { name: name.to_string() }))\n}\n",
    },
    Level {
        key: "setting",
        chapter: Chapter::Text,
        title: "Key and value",
        file: "settings.txt",
        payload: b"language: Rust",
        target: "struct Setting {\n    key: String,\n    value: String,\n}",
        brief: "A key, a colon and a space, a value.",
        notes: "In nom 8 a tuple of parsers is a parser: `(alpha1, tag(\": \"), alpha1).parse(input)` runs them in turn and returns the tuple of their values, destructured right in the `let`.\n\nEach variable gets its color in the file above: the colors show what each part of the parser matched.",
        hints: &["let (input, (key, _, value)) = (alpha1, tag(\": \"), alpha1).parse(input)?;"],
        starter: "use nom::{IResult, Parser, bytes::complete::tag, character::complete::alpha1};\n\nfn parse(input: &str) -> IResult<&str, Setting> {\n    let (input, key) = alpha1(input)?;\n    Ok((input, Setting { key: key.to_string(), value: String::new() }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Setting> {\n    let (input, (key, _, value)) = (alpha1, tag(\": \"), alpha1).parse(input)?;\n    Ok((input, Setting { key: key.to_string(), value: value.to_string() }))\n}\n",
    },
    Level {
        key: "element",
        chapter: Chapter::Text,
        title: "Opening and closing tags",
        file: "title.html",
        payload: b"<title>Parsing with nom</title>",
        target: "struct Element {\n    name: String,\n    text: String,\n}",
        brief: "An element of HTML: the name of the tag, the text, and the closing tag of the same name.",
        notes: "`delimited(a, b, c)` keeps the value of `b` only: `delimited(char('<'), alpha1, char('>'))` is the name of a tag. `take_until(\"</\")` takes everything before the closing tag.\n\nThe tag of a parser can come from a variable: `tag(name)` only matches the name read before, a closing tag of another name fails.",
        hints: &[
            "let (input, name) = delimited(char('<'), alpha1, char('>'))(input)?;",
            "let (input, text) = take_until(\"</\")(input)?;",
            "let (input, _) = delimited(tag(\"</\"), tag(name), char('>'))(input)?;",
        ],
        starter: "use nom::{IResult, bytes::complete::{tag, take_until}, character::complete::{alpha1, char}, sequence::delimited};\n\nfn parse(input: &str) -> IResult<&str, Element> {\n    let (input, name) = delimited(char('<'), alpha1, char('>'))(input)?;\n    Ok((input, Element { name: name.to_string(), text: String::new() }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Element> {\n    let (input, name) = delimited(char('<'), alpha1, char('>'))(input)?;\n    let (input, text) = take_until(\"</\")(input)?;\n    let (input, _) = delimited(tag(\"</\"), tag(name), char('>'))(input)?;\n    Ok((input, Element { name: name.to_string(), text: text.to_string() }))\n}\n",
    },
    Level {
        key: "age",
        chapter: Chapter::Numbers,
        title: "A number",
        file: "age.txt",
        payload: b"age=42",
        target: "struct Age {\n    years: u8,\n}",
        brief: "Read the number as a `u8`, not as text.",
        notes: "`nom::character::complete::u8` reads a number written in digits and returns it as a `u8`: \"300\" fails, it does not fit. Not to confuse with `nom::number::complete::u8`, which reads one byte of a binary file.\n\n`preceded(a, b)` matches `a` then `b` and keeps the value of `b`.",
        hints: &["let (input, years) = preceded(tag(\"age=\"), u8)(input)?;"],
        starter: "use nom::{IResult, bytes::complete::tag, character::complete::u8, sequence::preceded};\n\nfn parse(input: &str) -> IResult<&str, Age> {\n    let (input, _) = tag(\"age=\")(input)?;\n    Ok((input, Age { years: 0 }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Age> {\n    let (input, years) = preceded(tag(\"age=\"), u8)(input)?;\n    Ok((input, Age { years }))\n}\n",
    },
    Level {
        key: "point",
        chapter: Chapter::Numbers,
        title: "Signed numbers",
        file: "point.txt",
        payload: b"x=10,y=-3",
        target: "struct Point {\n    x: i32,\n    y: i32,\n}",
        brief: "Two coordinates, one of them negative.",
        notes: "The signed parsers `i8` to `i64` take an optional `+` or `-` before the digits.\n\n`separated_pair(a, sep, b)` returns `(a, b)` and drops the separator.",
        hints: &["preceded(tag(\"x=\"), i32)", "separated_pair(preceded(tag(\"x=\"), i32), char(','), preceded(tag(\"y=\"), i32))"],
        starter: "use nom::{IResult, Parser, bytes::complete::tag, character::complete::{char, i32}, sequence::{preceded, separated_pair}};\n\nfn parse(input: &str) -> IResult<&str, Point> {\n    let (input, x) = preceded(tag(\"x=\"), i32)(input)?;\n    Ok((input, Point { x, y: 0 }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Point> {\n    let (input, (x, y)) = separated_pair(preceded(tag(\"x=\"), i32), char(','), preceded(tag(\"y=\"), i32))(input)?;\n    Ok((input, Point { x, y }))\n}\n",
    },
    Level {
        key: "reading",
        chapter: Chapter::Numbers,
        title: "Decimals",
        file: "reading.txt",
        payload: b"sensor 7: -3.5C",
        target: "struct Reading {\n    sensor: u16,\n    celsius: f64,\n}",
        brief: "The number of the sensor, then the temperature in degrees Celsius.",
        notes: "`double` reads a decimal number written in text: an optional sign, digits, a fraction, an exponent (`1.5e3`).\n\nThe `C` at the end is part of the file: the parser has to read everything.",
        hints: &[
            "preceded(tag(\"sensor \"), u16)",
            "let (input, celsius) = preceded(tag(\": \"), double)(input)?;",
            "let (input, _) = char('C')(input)?;",
        ],
        starter: "use nom::{IResult, bytes::complete::tag, character::complete::{char, u16}, number::complete::double, sequence::preceded};\n\nfn parse(input: &str) -> IResult<&str, Reading> {\n    let (input, sensor) = preceded(tag(\"sensor \"), u16)(input)?;\n    Ok((input, Reading { sensor, celsius: 0.0 }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Reading> {\n    let (input, sensor) = preceded(tag(\"sensor \"), u16)(input)?;\n    let (input, celsius) = preceded(tag(\": \"), double)(input)?;\n    let (input, _) = char('C')(input)?;\n    Ok((input, Reading { sensor, celsius }))\n}\n",
    },
    Level {
        key: "colors",
        chapter: Chapter::Records,
        title: "A list",
        file: "colors.csv",
        payload: b"red,green,blue",
        target: "struct Palette {\n    colors: Vec<String>,\n}",
        brief: "As many names as there are, separated by commas.",
        notes: "`separated_list1(sep, item)` reads one item or more with `sep` between them, into a `Vec`. The items of a `Vec` take the color of their variable, every other one in a lighter shade.",
        hints: &[
            "let (input, colors) = separated_list1(char(','), alpha1)(input)?;",
            "The values are `&str`: `colors.iter().map(|c| c.to_string()).collect()` gives the `Vec<String>`, here `colors` is fine as it is.",
        ],
        starter: "use nom::{IResult, character::complete::{alpha1, char}, multi::separated_list1};\n\nfn parse(input: &str) -> IResult<&str, Palette> {\n    let (input, first) = alpha1(input)?;\n    Ok((input, Palette { colors: vec![first] }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Palette> {\n    let (input, colors) = separated_list1(char(','), alpha1)(input)?;\n    Ok((input, Palette { colors }))\n}\n",
    },
    Level {
        key: "count",
        chapter: Chapter::Records,
        title: "So many records",
        file: "names.txt",
        payload: b"3\nada\nbob\neve\n",
        target: "struct Names {\n    names: Vec<String>,\n}",
        brief: "The first line says how many names follow, one per line.",
        notes: "`count(parser, n)` runs the parser exactly `n` times: the file says how many records there are, the parser reads that many and no more.\n\n`terminated(a, b)` keeps the value of `a`: `terminated(alpha1, newline)` is a name and its line break.",
        hints: &[
            "let (input, n) = terminated(u32, newline)(input)?;",
            "let (input, names) = count(terminated(alpha1, newline), n as usize)(input)?;",
        ],
        starter: "use nom::{IResult, character::complete::{alpha1, newline, u32}, multi::count, sequence::terminated};\n\nfn parse(input: &str) -> IResult<&str, Names> {\n    let (input, n) = terminated(u32, newline)(input)?;\n    Ok((input, Names { names: vec![] }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Names> {\n    let (input, n) = terminated(u32, newline)(input)?;\n    let (input, names) = count(terminated(alpha1, newline), n as usize)(input)?;\n    Ok((input, Names { names }))\n}\n",
    },
    Level {
        key: "people",
        chapter: Chapter::Records,
        title: "Records of structs",
        file: "people.txt",
        payload: b"2\nada 36\nbob 41\n",
        target: "struct People {\n    people: Vec<Person>,\n}\n\nstruct Person {\n    name: String,\n    age: u32,\n}",
        brief: "A count, then a person per line: a name, a space, an age.",
        notes: "A parser function is a parser like any other: write `fn person(input: &str) -> IResult<&str, Person>` and give it to `count(person, n)`. The function where parsing starts is `parse`.",
        hints: &[
            "fn person(input: &str) -> IResult<&str, Person> {\n    let (input, (name, _, age, _)) = (alpha1, space1, u32, newline).parse(input)?;\n    Ok((input, Person { name: name.to_string(), age }))\n}",
            "let (input, people) = count(person, n as usize)(input)?;",
        ],
        starter: "use nom::{IResult, Parser, character::complete::{alpha1, newline, space1, u32}, multi::count, sequence::terminated};\n\nfn person(input: &str) -> IResult<&str, Person> {\n    let (input, name) = alpha1(input)?;\n    Ok((input, Person { name: name.to_string(), age: 0 }))\n}\n\nfn parse(input: &str) -> IResult<&str, People> {\n    let (input, n) = terminated(u32, newline)(input)?;\n    Ok((input, People { people: vec![] }))\n}\n",
        solution: "fn person(input: &str) -> IResult<&str, Person> {\n    let (input, (name, _, age, _)) = (alpha1, space1, u32, newline).parse(input)?;\n    Ok((input, Person { name: name.to_string(), age }))\n}\n\nfn parse(input: &str) -> IResult<&str, People> {\n    let (input, n) = terminated(u32, newline)(input)?;\n    let (input, people) = count(person, n as usize)(input)?;\n    Ok((input, People { people }))\n}\n",
    },
    Level {
        key: "frame",
        chapter: Chapter::Framing,
        title: "The length first",
        file: "frame.txt",
        payload: b"5HELLO",
        target: "struct Frame {\n    payload: String,\n}",
        brief: "The digits say how many characters the payload has.",
        notes: "A message says its own length before its content, the reader then knows how much to read without looking for an end marker, and the content may hold any character. ZeroMQ frames work so: in ZMTP each frame is a flags byte, its size (one byte, or eight bytes in big endian for the long frames) and the body; `5HELLO` is the same idea in text.\n\n`take(n)` takes `n` characters, `n` read just before.",
        hints: &["let (input, len) = u32(input)?;", "let (input, payload) = take(len)(input)?;"],
        starter: "use nom::{IResult, bytes::complete::take, character::complete::u32};\n\nfn parse(input: &str) -> IResult<&str, Frame> {\n    let (input, len) = u32(input)?;\n    Ok((input, Frame { payload: String::new() }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Frame> {\n    let (input, len) = u32(input)?;\n    let (input, payload) = take(len)(input)?;\n    Ok((input, Frame { payload: payload.to_string() }))\n}\n",
    },
    Level {
        key: "frames",
        chapter: Chapter::Framing,
        title: "Frame after frame",
        file: "frames.txt",
        payload: b"5HELLO5WORLD3FOO",
        target: "struct Message {\n    frames: Vec<String>,\n}",
        brief: "Frames one after the other until the end of the file.",
        notes: "`length_data(len)` does both steps: `len` reads the length, then it takes that many. `many1(length_data(u32))` reads frames while there are some.\n\nWithout the lengths `HELLOWORLDFOO` could not be cut back into its frames.",
        hints: &["let (input, frames) = many1(length_data(u32))(input)?;"],
        starter: "use nom::{IResult, character::complete::u32, multi::{length_data, many1}};\n\nfn parse(input: &str) -> IResult<&str, Message> {\n    let (input, first) = length_data(u32)(input)?;\n    Ok((input, Message { frames: vec![first.to_string()] }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Message> {\n    let (input, frames) = many1(length_data(u32))(input)?;\n    Ok((input, Message { frames }))\n}\n",
    },
    Level {
        key: "netstring",
        chapter: Chapter::Framing,
        title: "Netstrings",
        file: "netstring.txt",
        payload: b"12:hello world!,",
        target: "struct Netstring {\n    text: String,\n}",
        brief: "A netstring: the length, a colon, the text, a comma.",
        notes: "Netstrings, by D. J. Bernstein, frame text this way: `12:hello world!,`. The strings of Bencode, the format of BitTorrent files, are the same without the comma: `4:spam`.\n\nThe parser of the length can be any parser: `terminated(u32, char(':'))`.",
        hints: &["length_data(terminated(u32, char(':')))", "terminated(length_data(terminated(u32, char(':'))), char(','))"],
        starter: "use nom::{IResult, character::complete::{char, u32}, multi::length_data, sequence::terminated};\n\nfn parse(input: &str) -> IResult<&str, Netstring> {\n    let (input, len) = terminated(u32, char(':'))(input)?;\n    Ok((input, Netstring { text: String::new() }))\n}\n",
        solution: "fn parse(input: &str) -> IResult<&str, Netstring> {\n    let (input, text) = terminated(length_data(terminated(u32, char(':'))), char(','))(input)?;\n    Ok((input, Netstring { text: text.to_string() }))\n}\n",
    },
    Level {
        key: "magic",
        chapter: Chapter::Binary,
        title: "Magic and endianness",
        file: "header.bin",
        payload: b"IpaM\x27\x00\x00\x00",
        target: "struct Header {\n    version: u32,\n}",
        brief: "Four magic bytes `IpaM`, then the version, a `u32` in little endian.",
        notes: "A binary file starts with magic bytes that say what it is. The MapInfo of a StarCraft II map starts with `IpaM`, which is `MapI` written as a little endian number. A PNG starts with `\\x89PNG\\r\\n\\x1a\\n`, made to catch the transfers that alter line endings or the eighth bit.\n\nLittle endian puts the low byte first: `27 00 00 00` is 39. Big endian, the network order, would be `00 00 00 27`. `le_u32` and `be_u32` read them.",
        hints: &["let (input, _) = tag(&b\"IpaM\"[..])(input)?;", "let (input, version) = le_u32(input)?;"],
        starter: "use nom::{IResult, bytes::complete::tag, number::complete::le_u32};\n\nfn parse(input: &[u8]) -> IResult<&[u8], Header> {\n    let (input, _) = tag(&b\"IpaM\"[..])(input)?;\n    Ok((input, Header { version: 0 }))\n}\n",
        solution: "fn parse(input: &[u8]) -> IResult<&[u8], Header> {\n    let (input, _) = tag(&b\"IpaM\"[..])(input)?;\n    let (input, version) = le_u32(input)?;\n    Ok((input, Header { version }))\n}\n",
    },
    Level {
        key: "pascal",
        chapter: Chapter::Binary,
        title: "A length byte",
        file: "name.bin",
        payload: b"\x06Ferris",
        target: "struct Name {\n    name: String,\n}",
        brief: "One byte of length, then that many bytes of text.",
        notes: "The strings of Pascal kept their length in the first byte, at most 255. Binary protocols do the same, the length in one, two or four bytes before the data.\n\nIn a `&[u8]` the text is bytes: `String::from_utf8(name.to_vec())` makes a `String`, and fails if they are not UTF-8.",
        hints: &[
            "let (input, len) = be_u8(input)?;",
            "let (input, name) = take(len)(input)?;",
            "String::from_utf8(name.to_vec()).unwrap()",
        ],
        starter: "use nom::{IResult, bytes::complete::take, number::complete::be_u8};\n\nfn parse(input: &[u8]) -> IResult<&[u8], Name> {\n    let (input, len) = be_u8(input)?;\n    Ok((input, Name { name: String::new() }))\n}\n",
        solution: "fn parse(input: &[u8]) -> IResult<&[u8], Name> {\n    let (input, len) = be_u8(input)?;\n    let (input, name) = take(len)(input)?;\n    Ok((input, Name { name: String::from_utf8(name.to_vec()).unwrap() }))\n}\n",
    },
    Level {
        key: "dns",
        chapter: Chapter::Binary,
        title: "A domain name",
        file: "qname.bin",
        payload: b"\x03www\x07example\x03com\x00",
        target: "struct DomainName {\n    labels: Vec<String>,\n}",
        brief: "Labels with a length byte each, until a length of zero.",
        notes: "The names in DNS messages are written so: `www.example.com` is `3www 7example 3com 0`, a length byte before each label and a zero at the end, the root.\n\n`many_till(item, end)` reads items until `end` matches, and returns both: `(Vec<item>, end)`.",
        hints: &[
            "many_till(length_data(be_u8), tag(&b\"\\0\"[..]))",
            "let (input, (labels, _)) = many_till(length_data(be_u8), tag(&b\"\\0\"[..]))(input)?;",
        ],
        starter: "use nom::{IResult, bytes::complete::tag, multi::{length_data, many_till}, number::complete::be_u8};\n\nfn parse(input: &[u8]) -> IResult<&[u8], DomainName> {\n    let (input, first) = length_data(be_u8)(input)?;\n    Ok((input, DomainName { labels: vec![] }))\n}\n",
        solution: "fn parse(input: &[u8]) -> IResult<&[u8], DomainName> {\n    let (input, (labels, _)) = many_till(length_data(be_u8), tag(&b\"\\0\"[..]))(input)?;\n    Ok((input, DomainName { labels }))\n}\n",
    },
    Level {
        key: "tlv",
        chapter: Chapter::Binary,
        title: "Type, length, value",
        file: "options.bin",
        payload: b"\x01\x00\x04ping\x02\x00\x02\x00\x2a\x07\x00\x00",
        target: "struct Options {\n    records: Vec<Record>,\n}\n\nstruct Record {\n    kind: u8,\n    value: Vec<u8>,\n}",
        brief: "Records until the end: a type byte, a length in two bytes big endian, the value.",
        notes: "Type, length, value: the reader skips the types it does not know, it has their length. The options of DHCP, the extensions of TLS, LLDP and ASN.1 BER are written so, and the fields of Protocol Buffers have a tag and, for the bytes, a length.\n\nThe last record has a length of zero, its value is empty.",
        hints: &[
            "fn record(input: &[u8]) -> IResult<&[u8], Record> {\n    let (input, kind) = be_u8(input)?;\n    let (input, value) = length_data(be_u16)(input)?;\n    Ok((input, Record { kind, value: value.to_vec() }))\n}",
            "let (input, records) = many0(record)(input)?;",
        ],
        starter: "use nom::{IResult, multi::{length_data, many0}, number::complete::{be_u16, be_u8}};\n\nfn record(input: &[u8]) -> IResult<&[u8], Record> {\n    let (input, kind) = be_u8(input)?;\n    Ok((input, Record { kind, value: vec![] }))\n}\n\nfn parse(input: &[u8]) -> IResult<&[u8], Options> {\n    let (input, first) = record(input)?;\n    Ok((input, Options { records: vec![first] }))\n}\n",
        solution: "fn record(input: &[u8]) -> IResult<&[u8], Record> {\n    let (input, kind) = be_u8(input)?;\n    let (input, value) = length_data(be_u16)(input)?;\n    Ok((input, Record { kind, value: value.to_vec() }))\n}\n\nfn parse(input: &[u8]) -> IResult<&[u8], Options> {\n    let (input, records) = many0(record)(input)?;\n    Ok((input, Options { records }))\n}\n",
    },
    Level {
        key: "udp",
        chapter: Chapter::Binary,
        title: "A UDP datagram",
        file: "datagram.bin",
        payload: b"\x30\x39\x00\x35\x00\x0d\x00\x00hello",
        target: "struct Datagram {\n    src_port: u16,\n    dst_port: u16,\n    payload: Vec<u8>,\n}",
        brief: "The header of UDP, four `u16` in big endian, then the payload. The length counts the 8 bytes of the header too.",
        notes: "The header of UDP has the source port, the destination port, the length of the datagram, header included, and a checksum, here 0 for none.\n\nTCP has no such length: its Data Offset is the length of the header in 32-bit words, and the length of the segment comes from the Total Length of the IP header. TCP is a stream of bytes without messages, so the protocols over it frame their messages themselves: HTTP with Content-Length, TLS records with a `u16` length, ZeroMQ with the size of each frame, PostgreSQL with an `i32` before each message.",
        hints: &[
            "let (input, (src_port, dst_port, length, _checksum)) = (be_u16, be_u16, be_u16, be_u16).parse(input)?;",
            "let (input, payload) = take(length - 8)(input)?;",
        ],
        starter: "use nom::{IResult, Parser, bytes::complete::take, number::complete::be_u16};\n\nfn parse(input: &[u8]) -> IResult<&[u8], Datagram> {\n    let (input, src_port) = be_u16(input)?;\n    Ok((input, Datagram { src_port, dst_port: 0, payload: vec![] }))\n}\n",
        solution: "fn parse(input: &[u8]) -> IResult<&[u8], Datagram> {\n    let (input, (src_port, dst_port, length, _checksum)) = (be_u16, be_u16, be_u16, be_u16).parse(input)?;\n    let (input, payload) = take(length - 8)(input)?;\n    Ok((input, Datagram { src_port, dst_port, payload: payload.to_vec() }))\n}\n",
    },
    Level {
        key: "zmtp",
        chapter: Chapter::Binary,
        title: "ZeroMQ frames",
        file: "message.zmtp",
        payload: b"\x01\x05HELLO\x00\x05WORLD",
        target: "struct Message {\n    frames: Vec<Frame>,\n}\n\nstruct Frame {\n    more: bool,\n    body: String,\n}",
        brief: "ZMTP frames: a flags byte, a size byte, the body. Bit 0 of the flags says more frames follow.",
        notes: "In ZMTP 3, the protocol of ZeroMQ, a frame is a flags byte, a size and the body. Flag bit 0 is MORE, another frame of the same message follows; bit 1 is LONG, the size is then eight bytes in big endian instead of one; bit 2 is COMMAND.\n\nThe bits of a byte are read with `&`: `flags & 1 == 1`.",
        hints: &[
            "fn frame(input: &[u8]) -> IResult<&[u8], Frame> {\n    let (input, flags) = be_u8(input)?;\n    let (input, body) = length_data(be_u8)(input)?;\n    Ok((input, Frame { more: flags & 1 == 1, body: String::from_utf8_lossy(body).to_string() }))\n}",
            "let (input, frames) = many1(frame)(input)?;",
        ],
        starter: "use nom::{IResult, multi::{length_data, many1}, number::complete::be_u8};\n\nfn frame(input: &[u8]) -> IResult<&[u8], Frame> {\n    let (input, flags) = be_u8(input)?;\n    Ok((input, Frame { more: false, body: String::new() }))\n}\n\nfn parse(input: &[u8]) -> IResult<&[u8], Message> {\n    let (input, first) = frame(input)?;\n    Ok((input, Message { frames: vec![first] }))\n}\n",
        solution: "fn frame(input: &[u8]) -> IResult<&[u8], Frame> {\n    let (input, flags) = be_u8(input)?;\n    let (input, body) = length_data(be_u8)(input)?;\n    Ok((input, Frame { more: flags & 1 == 1, body: String::from_utf8_lossy(body).to_string() }))\n}\n\nfn parse(input: &[u8]) -> IResult<&[u8], Message> {\n    let (input, frames) = many1(frame)(input)?;\n    Ok((input, Message { frames }))\n}\n",
    },
    Level {
        key: "mapinfo",
        chapter: Chapter::Binary,
        title: "A MapInfo",
        file: "MapInfo",
        payload: b"IpaM\x27\x00\x00\x00\x98\x00\x00\x00\x88\x00\x00\x00Dark\x00Zerus\x00",
        target: "struct MapInfo {\n    file_version: u32,\n    cell_width: u32,\n    cell_height: u32,\n    theme: String,\n    tile_set: String,\n}",
        brief: "A simplified MapInfo of StarCraft II: the magic, three `u32` in little endian, two strings ended by a zero byte.",
        notes: "s2protocol reads the MapInfo of the maps this way, with `dbg_peek_hex` around each parser and `dbg_bytes_and_tail!` after: try them here, they write to the trace.\n\nThe strings end with a zero byte, C style: `take_while(|b| b != 0)` then `tag(&b\"\\0\"[..])`, or `terminated(take_until(&b\"\\0\"[..]), tag(&b\"\\0\"[..]))`.",
        hints: &[
            "let (input, _) = tag(&b\"IpaM\"[..])(input)?;\nlet (input, (file_version, cell_width, cell_height)) = (le_u32, le_u32, le_u32).parse(input)?;",
            "fn c_string(input: &[u8]) -> IResult<&[u8], String> {\n    let (input, bytes) = terminated(take_while(|b| b != 0), tag(&b\"\\0\"[..]))(input)?;\n    Ok((input, String::from_utf8_lossy(bytes).to_string()))\n}",
        ],
        starter: "use nom::{IResult, Parser, bytes::complete::{tag, take_while}, number::complete::le_u32, sequence::terminated};\n\nfn parse(input: &[u8]) -> IResult<&[u8], MapInfo> {\n    let (input, _) = dbg_peek_hex(tag(&b\"IpaM\"[..]), \"read file magic, IpaM bytes\")(input)?;\n    let (input, file_version) = dbg_peek_hex(le_u32, \"file_version\")(input)?;\n    dbg_bytes_and_tail!(file_version);\n    Ok((input, MapInfo { file_version, cell_width: 0, cell_height: 0, theme: String::new(), tile_set: String::new() }))\n}\n",
        solution: "fn c_string(input: &[u8]) -> IResult<&[u8], String> {\n    let (input, bytes) = terminated(take_while(|b| b != 0), tag(&b\"\\0\"[..]))(input)?;\n    Ok((input, String::from_utf8_lossy(bytes).to_string()))\n}\n\nfn parse(input: &[u8]) -> IResult<&[u8], MapInfo> {\n    let (input, _) = tag(&b\"IpaM\"[..])(input)?;\n    let (input, (file_version, cell_width, cell_height)) = (le_u32, le_u32, le_u32).parse(input)?;\n    let (input, theme) = c_string(input)?;\n    let (input, tile_set) = c_string(input)?;\n    Ok((input, MapInfo { file_version, cell_width, cell_height, theme, tile_set }))\n}\n",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_solution_solves_its_level_and_no_starter_does() {
        for level in LEVELS {
            let expected = level
                .expected()
                .unwrap_or_else(|| panic!("{}: the solution fails: {:?}", level.key, script::run(level.solution, level.payload).result));
            let solved = check(level, &expected, &script::run(level.solution, level.payload));
            assert!(solved.solved, "{}: {}", level.key, solved.status);
            let starter = check(level, &expected, &script::run(level.starter, level.payload));
            assert!(!starter.solved, "{}: the starter solves it", level.key);
            // The starter runs: no error in the script itself.
            assert!(
                !matches!(script::run(level.starter, level.payload).result, Err(RunError::Script(_))),
                "{}: {:?}",
                level.key,
                script::run(level.starter, level.payload).result
            );
        }
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<_> = LEVELS.iter().map(|l| l.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), LEVELS.len());
    }

    #[test]
    fn the_hints_run() {
        // The one-line hints of a `let` are statements of the solution or close to them.
        let level = get("udp").unwrap().1;
        let expected = level.expected().unwrap();
        let src = format!(
            "fn parse(input: &[u8]) -> IResult<&[u8], Datagram> {{\n{}\n{}\nOk((input, Datagram {{ src_port, dst_port, payload: payload.to_vec() }}))\n}}",
            level.hints[0], level.hints[1]
        );
        assert!(check(level, &expected, &script::run(&src, level.payload)).solved);
    }
}
