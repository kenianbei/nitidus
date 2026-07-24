//! Bevy app assembly: plugins, frame pacing, and global resources.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy_ratatui::RatatuiPlugins;
use nitidus_ui_kit::theme;
use plurimus::PlurimusPlugin;

use crate::shell::ShellPlugin;

const FRAMES_PER_SECOND: f64 = 30.0;

pub fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / FRAMES_PER_SECOND,
        ))),
        RatatuiPlugins {
            enable_kitty_protocol: true,
            enable_input_forwarding: true,
            enable_mouse_capture: true,
        },
        StatesPlugin,
        PlurimusPlugin,
    ));
    app.insert_resource(Time::<Fixed>::from_seconds(1.0 / FRAMES_PER_SECOND));
    app.insert_resource(theme::tailwind_dark());
    app.add_plugins(ShellPlugin);
    app
}
