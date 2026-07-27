//! Bevy app assembly: plugins, frame pacing, and global resources.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy_ratatui::RatatuiPlugins;
use nitidus_ui_kit::theme;
use plurimus::PlurimusPlugin;

use crate::bootstrap::EngineSetup;
use crate::cmdline::CommandLinePlugin;
use crate::compose::ComposePlugin;
use crate::config::LoadedConfig;
use crate::contacts::ContactsPlugin;
use crate::engine::{CacheResource, EnginePlugin, EngineResource, StartupNotices};
use crate::explorer::ExplorerPlugin;
use crate::index::IndexPlugin;
use crate::keymap::Keymaps;
use crate::outbox::OutboxPlugin;
use crate::overlay::OverlayPlugin;
use crate::pager::PagerPlugin;
use crate::router::RouterPlugin;
use crate::shell::ShellPlugin;
use crate::sidebar::SidebarPlugin;
use crate::toast::ToastPlugin;

const FRAMES_PER_SECOND: f64 = 30.0;

pub fn build_app(loaded: LoadedConfig, keymaps: Keymaps, setup: EngineSetup) -> App {
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
    app.insert_resource(EngineResource(setup.engine));
    app.insert_resource(setup.store);
    app.insert_resource(setup.tracker);
    let mut notices = loaded.notices.clone();
    notices.extend(setup.notices);
    app.insert_resource(StartupNotices(notices));
    app.insert_resource(crate::addresses::AddressIndex::from_loaded(setup.addresses));
    if let Some(cache) = setup.cache {
        app.insert_resource(CacheResource(cache));
    }
    app.add_plugins((
        ShellPlugin,
        IndexPlugin,
        ContactsPlugin,
        PagerPlugin,
        SidebarPlugin,
        OverlayPlugin,
        ExplorerPlugin,
        RouterPlugin,
        CommandLinePlugin,
        ComposePlugin,
        OutboxPlugin,
        ToastPlugin,
        EnginePlugin,
        crate::accounts::AccountsPlugin,
    ));
    app.add_plugins(crate::panes::PaneRulesPlugin);
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
