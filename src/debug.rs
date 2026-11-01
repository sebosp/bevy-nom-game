//! The colored debugging of nom from s2protocol and nom-mpq: `peek_hex`, `dbg_bytes_and_tail!`,
//! `dbg_peek_hex`, as runs of styled text for the screen, or ANSI escapes for a terminal.
use crate::script::{Binding, Event, Outcome, RunError, Span};

/// The width of the name column, `DBG_CONTEXT_WIDTH` of s2protocol.
pub const DBG_CONTEXT_WIDTH: usize = 18;
/// The bytes `peek_hex` shows.
pub const PEEK_BYTES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    Dim,
    /// A printable byte, green.
    Printable,
    /// A null byte, dim on black.
    Null,
    /// Another byte, white.
    Other,
    /// The brackets of `peek_hex`, bright cyan.
    Bracket,
    /// The offsets, dim cyan.
    Offset,
    /// A length that is cut, bright yellow; a short one, dim yellow.
    Length,
    LengthDim,
    /// A value, magenta.
    Value,
    Error,
    Ok,
    /// The color of a binding.
    Binding(usize),
    /// The bytes of `_`.
    Discarded,
    /// Bytes not parsed yet, in the xxd view.
    Unparsed,
    UnparsedNull,
}

/// A run of text in one style.
#[derive(Debug, Clone, PartialEq)]
pub struct Seg {
    pub text: String,
    pub style: Style,
}

pub type Line = Vec<Seg>;

fn push(line: &mut Line, text: impl Into<String>, style: Style) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    match line.last_mut() {
        Some(last) if last.style == style => last.text.push_str(&text),
        _ => line.push(Seg { text, style }),
    }
}

/// The style of a byte in `peek_hex`.
fn byte_style(b: u8) -> Style {
    match b {
        0 => Style::Null,
        32..=126 => Style::Printable,
        _ => Style::Other,
    }
}

/// `peek_hex_color` of nom-mpq: `[6865 6c6c 6f00 0000 hello...]`, the first 8 bytes.
pub fn peek_hex(data: &[u8]) -> Line {
    let mut line = Line::new();
    let chunk = &data[..PEEK_BYTES.min(data.len())];
    push(&mut line, "[", Style::Bracket);
    let mut even_space = false;
    for &b in chunk {
        push(&mut line, format!("{b:02x}"), byte_style(b));
        if even_space {
            push(&mut line, " ", Style::Plain);
        }
        even_space = !even_space;
    }
    for _ in chunk.len()..PEEK_BYTES {
        push(&mut line, "  ", Style::Plain);
        if even_space {
            push(&mut line, " ", Style::Plain);
        }
        even_space = !even_space;
    }
    push(&mut line, " ", Style::Plain);
    for &b in chunk {
        let c = if (32..=126).contains(&b) { b as char } else { '.' };
        push(&mut line, c.to_string(), byte_style(b));
    }
    for _ in chunk.len()..PEEK_BYTES {
        push(&mut line, " ", Style::Plain);
    }
    push(&mut line, "]", Style::Bracket);
    line
}

/// The name in its style, padded with plain spaces to `DBG_CONTEXT_WIDTH`.
fn name_column(line: &mut Line, name: &str, style: Style) {
    let name: String = name.chars().take(DBG_CONTEXT_WIDTH).collect();
    let pad = DBG_CONTEXT_WIDTH - name.chars().count();
    push(line, name, style);
    push(line, " ".repeat(pad), Style::Plain);
}

/// `dbg_bytes_and_tail!`: `|name | 0x00000006[   003] [6164 61 ada] | next [...]`.
pub fn bytes_and_tail(name: &str, name_style: Style, input: &[u8], span: &Span) -> Line {
    let mut line = Line::new();
    push(&mut line, "|", Style::Dim);
    name_column(&mut line, name, name_style);
    push(&mut line, " | ", Style::Dim);
    push(&mut line, format!("0x{:08x}", span.start), Style::Offset);
    let len = span.end - span.start;
    push(&mut line, if len > PEEK_BYTES { "[..." } else { "[   " }, Style::Plain);
    push(&mut line, format!("{len:03}"), if len > PEEK_BYTES { Style::Length } else { Style::LengthDim });
    push(&mut line, "] ", Style::Plain);
    line.extend(peek_hex(&input[span.clone()]));
    push(&mut line, " | next ", Style::Dim);
    line.extend(peek_hex(&input[span.end.min(input.len())..]));
    line
}

/// `dbg_displayable_and_tail!`: `->name |value`.
pub fn displayable(name: &str, name_style: Style, value: &str) -> Line {
    let mut line = Line::new();
    push(&mut line, "->", Style::Dim);
    name_column(&mut line, name, name_style);
    push(&mut line, "|", Style::Dim);
    push(&mut line, value, Style::Value);
    line
}

