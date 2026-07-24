//! Contact management for nitidus: vCard domain model, persistence, and
//! the contact book UI plugins.

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
