//! Application wiring for the `nitidus` binary.

pub mod app;
pub mod config;
pub mod dirs;
pub mod logging;
pub mod shell;

pub fn run(loaded: config::LoadedConfig) -> anyhow::Result<()> {
    tracing::info!("nitidus {} starting", env!("CARGO_PKG_VERSION"));
    let exit = app::build_app(loaded).run();
    if let bevy::app::AppExit::Error(code) = exit {
        anyhow::bail!("app exited with error code {code}");
    }
    tracing::info!("nitidus exited cleanly");
    Ok(())
}
