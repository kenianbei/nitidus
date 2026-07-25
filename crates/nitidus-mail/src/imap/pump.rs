//! The coroutine pump: io-imap coroutines yield `WantsRead`/`WantsWrite`
//! and the pump moves bytes over the session's stream. Command errors
//! (NO/BAD/BYE) come back as `Command`; transport failures as `Io`, so
//! the session can tell what is retryable after a reconnect.

use io_imap::codec::fragmentizer::Fragmentizer;
use io_imap::coroutine::{ImapCoroutine, ImapCoroutineState, ImapYield};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::stream::ImapStream;

const READ_BUFFER_BYTES: usize = 16 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum PumpError {
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Command(String),
}

pub async fn run<C, T, E>(
    stream: &mut ImapStream,
    fragmentizer: &mut Fragmentizer,
    mut coroutine: C,
) -> Result<T, PumpError>
where
    C: ImapCoroutine<Yield = ImapYield, Return = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    let mut input: Option<usize> = None;
    loop {
        let arg = input.take().map(|n| &buffer[..n]);
        match coroutine.resume(fragmentizer, arg) {
            ImapCoroutineState::Yielded(ImapYield::WantsWrite(bytes)) => {
                stream
                    .write_all(&bytes)
                    .await
                    .map_err(|error| PumpError::Io(format!("write: {error}")))?;
                stream
                    .flush()
                    .await
                    .map_err(|error| PumpError::Io(format!("flush: {error}")))?;
            }
            ImapCoroutineState::Yielded(ImapYield::WantsRead) => {
                let n = stream
                    .read(&mut buffer)
                    .await
                    .map_err(|error| PumpError::Io(format!("read: {error}")))?;
                if n == 0 {
                    return Err(PumpError::Io("connection closed".to_owned()));
                }
                input = Some(n);
            }
            ImapCoroutineState::Complete(Ok(value)) => return Ok(value),
            ImapCoroutineState::Complete(Err(error)) => {
                return Err(PumpError::Command(error.to_string()));
            }
        }
    }
}