/// `dbg_peek_hex` when its parser fails: `context: Error(Tag) at: [....]`.
pub fn peek_error(context: &str, kind: &str, input: &[u8], pos: usize) -> Line {
    let mut line = Line::new();
    push(&mut line, format!("{context}: "), Style::Error);
    push(&mut line, format!("Error({kind}) at: "), Style::Plain);
    push(&mut line, format!("0x{pos:08x} "), Style::Offset);
    line.extend(peek_hex(&input[pos.min(input.len())..]));
    line
}

fn binding_style(b: &Binding) -> Style {
    b.color.map(Style::Binding).unwrap_or(Style::Discarded)
}

/// The trace of a run: a line per binding (its bytes, then its value), the debug macros, the
/// errors.
pub fn trace(out: &Outcome, input: &[u8]) -> Vec<Line> {
    let mut lines = Vec::new();
    for e in &out.events {
        match e {
            Event::Bound(i) => {
                let b = &out.bindings[*i];
                lines.push(bytes_and_tail(&b.name, binding_style(b), input, &b.consumed));
                if b.color.is_some() {
                    lines.push(displayable(&b.name, binding_style(b), &b.value.show(90)));
                }
            }
            Event::BytesAndTail { name, span, .. } => lines.push(bytes_and_tail(name, Style::Plain, input, span)),
            Event::Displayable { name, value, .. } => lines.push(displayable(name, Style::Plain, value)),
            Event::Print { text, .. } => lines.push(vec![Seg { text: text.clone(), style: Style::Plain }]),
            Event::PeekError { context, pos, kind, .. } => lines.push(peek_error(context, kind, input, *pos)),
            Event::Failed { err, line } => {
                let mut l = Line::new();
                push(&mut l, format!("line {line}: "), Style::Error);
                push(&mut l, format!("Err(Error(Error {{ code: {} }})) at ", err.kind), Style::Plain);
                push(&mut l, format!("0x{:08x} ", err.pos), Style::Offset);
                l.extend(peek_hex(&input[err.pos.min(input.len())..]));
                lines.push(l);
            }
        }
    }
    match &out.result {
        Ok((rest, v)) => {
            let mut l = Line::new();
            push(&mut l, "Ok", Style::Ok);
            push(&mut l, format!(" {} ", v.show(120)), Style::Value);
            if *rest < input.len() {
                push(&mut l, format!("rest 0x{rest:08x} "), Style::Offset);
                l.extend(peek_hex(&input[*rest..]));
            } else {
                push(&mut l, "all the input parsed", Style::Dim);
            }
            lines.push(l);
        }
        Err(RunError::Script(e)) => lines.push(vec![Seg { text: e.to_string(), style: Style::Error }]),
        Err(RunError::Parse { .. }) => {}
    }
    lines
}

/// The 16 bytes per line of `xxd`, each byte in the color of the binding that parsed it.
pub const XXD_WIDTH: usize = 16;

/// The style of each byte of the input: a binding's color for the bytes of its value, the
/// discarded style for the rest it consumed, unparsed otherwise. The items of a `Vec` alternate
/// between the full color and the shade.
pub fn byte_styles(input: &[u8], bindings: &[Binding]) -> Vec<(Style, bool)> {
    let mut styles: Vec<(Style, bool)> =
        input.iter().map(|&b| (if b == 0 { Style::UnparsedNull } else { Style::Unparsed }, false)).collect();
    for b in bindings {
        let style = binding_style(b);
        for i in b.consumed.clone() {
            if let Some(s) = styles.get_mut(i) {
                *s = (if b.color.is_some() { style } else { Style::Discarded }, true);
            }
        }
        for i in b.span.clone() {
            if let Some(s) = styles.get_mut(i) {
                *s = (style, false);
            }
        }
        for (n, item) in b.items.iter().enumerate() {
            for i in item.clone() {
                if let Some(s) = styles.get_mut(i) {
                    *s = (style, n % 2 == 1);
                }
            }
        }
    }
    styles
}

/// A cell of the xxd view: the text, its style, and whether it is the shade of the style.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub text: String,
    pub style: Style,
    pub shade: bool,
}

