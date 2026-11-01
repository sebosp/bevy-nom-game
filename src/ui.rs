//! The look shared by the screens: the palette, the fonts, the buttons, the scrolling.
use bevy::color::Mix;
use bevy::input::mouse::MouseScrollPixelsPerLine;
use bevy::picking::events::{PointerDrag, PointerScroll};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};

use crate::debug::Style;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    #[default]
    Menu,
    Level,
    /// Any file, any parser, no struct to fill.
    Sandbox,
}

pub mod colors {
    use bevy::prelude::Color;

    pub const BG: Color = Color::srgb_u8(0x11, 0x13, 0x18);
    pub const PANEL: Color = Color::srgb_u8(0x19, 0x1c, 0x24);
    /// The code: the editor, the struct, the trace.
    pub const CODE_BG: Color = Color::srgb_u8(0x0c, 0x0d, 0x11);
    pub const BORDER: Color = Color::srgb_u8(0x2a, 0x2e, 0x3a);
    pub const BUTTON: Color = Color::srgb_u8(0x26, 0x2a, 0x36);
    pub const TEXT: Color = Color::srgb_u8(0xe4, 0xe4, 0xe0);
    pub const DIM: Color = Color::srgb_u8(0x8a, 0x8f, 0x9c);
    /// The color of nom, the bytes being eaten.
    pub const ACCENT: Color = Color::srgb_u8(0xff, 0xb4, 0x54);
    pub const OK: Color = Color::srgb_u8(0x8b, 0xd4, 0x7c);
    pub const BAD: Color = Color::srgb_u8(0xff, 0x6b, 0x6b);

    /// The bytes not parsed yet: the gray of the first look at a file.
    pub const UNPARSED: Color = Color::srgb_u8(0x6b, 0x70, 0x80);
    pub const UNPARSED_NULL: Color = Color::srgb_u8(0x3e, 0x42, 0x4d);
    /// The bytes consumed into `_`.
    pub const DISCARDED: Color = Color::srgb_u8(0xa3, 0xa8, 0xb4);

    /// The variables, in the order of the `let`s: hues apart on the dark background.
    pub const BINDINGS: [Color; 8] = [
        Color::srgb_u8(0xff, 0x7a, 0x90),
        Color::srgb_u8(0x5f, 0xd7, 0xe0),
        Color::srgb_u8(0xe6, 0xdb, 0x74),
        Color::srgb_u8(0xc7, 0x92, 0xea),
        Color::srgb_u8(0x8b, 0xd4, 0x7c),
        Color::srgb_u8(0xff, 0xa0, 0x50),
        Color::srgb_u8(0x7a, 0xa2, 0xf7),
        Color::srgb_u8(0xf0, 0x8c, 0xc8),
    ];

    pub fn binding(i: usize) -> Color {
        BINDINGS[i % BINDINGS.len()]
    }
}

/// The text color and the background of a style; `shade` is the lighter half of a binding: what
/// a parser consumed around the value, every other item of a `Vec`.
pub fn style_colors(style: Style, shade: bool) -> (Color, Option<Color>) {
    let tint = |c: Color, alpha: f32| Some(c.with_alpha(alpha));
    match style {
        Style::Plain => (colors::TEXT, None),
        Style::Dim => (colors::DIM, None),
        Style::Printable | Style::Ok => (colors::OK, None),
        Style::Null => (Color::srgb_u8(0x5a, 0x5f, 0x6b), Some(Color::BLACK)),
        Style::Other => (Color::WHITE, None),
        Style::Bracket => (Color::srgb_u8(0x5f, 0xe7, 0xf0), None),
        Style::Offset => (Color::srgb_u8(0x3f, 0x8f, 0x99), None),
        Style::Length => (Color::srgb_u8(0xff, 0xe0, 0x66), None),
        Style::LengthDim => (Color::srgb_u8(0x9c, 0x8a, 0x3c), None),
        Style::Value => (Color::srgb_u8(0xe0, 0x70, 0xe0), None),
        Style::Error => (colors::BAD, None),
        Style::Unparsed => (colors::UNPARSED, None),
        Style::UnparsedNull => (colors::UNPARSED_NULL, None),
        Style::Discarded => (colors::DISCARDED, tint(colors::DISCARDED, 0.10)),
        Style::Binding(i) => {
            let c = colors::binding(i);
            if shade { (c.mix(&Color::WHITE, 0.55), tint(c, 0.09)) } else { (c, tint(c, 0.30)) }
        }
    }
}

