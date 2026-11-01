//! Nom Nom Bytes: the levels give a file and a struct, the player writes the nom parser that
//! reads one into the other, and sees the bytes each variable matched in its color.
pub mod debug;
pub mod files;
pub mod levels;
pub mod progress;
pub mod screens;
pub mod script;
pub mod storage;
pub mod ui;

use bevy::prelude::*;

pub struct NomGamePlugin;

impl Plugin for NomGamePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(progress::Progress::load()).add_plugins((ui::UiPlugin, files::FilesPlugin, screens::ScreensPlugin));
    }
}
