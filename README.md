# Nom Nom Bytes

A game about [nom](https://github.com/rust-bakery/nom) parser combinators, in Bevy, for Linux,
macOS, Windows and the browser: https://sebosp.github.io/bevy-nom-game/

Each level gives a file and a struct. Write the nom parser that reads one into the other: it runs
as you type, every `let` takes a color, and the bytes it matched in the file take the same color.
The file starts gray, the parser eats it in colors.

The levels go from `tag("hello world")` to classes of characters, numbers, lists and `count`,
length prefixes in text (`5HELLO`, netstrings), then binary: magic bytes and endianness, length
bytes, DNS names, type-length-value records, a UDP header, ZeroMQ frames, a MapInfo of StarCraft
II. The notes of each level tell which protocols use the same idea.

The sandbox opens any file (the button, or drop it on the window) for any parser.

## The parsers

The game reads a subset of Rust with the parsers and combinators of nom 8, and runs it itself, the
browser included:

```rust
use nom::{IResult, Parser, bytes::complete::take, number::complete::be_u8};

fn parse(input: &[u8]) -> IResult<&[u8], Name> {
    let (input, len) = be_u8(input)?;
    let (input, name) = take(len)(input)?;
    dbg_bytes_and_tail!(name);
    Ok((input, Name { name: String::from_utf8(name.to_vec()).unwrap() }))
}
```

- functions, `let` with tuple patterns, `?`, closures, `if`, struct literals, `as`, arithmetic
  with the overflow checks of Rust;
- `f(args)(input)` and `f(args).parse(input)`, tuples of parsers;
- the debugging of s2protocol and nom-mpq: `dbg_bytes_and_tail!`, `dbg_displayable_and_tail!`,
  `dbg_peek_hex(parser, "context")`, `context`, `println!`, in the trace with the colors of
  `peek_hex`;
- `&str` input for text, `&[u8]` for binary, from the type of the argument of `parse`.

The list of the combinators and parsers is in the sandbox.

## Building

```sh
cargo dev                 # bevy as a shared library: fast local builds
cargo build --release     # static, the binary to copy
trunk serve               # the browser version on http://localhost:8080
cargo test --release
```

The trace of a parser in the terminal, in the colors of s2protocol's `nom_color_debug`:

```sh
bevy-nom-game --trace parser.rs file.bin
```

Screenshots without a person: `NG_SCREENSHOT=out.png NG_SCREEN=level NG_LEVEL=mapinfo
NG_CODE=solution cargo dev`, see `src/main.rs`.

## Licenses

The code is under the Apache License 2.0. The fonts: DejaVu Sans Mono (Bitstream Vera license),
Fira Sans and Barlow Condensed (SIL Open Font License), see `assets/fonts`.
