//! Application wiring for the `nitidus` binary.

pub mod action;
pub mod app;
pub mod bootstrap;
pub mod cmdline;
pub mod command;
pub mod config;
pub mod dirs;
pub mod engine;
pub mod help;
pub mod index;
pub mod keymap;
pub mod logging;
pub mod overlay;
pub mod pager;
pub mod router;
pub mod screen;
pub mod sidebar;
pub mod shell;
pub mod status;
pub mod store;

pub fn run(loaded: config::LoadedConfig) -> anyhow::Result<()> {
    let keymaps = keymap::Keymaps::compile(&loaded.keymaps)?;
    let setup = bootstrap::bootstrap(&loaded.config)?;
    tracing::info!("nitidus {} starting", env!("CARGO_PKG_VERSION"));
    let mut app = app::build_app(loaded, keymaps, setup);
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
