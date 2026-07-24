//! Application wiring for the `nitidus` binary.

pub mod logging;

pub fn run() -> anyhow::Result<()> {
    tracing::info!("nitidus {} started", env!("CARGO_PKG_VERSION"));
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn run_succeeds() {
        assert!(run().is_ok());
    }
}