#[derive(Resource, Clone)]
pub struct Fonts {
    pub mono: Handle<Font>,
    pub mono_bold: Handle<Font>,
    pub regular: Handle<Font>,
    pub bold: Handle<Font>,
    /// Barlow Condensed SemiBold: the titles.
    pub display: Handle<Font>,
}

impl Fonts {
    fn font(handle: &Handle<Font>, size: f32) -> TextFont {
        TextFont { font: FontSource::Handle(handle.clone()), font_size: FontSize::Px(size), ..default() }
    }
    pub fn mono(&self, size: f32) -> TextFont {
        Self::font(&self.mono, size)
    }
    pub fn mono_bold(&self, size: f32) -> TextFont {
        Self::font(&self.mono_bold, size)
    }
    pub fn text(&self, size: f32) -> TextFont {
        Self::font(&self.regular, size)
    }
    pub fn bold(&self, size: f32) -> TextFont {
        Self::font(&self.bold, size)
    }
    pub fn display(&self, size: f32) -> TextFont {
        Self::font(&self.display, size)
    }
}

impl FromWorld for Fonts {
    fn from_world(world: &mut World) -> Self {
        let mut fonts = world.resource_mut::<Assets<Font>>();
        let mut load = |bytes: &[u8]| fonts.add(Font::from_bytes(bytes.to_vec()));
        Fonts {
            mono: load(include_bytes!("../assets/fonts/DejaVuSansMono.ttf")),
            mono_bold: load(include_bytes!("../assets/fonts/DejaVuSansMono-Bold.ttf")),
            regular: load(include_bytes!("../assets/fonts/FiraSans-Regular.ttf")),
            bold: load(include_bytes!("../assets/fonts/FiraSans-Bold.ttf")),
            display: load(include_bytes!("../assets/fonts/BarlowCondensed-SemiBold.ttf")),
        }
    }
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Screen>().add_systems(Startup, setup).add_systems(Update, (hover_buttons, escape_goes_back));
    }

    /// The fonts are there before the first screen is entered, which happens before `Startup`.
    fn finish(&self, app: &mut App) {
        app.init_resource::<Fonts>();
    }
}

fn setup(mut commands: Commands) {
    commands.spawn((Camera2d, Camera { clear_color: ClearColorConfig::Custom(colors::BG), ..default() }));
}

/// A button's colors, for the hover.
#[derive(Component)]
pub struct Btn {
    pub color: Color,
    pub hover: Color,
}

#[allow(clippy::type_complexity)]
fn hover_buttons(mut buttons: Query<(&Hovered, &Btn, &mut BackgroundColor), Or<(Changed<Hovered>, Changed<Btn>)>>) {
    for (hovered, btn, mut bg) in &mut buttons {
        bg.0 = if hovered.get() { btn.hover } else { btn.color };
    }
}

fn escape_goes_back(keys: Res<ButtonInput<KeyCode>>, screen: Res<State<Screen>>, mut next: ResMut<NextState<Screen>>) {
    if keys.just_pressed(KeyCode::Escape) && *screen.get() != Screen::Menu {
        next.set(Screen::Menu);
    }
}

pub fn label(text: impl Into<String>, font: TextFont, color: Color) -> impl Bundle {
    (Text::new(text), font, TextColor(color), Pickable::IGNORE)
}

