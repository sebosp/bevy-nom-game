//! The levels by chapter, and the sandbox.
use bevy::picking::events::PointerClick;
use bevy::prelude::*;

use super::workbench::Current;
use crate::levels::{Chapter, LEVELS};
use crate::progress::Progress;
use crate::ui::*;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Menu), spawn);
    }
}

const CHAPTERS: [Chapter; 5] = [Chapter::Text, Chapter::Numbers, Chapter::Records, Chapter::Framing, Chapter::Binary];

fn spawn(mut commands: Commands, fonts: Res<Fonts>, progress: Res<Progress>) {
    commands.spawn(screen_root(Screen::Menu)).with_children(|root| {
        root.spawn(scroll_column(18.0, 28.0)).observe(scroll_on_wheel).observe(scroll_on_drag).with_children(|list| {
            list.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(4), margin: UiRect::bottom(px(8)), ..default() })
                .with_children(|head| {
                    head.spawn(label("Nom Nom Bytes", fonts.display(54.0), colors::ACCENT));
                    head.spawn(label("Write the nom parser, watch it eat the file byte by byte.", fonts.text(18.0), colors::DIM));
                    let solved = LEVELS.iter().filter(|l| progress.solved.contains(l.key)).count();
                    head.spawn(label(format!("{solved} of {} levels solved", LEVELS.len()), fonts.text(15.0), colors::DIM));
                });
            let mut number = 0;
            for chapter in CHAPTERS {
                list.spawn(label(chapter.title(), fonts.display(28.0), colors::TEXT));
                list.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(12),
                    row_gap: px(12),
                    ..default()
                })
                .with_children(|row| {
                    for (i, level) in LEVELS.iter().enumerate().filter(|(_, l)| l.chapter == chapter) {
                        number += 1;
                        let solved = progress.solved.contains(level.key);
                        level_card(row, &fonts, number, i, level.title, level.file, solved);
                    }
                });
            }
            list.spawn(label("Sandbox", fonts.display(28.0), colors::TEXT));
            list.spawn(button_node(
                colors::BUTTON,
                Node {
                    width: px(420),
                    padding: UiRect::all(px(14)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Start,
                    row_gap: px(4),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
            ))
            .with_children(|card| {
                card.spawn(label("Your own file", fonts.bold(18.0), colors::TEXT));
                card.spawn(label("Open any file, or drop it on the window, and parse it.", fonts.text(14.0), colors::DIM));
            })
            .observe(|_: On<PointerClick>, mut next: ResMut<NextState<Screen>>| next.set(Screen::Sandbox));
        });
    });
}

fn level_card(row: &mut ChildSpawnerCommands, fonts: &Fonts, number: usize, index: usize, title: &str, file: &str, solved: bool) {
    let border = if solved { colors::OK } else { colors::BORDER };
    row.spawn((
        button_node(
            colors::PANEL,
            Node {
                width: px(250),
                padding: UiRect::all(px(12)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                row_gap: px(4),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
        ),
        BorderColor::all(border),
    ))
    .with_children(|card| {
        card.spawn(Node { flex_direction: FlexDirection::Row, column_gap: px(8), align_items: AlignItems::Baseline, ..default() })
            .with_children(|top| {
                top.spawn(label(format!("{number:02}"), fonts.display(22.0), if solved { colors::OK } else { colors::ACCENT }));
                top.spawn(label(title, fonts.bold(16.0), colors::TEXT));
            });
        card.spawn(label(if solved { format!("{file}  ✓") } else { file.to_string() }, fonts.mono(13.0), colors::DIM));
    })
    .observe(move |_: On<PointerClick>, mut current: ResMut<Current>, mut next: ResMut<NextState<Screen>>| {
        current.0 = index;
        next.set(Screen::Level);
    });
}
