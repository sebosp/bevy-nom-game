//! The screen of a level, and of the sandbox: the file, the parser, the trace. The parser runs
//! as it is typed, and the bytes of each variable take its color.
use bevy::ecs::schedule::SystemCondition;
use bevy::input_focus::{AutoFocus, InputFocus};
use bevy::picking::events::PointerClick;
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle, TextEdit};
use bevy::ui_widgets::TextInput;

use crate::debug;
use crate::files::{Incoming, OpenDialog};
use crate::levels::{self, Check, LEVELS, Level};
use crate::progress::Progress;
use crate::script::{self, Outcome, RunError, Val};
use crate::ui::*;

/// The level open, by its place in [`LEVELS`].
#[derive(Resource, Default)]
pub struct Current(pub usize);

/// The file of the sandbox.
#[derive(Resource)]
pub struct SandboxFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl Default for SandboxFile {
    fn default() -> Self {
        let sample = levels::get("mapinfo").map(|(_, l)| l.payload).unwrap_or_default();
        SandboxFile { name: "MapInfo (a sample, open your own file)".into(), bytes: sample.to_vec() }
    }
}

const SANDBOX_KEY: &str = "sandbox";

const SANDBOX_STARTER: &str = "use nom::{IResult, Parser, bytes::complete::take, number::complete::le_u32};\n\n// Any file: open one with the button, or drop it on the window.\n// Each variable takes a color, its bytes above take the same.\nfn parse(input: &[u8]) -> IResult<&[u8], Vec<u8>> {\n    let (input, magic) = take(4usize)(input)?;\n    let (input, version) = le_u32(input)?;\n    dbg_bytes_and_tail!(version);\n    Ok((input, magic.to_vec()))\n}\n";

/// The xxd view shows at most this many lines, 16 bytes each.
const MAX_XXD_LINES: usize = 512;
/// The text view shows at most this many bytes.
const MAX_TEXT_BYTES: usize = 16 * 1024;
/// The trace shows at most this many lines.
const MAX_TRACE_LINES: usize = 300;
/// The parser runs after the typing pauses this long, in seconds.
const DEBOUNCE: f32 = 0.12;

/// The state of the screen.
#[derive(Resource)]
pub struct Bench {
    pub key: String,
    pub level: Option<&'static Level>,
    pub file: String,
    pub payload: Vec<u8>,
    /// xxd, or the text.
    pub hex: bool,
    pub src: String,
    pub dirty_at: Option<f32>,
    pub outcome: Option<Outcome>,
    pub expected: Option<Val>,
    pub check: Option<Check>,
    /// Counts the runs, the views follow it.
    pub version: u64,
    pub saved_at: f32,
}

#[derive(Component)]
pub struct Editor;
#[derive(Component)]
struct PayloadView;
#[derive(Component)]
struct TraceView;
#[derive(Component)]
struct FieldsView;
#[derive(Component)]
struct HintsView;
#[derive(Component)]
struct StatusLabel;
#[derive(Component)]
struct FileLabel;
#[derive(Component)]
struct ViewToggleLabel;
#[derive(Component)]
struct NextButton;
#[derive(Component)]
struct Gutter;

pub struct WorkbenchPlugin;

impl Plugin for WorkbenchPlugin {
    fn build(&self, app: &mut App) {
        let on_bench = resource_exists::<Bench>.and_then(in_state(Screen::Level).or_else(in_state(Screen::Sandbox)));
        app.init_resource::<Current>()
            .init_resource::<SandboxFile>()
            .add_systems(OnEnter(Screen::Level), enter_level)
            .add_systems(OnEnter(Screen::Sandbox), enter_sandbox)
            .add_systems(OnExit(Screen::Level), save_code)
            .add_systems(OnExit(Screen::Sandbox), save_code)
            .add_systems(Update, (take_files, watch_editor, run_parser, refresh, sync_gutter, tab_inserts_spaces).chain().run_if(on_bench));
    }
}

