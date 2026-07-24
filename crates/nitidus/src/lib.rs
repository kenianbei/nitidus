//! Application wiring for the `nitidus` binary.

pub mod action;
pub mod app;
pub mod cmdline;
pub mod config;
pub mod dirs;
pub mod engine;
pub mod keymap;
pub mod logging;
pub mod router;
pub mod shell;
pub mod status;

pub fn run(loaded: config::LoadedConfig) -> anyhow::Result<()> {
    let keymaps = keymap::Keymaps::compile(&loaded.keymaps)?;
    let mut mail_engine = nitidus_mail::MailEngine::new(loaded.config.accounts.len())?;
    let notices = engine::register_accounts(&mut mail_engine, &loaded.config)?;
    tracing::info!("nitidus {} starting", env!("CARGO_PKG_VERSION"));
    let exit = app::build_app(loaded, keymaps, mail_engine, notices).run();
    if let bevy::app::AppExit::Error(code) = exit {
        anyhow::bail!("app exited with error code {code}");
    }
    tracing::info!("nitidus exited cleanly");
    Ok(())
}
