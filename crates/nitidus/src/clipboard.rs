//! System clipboard access, with an OSC 52 fallback.
//!
//! `arboard` needs a display server, which a session over SSH or on a
//! bare tty does not have. OSC 52 writes through the terminal itself and
//! works in both, but is write-only and not universally supported — so
//! copying tries the real clipboard first and escapes out to OSC 52,
//! while reading is clipboard-only.

use std::io::Write;

use base64::Engine as _;

const OSC52_PREFIX: &str = "\x1b]52;c;";
const OSC52_SUFFIX: &str = "\x07";

/// Puts `text` on the system clipboard.
pub fn set(text: &str) -> anyhow::Result<()> {
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
        Ok(()) => Ok(()),
        Err(error) => write_osc52(text).map_err(|fallback| {
            anyhow::anyhow!("clipboard unavailable ({error}), and OSC 52 failed: {fallback}")
        }),
    }
}

/// Reads the system clipboard, or `None` when there is no clipboard to
/// read — OSC 52 cannot report back, so callers fall back to their own
/// buffer.
pub fn get() -> Option<String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .ok()
}

/// Writes the terminal's own copy sequence to stdout. The payload is
/// base64 per the OSC 52 spec.
fn write_osc52(text: &str) -> std::io::Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut stdout = std::io::stdout().lock();
    write!(stdout, "{OSC52_PREFIX}{encoded}{OSC52_SUFFIX}")?;
    stdout.flush()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn osc52_payload_is_base64_between_the_sequence_markers() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("hello");
        let sequence = format!("{OSC52_PREFIX}{encoded}{OSC52_SUFFIX}");

        assert!(sequence.starts_with("\x1b]52;c;"));
        assert!(sequence.ends_with('\x07'));
        assert!(
            sequence.contains("aGVsbG8="),
            "payload must be base64: {sequence:?}"
        );
    }
}