fn enter_level(mut commands: Commands, current: Res<Current>, progress: Res<Progress>, fonts: Res<Fonts>) {
    let level = &LEVELS[current.0.min(LEVELS.len() - 1)];
    let src = progress.code.get(level.key).cloned().unwrap_or_else(|| level.starter.to_string());
    let bench = Bench {
        key: level.key.to_string(),
        level: Some(level),
        file: level.file.to_string(),
        payload: level.payload.to_vec(),
        hex: level.binary(),
        src: src.clone(),
        dirty_at: Some(f32::MIN),
        outcome: None,
        expected: level.expected(),
        check: None,
        version: 0,
        saved_at: 0.0,
    };
    spawn_screen(&mut commands, &fonts, &bench, Screen::Level, current.0, &progress);
    commands.insert_resource(bench);
}

fn enter_sandbox(mut commands: Commands, file: Res<SandboxFile>, progress: Res<Progress>, fonts: Res<Fonts>) {
    let src = progress.code.get(SANDBOX_KEY).cloned().unwrap_or_else(|| SANDBOX_STARTER.to_string());
    let bench = Bench {
        key: SANDBOX_KEY.into(),
        level: None,
        file: file.name.clone(),
        payload: file.bytes.clone(),
        hex: std::str::from_utf8(&file.bytes).is_err(),
        src,
        dirty_at: Some(f32::MIN),
        outcome: None,
        expected: None,
        check: None,
        version: 0,
        saved_at: 0.0,
    };
    spawn_screen(&mut commands, &fonts, &bench, Screen::Sandbox, 0, &progress);
    commands.insert_resource(bench);
}

fn save_code(bench: Option<Res<Bench>>, mut progress: ResMut<Progress>) {
    if let Some(bench) = bench {
        progress.code.insert(bench.key.clone(), bench.src.clone());
        progress.save();
    }
}

fn caption(text: &str, fonts: &Fonts) -> impl Bundle {
    label(text.to_uppercase(), fonts.bold(12.0), colors::DIM)
}

fn code_block(text: &str, fonts: &Fonts) -> impl Bundle {
    (
        Node { padding: UiRect::all(px(10)), border_radius: BorderRadius::all(px(6)), ..default() },
        BackgroundColor(colors::CODE_BG),
        children![label(text, fonts.mono(14.0), colors::TEXT)],
    )
}