/// The lines of the xxd view, at most `max_lines`.
pub fn xxd(input: &[u8], styles: &[(Style, bool)], max_lines: usize) -> Vec<Vec<Cell>> {
    let mut lines = Vec::new();
    for (n, chunk) in input.chunks(XXD_WIDTH).enumerate().take(max_lines) {
        let base = n * XXD_WIDTH;
        let mut cells: Vec<Cell> = Vec::new();
        let mut put = |text: String, style: Style, shade: bool| match cells.last_mut() {
            Some(c) if c.style == style && c.shade == shade => c.text.push_str(&text),
            _ => cells.push(Cell { text, style, shade }),
        };
        put(format!("{base:08x}: "), Style::Offset, false);
        for i in 0..XXD_WIDTH {
            match chunk.get(i) {
                Some(b) => {
                    let (style, shade) = styles[base + i];
                    put(format!("{b:02x}"), style, shade);
                }
                None => put("  ".into(), Style::Plain, false),
            }
            if i % 2 == 1 {
                // The space takes the style of the byte pair when both are of one binding.
                let same = i + 1 < chunk.len() && styles.get(base + i) == styles.get(base + i + 1);
                let (style, shade) = if same { styles[base + i] } else { (Style::Plain, false) };
                put(" ".into(), style, shade);
            }
        }
        put(" ".into(), Style::Plain, false);
        for (i, &b) in chunk.iter().enumerate() {
            let (style, shade) = styles[base + i];
            let c = if (32..=126).contains(&b) { b as char } else { '.' };
            put(c.to_string(), style, shade);
        }
        lines.push(cells);
    }
    lines
}

/// The text view: the characters in the colors of the bindings, a line break shown as `↵`.
pub fn text_view(input: &[u8], styles: &[(Style, bool)], max_bytes: usize) -> Vec<Cell> {
    let text = String::from_utf8_lossy(&input[..input.len().min(max_bytes)]).into_owned();
    let mut cells: Vec<Cell> = Vec::new();
    let mut pos = 0;
    for c in text.chars() {
        let (style, shade) = styles.get(pos).copied().unwrap_or((Style::Unparsed, false));
        let shown = match c {
            '\n' => "↵\n".to_string(),
            '\r' => "␍".to_string(),
            '\t' => "→   ".to_string(),
            c => c.to_string(),
        };
        match cells.last_mut() {
            Some(cell) if cell.style == style && cell.shade == shade => cell.text.push_str(&shown),
            _ => cells.push(Cell { text: shown, style, shade }),
        }
        pos += c.len_utf8();
    }
    cells
}

/// ANSI escapes for the terminal, the colors of `console` in s2protocol.
pub fn to_ansi(line: &Line) -> String {
    let mut out = String::new();
    for seg in line {
        let code = match seg.style {
            Style::Plain => "",
            Style::Dim | Style::Discarded | Style::Unparsed => "\x1b[2m",
            Style::Printable | Style::Ok => "\x1b[32m",
            Style::Null | Style::UnparsedNull => "\x1b[2;40m",
            Style::Other => "\x1b[37m",
            Style::Bracket => "\x1b[96m",
            Style::Offset => "\x1b[2;36m",
            Style::Length => "\x1b[93m",
            Style::LengthDim => "\x1b[2;33m",
            Style::Value => "\x1b[35m",
            Style::Error => "\x1b[31m",
            Style::Binding(i) => ["\x1b[91m", "\x1b[93m", "\x1b[92m", "\x1b[96m", "\x1b[94m", "\x1b[95m", "\x1b[33m", "\x1b[36m"][i % 8],
        };
        if code.is_empty() {
            out.push_str(&seg.text);
        } else {
            out.push_str(code);
            out.push_str(&seg.text);
            out.push_str("\x1b[0m");
        }
    }
    out
}

pub fn plain(line: &Line) -> String {
    line.iter().map(|s| s.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_hex_matches_nom_mpq() {
        assert_eq!(plain(&peek_hex(b"hello\0")), "[6865 6c6c 6f00       hello.  ]");
        assert_eq!(plain(&peek_hex(b"0123456789")), "[3031 3233 3435 3637  01234567]");
    }

    #[test]
    fn bytes_and_tail_line() {
        let line = bytes_and_tail("name", Style::Binding(0), b"hi ada!", &(3..6));
        assert_eq!(
            plain(&line),
            "|name               | 0x00000003[   003] [6164 61              ada     ] | next [21                   !       ]"
        );
    }

    #[test]
    fn xxd_colors_the_bindings() {
        let out = crate::script::run(
            "fn parse(input: &[u8]) -> IResult<&[u8], T> { let (input, n) = be_u8(input)?; let (input, s) = take(n)(input)?; Ok((input, s)) }",
            b"\x03abcz",
        );
        let styles = byte_styles(b"\x03abcz", &out.bindings);
        assert_eq!(styles[0], (Style::Binding(0), false));
        assert_eq!(styles[2], (Style::Binding(1), false));
        assert_eq!(styles[4], (Style::Unparsed, false));
        let lines = xxd(b"\x03abcz", &styles, 10);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].iter().map(|c| c.text.as_str()).collect::<String>().trim_end(),
            "00000000: 0361 6263 7a                             .abcz"
        );
    }
}
