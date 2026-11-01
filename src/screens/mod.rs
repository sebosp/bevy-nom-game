//! The screens: the levels, a level, the sandbox.
pub mod menu;
pub mod workbench;

use bevy::prelude::*;

pub struct ScreensPlugin;

impl Plugin for ScreensPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((menu::MenuPlugin, workbench::WorkbenchPlugin));
    }
}