fn spawn_screen(commands: &mut Commands, fonts: &Fonts, bench: &Bench, screen: Screen, index: usize, progress: &Progress) {
    commands.spawn(screen_root(screen)).with_children(|root| {
        // The top bar.
        root.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(10),
            padding: UiRect::axes(px(12), px(8)),
            ..default()
        })
        .with_children(|bar| {
            bar.spawn(button("← Levels", fonts, colors::BUTTON))
                .observe(|_: On<PointerClick>, mut next: ResMut<NextState<Screen>>| next.set(Screen::Menu));
            let title = match bench.level {
                Some(l) => format!("{:02}  {}", index + 1, l.title),
                None => "Sandbox".to_string(),
            };
            bar.spawn(label(title, fonts.display(30.0), colors::TEXT));
            if let Some(level) = bench.level {
                bar.spawn(label(level.chapter.title(), fonts.text(15.0), colors::DIM));
            }
            bar.spawn(Node { flex_grow: 1.0, ..default() });
            if bench.level.is_some() {
                bar.spawn(button("Reset", fonts, colors::BUTTON)).observe(reset_code);
                let prev = index.checked_sub(1);
                if let Some(prev) = prev {
                    bar.spawn(button("← Previous", fonts, colors::BUTTON)).observe(
                        move |_: On<PointerClick>, mut c: ResMut<Current>, mut next: ResMut<NextState<Screen>>| {
                            c.0 = prev;
                            next.set(Screen::Level);
                        },
                    );
                }
                if index + 1 < LEVELS.len() {
                    let solved = progress.solved.contains(LEVELS[index].key);
                    bar.spawn((button("Next →", fonts, if solved { colors::OK.darker(0.35) } else { colors::BUTTON }), NextButton))
                        .observe(move |_: On<PointerClick>, mut c: ResMut<Current>, mut next: ResMut<NextState<Screen>>| {
                            c.0 = index + 1;
                            next.set(Screen::Level);
                        });
                }
            } else {
                bar.spawn(button("Open a file", fonts, colors::ACCENT.darker(0.45)))
                    .observe(|_: On<PointerClick>, mut open: ResMut<OpenDialog>| open.0 = true);
            }
        });

        // The level on the left, the file, the parser and the trace on the right.
        root.spawn(Node {
            flex_direction: FlexDirection::Row,
            flex_grow: 1.0,
            min_height: px(0),
            column_gap: px(10),
            padding: UiRect { left: px(10), right: px(10), bottom: px(10), top: px(0) },
            ..default()
        })
        .with_children(|body| {
            body.spawn(panel_node(Node { width: percent(32), min_width: px(280), ..default() })).with_children(|left| {
                left.spawn(scroll_column(10.0, 14.0)).observe(scroll_on_wheel).observe(scroll_on_drag).with_children(|col| {
                    match bench.level {
                        Some(level) => level_side(col, fonts, level, progress),
                        None => sandbox_side(col, fonts),
                    }
                });
            });
            body.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_width: px(0), row_gap: px(10), ..default() })
                .with_children(|right| {
                    right.spawn(panel(1.0)).with_children(|p| {
                        p.spawn(panel_title()).with_children(|t| {
                            t.spawn(caption("File", fonts));
                            t.spawn((label(file_label(bench), fonts.mono(13.0), colors::TEXT), FileLabel));
                            t.spawn(Node { flex_grow: 1.0, ..default() });
                            t.spawn(button_frame(colors::BUTTON))
                                .with_children(|b| {
                                    b.spawn((
                                        label(if bench.hex { "Show text" } else { "Show hex" }, fonts.text(14.0), colors::TEXT),
                                        ViewToggleLabel,
                                    ));
                                })
                                .observe(
                                    |_: On<PointerClick>, mut bench: ResMut<Bench>, mut labels: Query<&mut Text, With<ViewToggleLabel>>| {
                                        bench.hex = !bench.hex;
                                        bench.version += 1;
                                        for mut text in &mut labels {
                                            text.0 = if bench.hex { "Show text".into() } else { "Show hex".into() };
                                        }
                                    },
                                );
                        });
                        p.spawn((scroll_column(2.0, 12.0), BackgroundColor(colors::CODE_BG), PayloadView))
                            .observe(scroll_on_wheel)
                            .observe(scroll_on_drag);
                    });
                    right.spawn(panel(1.4)).with_children(|p| {
                        p.spawn(panel_title()).with_children(|t| {
                            t.spawn(caption("Parser", fonts));
                            t.spawn(label("runs as you type", fonts.text(13.0), colors::DIM));
                            t.spawn(Node { flex_grow: 1.0, ..default() });
                            t.spawn((label("", fonts.text(14.0), colors::DIM), StatusLabel));
                        });
                        let mut editable = EditableText::new(&bench.src);
                        editable.allow_newlines = true;
                        editable.visible_lines = None;
                        p.spawn(Node { flex_direction: FlexDirection::Row, flex_grow: 1.0, min_height: px(0), ..default() }).with_children(
                            |p| {
                                // The line numbers, scrolled with the editor by `sync_gutter`.
                                p.spawn((
                                    Node {
                                        width: px(46),
                                        padding: UiRect { top: px(10), right: px(8), ..default() },
                                        overflow: Overflow::clip(),
                                        justify_content: JustifyContent::End,
                                        ..default()
                                    },
                                    BackgroundColor(colors::CODE_BG),
                                ))
                                .with_children(|g| {
                                    g.spawn((
                                        Text::default(),
                                        fonts.mono(15.0),
                                        TextColor(colors::DIM),
                                        TextLayout::new(Justify::Right, LineBreak::NoWrap),
                                        Node::default(),
                                        Gutter,
                                        Pickable::IGNORE,
                                    ));
                                });
                                p.spawn((
                                    Node {
                                        flex_grow: 1.0,
                                        min_height: px(0),
                                        min_width: px(0),
                                        padding: UiRect { top: px(10), bottom: px(10), right: px(10), left: px(4) },
                                        ..default()
                                    },
                                    BackgroundColor(colors::CODE_BG),
                                    TextInput,
                                    editable,
                                    TextLayout::no_wrap(),
                                    fonts.mono(15.0),
                                    TextColor(colors::TEXT),
                                    TextCursorStyle {
                                        color: colors::ACCENT,
                                        selection_color: colors::ACCENT.with_alpha(0.3),
                                        unfocused_selection_color: colors::ACCENT.with_alpha(0.12),
                                        selected_text_color: None,
                                        selection_radius: 0.1,
                                    },
                                    AutoFocus,
                                    Editor,
                                ));
                            },
                        );
                    });
                    right.spawn(panel(0.9)).with_children(|p| {
                        p.spawn(panel_title()).with_children(|t| {
                            t.spawn(caption("Trace", fonts));
                            t.spawn(label("dbg_bytes_and_tail! of every variable", fonts.text(13.0), colors::DIM));
                        });
                        p.spawn((scroll_column(2.0, 12.0), BackgroundColor(colors::CODE_BG), TraceView))
                            .observe(scroll_on_wheel)
                            .observe(scroll_on_drag);
                    });
                });
        });
    });
}

