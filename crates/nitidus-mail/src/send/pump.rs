//! The SMTP coroutine pump — sibling of the IMAP one (io-smtp resumes
//! with plain byte slices, no Fragmentizer).

use io_smtp::coroutine::{SmtpCoroutine, SmtpCoroutineState, SmtpYield};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::MailError;
use crate::net::RemoteStream;

const READ_BUFFER_BYTES: usize = 16 * 1024;

pub(super) async fn run<C, T, E>(
    stream: &mut RemoteStream,
    mut coroutine: C,
) -> Result<T, MailError>
where
    C: SmtpCoroutine<Yield = SmtpYield, Return = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    let mut input: Option<usize> = None;
    loop {
        let arg = input.take().map(|n| &buffer[..n]);
        match coroutine.resume(arg) {
            SmtpCoroutineState::Yielded(SmtpYield::WantsWrite(bytes)) => {
                stream
                    .write_all(&bytes)
                    .await
                    .map_err(|error| MailError::Backend(format!("smtp write: {error}")))?;
                stream
                    .flush()
                    .await
                    .map_err(|error| MailError::Backend(format!("smtp flush: {error}")))?;
            }
            SmtpCoroutineState::Yielded(SmtpYield::WantsRead) => {
                let n = stream
                    .read(&mut buffer)
                    .await
                    .map_err(|error| MailError::Backend(format!("smtp read: {error}")))?;
                if n == 0 {
                    return Err(MailError::Backend("smtp connection closed".to_owned()));
                }
                input = Some(n);
            }
            SmtpCoroutineState::Complete(Ok(value)) => return Ok(value),
            SmtpCoroutineState::Complete(Err(error)) => {
                return Err(MailError::Backend(error.to_string()));
            }
        }
    }
}
