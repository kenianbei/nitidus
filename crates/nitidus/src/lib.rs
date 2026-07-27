//! Application wiring for the `nitidus` binary.

pub mod accounts;
pub mod action;
pub mod addresses;
pub mod app;
pub mod bootstrap;
pub mod clipboard;
pub mod cmdline;
pub mod command;
pub mod compose;
pub mod config;
pub mod contacts;
pub mod dirs;
pub mod engine;
pub mod explorer;
pub mod focus;
pub mod help;
pub mod index;
pub mod keymap;
pub mod logging;
pub mod mouse;
pub mod outbox;
pub mod overlay;
pub mod pager;
pub mod panes;
pub mod router;
pub mod shell;
pub mod sidebar;
pub mod status;
pub mod store;
pub mod toast;

pub fn run(loaded: config::LoadedConfig) -> anyhow::Result<()> {
    let keymaps = keymap::Keymaps::compile(&loaded.keymaps)?;
    let setup = bootstrap::bootstrap(&loaded.config)?;
    // Terminal graphics negotiation must precede the TUI owning stdio.
    let photo_picker = contacts::PhotoPicker::detect();
    tracing::info!("nitidus {} starting", env!("CARGO_PKG_VERSION"));
    let mut app = app::build_app(loaded, keymaps, setup);
    app.insert_resource(photo_picker);
    let exit = app.run();
    if let Some(cache) = app.world_mut().remove_resource::<engine::CacheResource>() {
        cache.0.close();
    }
    if let bevy::app::AppExit::Error(code) = exit {
        anyhow::bail!("app exited with error code {code}");
    }
    tracing::info!("nitidus exited cleanly");
    Ok(())
}