fn file_label(bench: &Bench) -> String {
    format!("{}  {} bytes", bench.file, bench.payload.len())
}

fn level_side(col: &mut ChildSpawnerCommands, fonts: &Fonts, level: &Level, progress: &Progress) {
    col.spawn(label(level.brief, fonts.text(17.0), colors::TEXT));
    col.spawn(caption("The struct to return", fonts));
    col.spawn(code_block(level.target, fonts));
    col.spawn(caption("Fields", fonts));
    col.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(4), ..default() }, FieldsView));
    col.spawn(Node {
        flex_direction: FlexDirection::Row,
        column_gap: px(8),
        align_items: AlignItems::Center,
        margin: UiRect::top(px(6)),
        ..default()
    })
    .with_children(|row| {
        row.spawn(caption("Hints", fonts));
        row.spawn(button("Show a hint", fonts, colors::BUTTON)).observe(show_hint);
    });
    col.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }, HintsView)).with_children(|h| {
        spawn_hints(h, fonts, level, progress.hints.get(level.key).copied().unwrap_or(0));
    });
    col.spawn(caption("Notes", fonts));
    for paragraph in level.notes.split("\n\n") {
        col.spawn(label(paragraph, fonts.text(15.0), colors::TEXT.with_alpha(0.85)));
    }
}

fn sandbox_side(col: &mut ChildSpawnerCommands, fonts: &Fonts) {
    col.spawn(label(
        "Any file, any parser: open a file, or drop it on the window, then write the parser that reads it.",
        fonts.text(17.0),
        colors::TEXT,
    ));
    col.spawn(label("The function `parse` runs, on `&[u8]` for binary files or `&str` for text. Each `let` gets a color, and its bytes in the file take it.", fonts.text(15.0), colors::TEXT.with_alpha(0.85)));
    col.spawn(caption("Debugging", fonts));
    col.spawn(code_block("dbg_bytes_and_tail!(name);\ndbg_displayable_and_tail!(name);\ndbg_peek_hex(parser, \"what\")\ncontext(\"what\", parser)\nprintln!(\"{name:?}\");", fonts));
    col.spawn(label(
        "They write to the trace, as in s2protocol: the offset, the length, the first bytes and the bytes after.",
        fonts.text(15.0),
        colors::TEXT.with_alpha(0.85),
    ));
    col.spawn(caption("Combinators", fonts));
    col.spawn(label(crate::script::parsers::COMBINATORS.join(", "), fonts.mono(12.0), colors::DIM));
    col.spawn(caption("Parsers", fonts));
    col.spawn(label(crate::script::parsers::BUILTINS.join(", "), fonts.mono(12.0), colors::DIM));
}

fn spawn_hints(h: &mut ChildSpawnerCommands, fonts: &Fonts, level: &Level, shown: usize) {
    for hint in level.hints.iter().take(shown) {
        h.spawn(code_block(hint, fonts));
    }
    if shown >= level.hints.len() && shown > 0 {
        h.spawn(caption("A solution", fonts));
        h.spawn(code_block(level.solution.trim_end(), fonts));
    }
}

fn show_hint(
    _: On<PointerClick>,
    bench: Res<Bench>,
    mut progress: ResMut<Progress>,
    views: Query<Entity, With<HintsView>>,
    fonts: Res<Fonts>,
    mut commands: Commands,
) {
    let Some(level) = bench.level else { return };
    let shown = progress.hints.entry(level.key.to_string()).or_default();
    // After the last hint, a solution shows too.
    *shown = (*shown + 1).min(level.hints.len());
    let shown = *shown;
    progress.save();
    for view in &views {
        commands.entity(view).despawn_children().with_children(|h| spawn_hints(h, &fonts, level, shown));
    }
}

