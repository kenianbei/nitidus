//! Bevy app assembly: plugins, frame pacing, and global resources.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy_ratatui::RatatuiPlugins;
use nitidus_ui_kit::theme;
use plurimus::PlurimusPlugin;

use crate::cmdline::CommandLinePlugin;
use crate::config::LoadedConfig;
use crate::engine::{EnginePlugin, EngineResource};
use crate::keymap::Keymaps;
use crate::router::RouterPlugin;
use crate::shell::ShellPlugin;

const FRAMES_PER_SECOND: f64 = 30.0;

pub fn build_app(
    loaded: LoadedConfig,
    keymaps: Keymaps,
    mail_engine: nitidus_mail::MailEngine,
) -> App {
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
    app.insert_resource(select_theme(&loaded.config.ui.theme));
    app.insert_resource(loaded.config);
    app.insert_resource(keymaps);
    app.insert_resource(EngineResource(mail_engine));
    app.add_plugins((ShellPlugin, RouterPlugin, CommandLinePlugin, EnginePlugin));
    app
}

/// Theme names are validated at config load; unknown names cannot reach
/// here, so the fallback arm only defends against future presets.
fn select_theme(name: &str) -> theme::Theme {
    match name {
        crate::config::THEME_TAILWIND_DARK => theme::tailwind_dark(),
        _ => theme::tailwind_dark(),
    }
}
