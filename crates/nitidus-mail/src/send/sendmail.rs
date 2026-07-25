//! The sendmail-style pipe: recipients as arguments, the message on
//! stdin — works with sendmail and msmtp without `-t` configuration.

use tokio::io::AsyncWriteExt;

use super::SendEnvelope;
use crate::error::MailError;

pub(super) async fn transmit(
    command: &str,
    envelope: &SendEnvelope,
    message: Vec<u8>,
) -> Result<(), MailError> {
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{command} \"$@\""))
        .arg("sendmail")
        .args(&envelope.recipients)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| MailError::Backend(format!("spawning {command:?}: {error}")))?;

    let Some(mut stdin) = child.stdin.take() else {
        return Err(MailError::Backend("sendmail stdin unavailable".to_owned()));
    };
    stdin
        .write_all(&message)
        .await
        .map_err(|error| MailError::Backend(format!("writing to {command:?}: {error}")))?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .await
        .map_err(|error| MailError::Backend(format!("waiting on {command:?}: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(MailError::Backend(format!(
            "{command:?} exited with {}: {}",
            output.status,
            stderr.trim()
        )))
    }
}