fn reset_code(_: On<PointerClick>, bench: Res<Bench>, mut editors: Query<&mut EditableText, With<Editor>>) {
    let Some(level) = bench.level else { return };
    for mut editable in &mut editors {
        editable.editor_mut().set_text(level.starter);
        editable.queue_edit(TextEdit::TextStart(false));
    }
}

/// The sandbox takes the files opened or dropped.
fn take_files(
    mut incoming: ResMut<Incoming>,
    mut file: ResMut<SandboxFile>,
    mut bench: ResMut<Bench>,
    mut labels: Query<&mut Text, With<FileLabel>>,
    time: Res<Time>,
) {
    let Some((name, bytes)) = incoming.0.pop() else { return };
    incoming.0.clear();
    if bench.level.is_some() {
        // A file dropped on a level opens in the sandbox next time.
        *file = SandboxFile { name, bytes };
        return;
    }
    bench.hex = std::str::from_utf8(&bytes).is_err();
    bench.file = name.clone();
    bench.payload = bytes.clone();
    bench.dirty_at = Some(time.elapsed_secs() - DEBOUNCE);
    *file = SandboxFile { name, bytes };
    for mut text in &mut labels {
        text.0 = file_label(&bench);
    }
}

fn watch_editor(editors: Query<&EditableText, With<Editor>>, mut bench: ResMut<Bench>, time: Res<Time>) {
    let Ok(editable) = editors.single() else { return };
    let value = editable.value();
    if value != bench.src.as_str() {
        bench.src = value.to_string();
        bench.dirty_at = Some(time.elapsed_secs());
    }
}