/// A button without content.
pub fn button_frame(color: Color) -> impl Bundle {
    button_node(
        color,
        Node {
            padding: UiRect::axes(px(14), px(7)),
            border_radius: BorderRadius::all(px(6)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

/// A button of the given layout.
pub fn button_node(color: Color, node: Node) -> impl Bundle {
    let hover = color.mix(&Color::WHITE, 0.12);
    (node, BackgroundColor(color), Btn { color, hover }, Hovered::default())
}

/// A button with a text; the click is an observer of the caller.
pub fn button(text: impl Into<String>, fonts: &Fonts, color: Color) -> impl Bundle {
    (button_frame(color), children![label(text, fonts.text(16.0), colors::TEXT)])
}

/// The root of a screen: the whole window, a column. In viewport units: the percentages of a
/// root node grow with the content, and the scroll areas under it would never scroll.
pub fn screen_root(screen: Screen) -> impl Bundle {
    (Node { width: vw(100), height: vh(100), flex_direction: FlexDirection::Column, ..default() }, DespawnOnExit(screen))
}

/// A node scrolled by the wheel and by dragging.
#[derive(Component)]
pub struct Scrollable;

/// A scrolling column: the parent gives its size.
pub fn scroll_column(gap: f32, padding: f32) -> impl Bundle {
    (
        Node {
            width: percent(100),
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(gap),
            padding: UiRect::all(px(padding)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        ScrollPosition::default(),
        Scrollable,
    )
}

/// The wheel scrolls the [`Scrollable`] under the pointer.
pub fn scroll_on_wheel(
    mut scroll: On<PointerScroll>,
    ratio: Res<MouseScrollPixelsPerLine>,
    mut areas: Query<(&ComputedNode, &mut ScrollPosition), With<Scrollable>>,
) {
    scroll.propagate(false);
    let delta = scroll.to_pixels(&ratio);
    if let Ok((node, mut position)) = areas.get_mut(scroll.entity) {
        scroll_by(node, &mut position, -delta.y);
    }
}

pub fn scroll_on_drag(mut drag: On<PointerDrag>, mut areas: Query<(&ComputedNode, &mut ScrollPosition), With<Scrollable>>) {
    drag.propagate(false);
    if let Ok((node, mut position)) = areas.get_mut(drag.entity) {
        scroll_by(node, &mut position, -drag.delta.y);
    }
}

fn scroll_by(node: &ComputedNode, position: &mut ScrollPosition, dy: f32) {
    let visible = node.size().y * node.inverse_scale_factor;
    let content = node.content_size().y * node.inverse_scale_factor;
    let y = (position.y + dy).clamp(0.0, (content - visible).max(0.0));
    if position.y != y {
        position.y = y;
    }
}

/// A panel with a small title on top.
pub fn panel(grow: f32) -> impl Bundle {
    panel_node(Node { flex_grow: grow, flex_basis: px(0), min_height: px(0), min_width: px(0), ..default() })
}

/// A panel of the given layout: a column with a border.
pub fn panel_node(node: Node) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            overflow: Overflow::clip(),
            ..node
        },
        BackgroundColor(colors::PANEL),
        BorderColor::all(colors::BORDER),
    )
}

/// The title bar of a panel: a caption, and room for buttons on the right.
pub fn panel_title() -> impl Bundle {
    Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: px(8),
        padding: UiRect::axes(px(12), px(6)),
        min_height: px(34),
        ..default()
    }
}

/// Spans of styled text: the trace and the payload are made of them.
pub fn spawn_styled(parent: &mut ChildSpawnerCommands, cells: impl IntoIterator<Item = (String, Style, bool)>, font: &TextFont) {
    parent.spawn((Text::default(), font.clone(), TextColor(colors::TEXT), TextLayout::no_wrap(), Pickable::IGNORE)).with_children(|t| {
        for (text, style, shade) in cells {
            let (fg, bg) = style_colors(style, shade);
            let mut span = t.spawn((TextSpan::new(text), font.clone(), TextColor(fg)));
            if let Some(bg) = bg {
                span.insert(TextBackgroundColor(bg));
            }
        }
    });
}
