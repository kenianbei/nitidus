//! UI toolkit for nitidus: theme system, layout helpers, widget builders,
//! and interactive widget primitives.

pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn crate_version_is_nonempty() {
        assert!(!crate_version().is_empty());
    }
}