fn run_parser(mut bench: ResMut<Bench>, mut progress: ResMut<Progress>, time: Res<Time>) {
    let now = time.elapsed_secs();
    let Some(at) = bench.dirty_at else { return };
    if now - at < DEBOUNCE {
        return;
    }
    bench.dirty_at = None;
    let outcome = script::run(&bench.src, &bench.payload);
    let check = match (bench.level, &bench.expected) {
        (Some(level), Some(expected)) => Some(levels::check(level, expected, &outcome)),
        _ => None,
    };
    if let (Some(level), Some(check)) = (bench.level, &check)
        && check.solved
        && !progress.solved.contains(level.key)
    {
        progress.solved.insert(level.key.to_string());
        progress.code.insert(bench.key.clone(), bench.src.clone());
        progress.save();
        bench.saved_at = now;
    }
    // The code is saved while typing, at most every two seconds.
    if now - bench.saved_at > 2.0 {
        progress.code.insert(bench.key.clone(), bench.src.clone());
        progress.save();
        bench.saved_at = now;
    }
    bench.outcome = Some(outcome);
    bench.check = check;
    bench.version += 1;
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh(
    mut seen: Local<u64>,
    bench: Res<Bench>,
    fonts: Res<Fonts>,
    payload_views: Query<Entity, With<PayloadView>>,
    trace_views: Query<Entity, With<TraceView>>,
    fields_views: Query<Entity, With<FieldsView>>,
    mut status: Query<(&mut Text, &mut TextColor), With<StatusLabel>>,
    mut next_buttons: Query<&mut Btn, With<NextButton>>,
    gutters: Query<Entity, With<Gutter>>,
    mut commands: Commands,
) {
    if bench.version == *seen && !bench.is_added() {
        return;
    }
    *seen = bench.version;
    let Some(outcome) = &bench.outcome else { return };
    let mono = fonts.mono(14.0);
    let styles = debug::byte_styles(&bench.payload, &outcome.bindings);

    let error_line = match &outcome.result {
        Err(RunError::Script(e)) => Some(e.line),
        Err(RunError::Parse { line, .. }) => Some(*line),
        Ok(_) => None,
    };
    let lines = bench.src.split('\n').count().max(1);
    for gutter in &gutters {
        commands.entity(gutter).despawn_children().with_children(|g| {
            for n in 1..=lines {
                let color = if Some(n) == error_line { colors::BAD } else { colors::DIM.with_alpha(0.6) };
                g.spawn((TextSpan::new(if n < lines { format!("{n}\n") } else { n.to_string() }), fonts.mono(15.0), TextColor(color)));
            }
        });
    }

    for view in &payload_views {
        commands.entity(view).despawn_children().with_children(|p| {
            if bench.hex {
                let lines = debug::xxd(&bench.payload, &styles, MAX_XXD_LINES);
                for line in lines {
                    spawn_styled(p, line.into_iter().map(|c| (c.text, c.style, c.shade)), &mono);
                }
                if bench.payload.len() > MAX_XXD_LINES * debug::XXD_WIDTH {
                    p.spawn(label(
                        format!("… {} more bytes", bench.payload.len() - MAX_XXD_LINES * debug::XXD_WIDTH),
                        fonts.text(13.0),
                        colors::DIM,
                    ));
                }
            } else {
                let cells = debug::text_view(&bench.payload, &styles, MAX_TEXT_BYTES);
                spawn_styled(p, cells.into_iter().map(|c| (c.text, c.style, c.shade)), &fonts.mono(22.0));
                if bench.payload.len() > MAX_TEXT_BYTES {
                    p.spawn(label(format!("… {} more bytes", bench.payload.len() - MAX_TEXT_BYTES), fonts.text(13.0), colors::DIM));
                }
            }
        });
    }

    for view in &trace_views {
        commands.entity(view).despawn_children().with_children(|t| {
            for line in debug::trace(outcome, &bench.payload).into_iter().take(MAX_TRACE_LINES) {
                spawn_styled(t, line.into_iter().map(|s| (s.text, s.style, false)), &fonts.mono(13.0));
            }
        });
    }

    if let Some(check) = &bench.check {
        for view in &fields_views {
            commands.entity(view).despawn_children().with_children(|f| {
                for field in &check.fields {
                    let (mark, color) = if field.ok { ("✓", colors::OK) } else { ("✗", colors::BAD) };
                    let line = match (&field.got, field.ok) {
                        (_, true) => format!("{mark} {}: {}", field.name, field.expected),
                        (Some(got), false) => format!("{mark} {}: {got}, expected {}", field.name, field.expected),
                        (None, false) => format!("{mark} {}: expected {}", field.name, field.expected),
                    };
                    f.spawn(label(line, fonts.mono(13.0), color));
                }
            });
        }
        if check.solved {
            for mut btn in &mut next_buttons {
                btn.color = colors::OK.darker(0.35);
                btn.hover = colors::OK.darker(0.25);
            }
        }
    }

    let (text, color) = match (&bench.check, &outcome.result) {
        (Some(check), _) if check.solved => ("Solved! On to the next level".to_string(), colors::OK),
        (Some(check), Err(_)) => (check.status.clone(), colors::BAD),
        (Some(check), Ok(_)) => (check.status.clone(), colors::ACCENT),
        (None, Ok((rest, v))) => {
            let left = bench.payload.len() - rest;
            (format!("Ok: {}{}", v.show(60), if left > 0 { format!(", {left} bytes left") } else { String::new() }), colors::OK)
        }
        (None, Err(RunError::Script(e))) => (e.to_string(), colors::BAD),
        (None, Err(RunError::Parse { err, line })) => (format!("line {line}: ErrorKind::{} at byte {}", err.kind, err.pos), colors::BAD),
    };
    for (mut t, mut c) in &mut status {
        t.0 = text.clone();
        c.0 = color;
    }
}

/// The line numbers follow the vertical scroll of the editor.
fn sync_gutter(editors: Query<(&EditableText, &ComputedNode), With<Editor>>, mut gutters: Query<&mut Node, With<Gutter>>) {
    let Ok((editable, node)) = editors.single() else { return };
    let top = px(-editable.viewport.offset.y * node.inverse_scale_factor);
    for mut gutter in &mut gutters {
        if gutter.top != top {
            gutter.top = top;
        }
    }
}

/// Tab indents in the editor; nothing else uses it.
fn tab_inserts_spaces(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut editors: Query<(Entity, &mut EditableText), With<Editor>>,
) {
    if !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    for (entity, mut editable) in &mut editors {
        if focus.get() == Some(entity) {
            editable.queue_edit(TextEdit::Insert("    ".into()));
        }
    }
}
