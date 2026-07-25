//! Engine-level IMAP push on a dedicated connection. Servers with
//! QRESYNC get io-imap's `ImapMailboxWatch` (IDLE + delta reselect);
//! everything else (Gmail advertises CONDSTORE but not QRESYNC) falls
//! back to plain RFC 2177 IDLE. Both paths reduce every wake to a
//! `FolderChanged { INBOX }`, and the app's normal re-scan — already
//! incremental — does the rest. A read timeout bounds silently dead
//! connections; the task reconnects with a capped backoff and ends when
//! the event channel closes.

use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use io_imap::coroutine::{ImapCoroutine, ImapCoroutineState};
use io_imap::rfc2177::idle::{ImapIdle, ImapIdleOptions, ImapIdleYield};
use io_imap::rfc3501::select::{ImapMailboxSelect, ImapMailboxSelectOptions};
use io_imap::types::response::Capability;
use io_imap::watch::{ImapMailboxWatch, ImapMailboxWatchYield};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

use super::session::{self, Connection, parse_mailbox};
use super::{INBOX, ImapConfig};
use crate::engine::MailEngine;
use crate::event::MailEvent;
use crate::types::{AccountId, FolderId};

const INITIAL_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// A run that lasted this long was healthy; its failure restarts the
/// backoff ladder from the bottom.
const HEALTHY_RUN: Duration = Duration::from_secs(60);
/// Re-establish IDLE when nothing arrives for this long — servers and
/// NATs silently drop long-idle connections.
const IDLE_READ_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const READ_BUFFER_BYTES: usize = 16 * 1024;

impl MailEngine {
    pub fn watch_imap(&mut self, account: AccountId, config: ImapConfig) {
        let events = self.events_sender();
        let id = account.clone();
        let handle = self.runtime_handle().spawn(async move {
            let mut backoff = INITIAL_BACKOFF;
            loop {
                let started = Instant::now();
                match run_watch(&account, &config, &events).await {
                    WatchEnd::ChannelClosed => return,
                    WatchEnd::Failed(error) => {
                        tracing::warn!("imap watch for {account} ended: {error}; retrying");
                    }
                }
                if started.elapsed() >= HEALTHY_RUN {
                    backoff = INITIAL_BACKOFF;
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        });
        self.track_watcher(id, handle);
    }
}

enum WatchEnd {
    ChannelClosed,
    Failed(String),
}

async fn run_watch(
    account: &AccountId,
    config: &ImapConfig,
    events: &flume::Sender<MailEvent>,
) -> WatchEnd {
    let mut connection = match session::connect(config).await {
        Ok(connection) => connection,
        Err(error) => return WatchEnd::Failed(error.to_string()),
    };
    let has_qresync = connection
        .capabilities
        .iter()
        .any(|capability| matches!(capability, Capability::QResync));
    if has_qresync {
        qresync_watch(account, &mut connection, events).await
    } else {
        idle_watch(account, &mut connection, events).await
    }
}

async fn qresync_watch(
    account: &AccountId,
    connection: &mut Connection,
    events: &flume::Sender<MailEvent>,
) -> WatchEnd {
    let mailbox = match parse_mailbox(INBOX) {
        Ok(mailbox) => mailbox,
        Err(error) => return WatchEnd::Failed(error.to_string()),
    };
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut watch =
        match ImapMailboxWatch::new(&connection.capabilities, mailbox, Arc::clone(&shutdown)) {
            Ok(watch) => watch,
            Err(error) => return WatchEnd::Failed(error.to_string()),
        };
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    let mut input: Option<usize> = None;
    loop {
        let arg = input.take().map(|n| &buffer[..n]);
        let step = match watch.resume(&mut connection.fragmentizer, arg) {
            ImapCoroutineState::Yielded(ImapMailboxWatchYield::WantsWrite(bytes)) => {
                WatchStep::Write(bytes)
            }
            ImapCoroutineState::Yielded(ImapMailboxWatchYield::WantsRead) => WatchStep::Read,
            ImapCoroutineState::Yielded(ImapMailboxWatchYield::Event(_)) => WatchStep::Changed,
            ImapCoroutineState::Complete(Ok(())) => return WatchEnd::ChannelClosed,
            ImapCoroutineState::Complete(Err(error)) => {
                return WatchEnd::Failed(error.to_string());
            }
        };
        match drive(step, connection, events, account, &shutdown, &mut buffer).await {
            Ok(read) => input = read,
            Err(end) => return end,
        }
    }
}

/// Plain IDLE: SELECT INBOX, IDLE, and treat every server event as a
/// change signal.
async fn idle_watch(
    account: &AccountId,
    connection: &mut Connection,
    events: &flume::Sender<MailEvent>,
) -> WatchEnd {
    let mailbox = match parse_mailbox(INBOX) {
        Ok(mailbox) => mailbox,
        Err(error) => return WatchEnd::Failed(error.to_string()),
    };
    let select = ImapMailboxSelect::new(mailbox, ImapMailboxSelectOptions::default());
    let selected = super::pump::run(&mut connection.stream, &mut connection.fragmentizer, select);
    if let Err(error) = selected.await {
        return WatchEnd::Failed(format!("select for idle: {error}"));
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let mut idle = ImapIdle::new(Arc::clone(&shutdown), ImapIdleOptions::default());
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    let mut input: Option<usize> = None;
    loop {
        let arg = input.take().map(|n| &buffer[..n]);
        let step = match idle.resume(&mut connection.fragmentizer, arg) {
            ImapCoroutineState::Yielded(ImapIdleYield::WantsWrite(bytes)) => {
                WatchStep::Write(bytes)
            }
            ImapCoroutineState::Yielded(ImapIdleYield::WantsRead) => WatchStep::Read,
            ImapCoroutineState::Yielded(ImapIdleYield::Event(_)) => WatchStep::Changed,
            ImapCoroutineState::Complete(Ok(())) => return WatchEnd::ChannelClosed,
            ImapCoroutineState::Complete(Err(error)) => {
                return WatchEnd::Failed(error.to_string());
            }
        };
        match drive(step, connection, events, account, &shutdown, &mut buffer).await {
            Ok(read) => input = read,
            Err(end) => return end,
        }
    }
}

enum WatchStep {
    Write(Vec<u8>),
    Read,
    Changed,
}

/// Executes one yielded step; `Ok(Some(n))` carries bytes read back to
/// the coroutine.
async fn drive(
    step: WatchStep,
    connection: &mut Connection,
    events: &flume::Sender<MailEvent>,
    account: &AccountId,
    shutdown: &AtomicBool,
    buffer: &mut [u8],
) -> Result<Option<usize>, WatchEnd> {
    match step {
        WatchStep::Write(bytes) => {
            write_all(connection, &bytes)
                .await
                .map_err(WatchEnd::Failed)?;
            Ok(None)
        }
        WatchStep::Read => {
            let read = tokio::time::timeout(IDLE_READ_TIMEOUT, connection.stream.read(buffer));
            match read.await {
                Err(_elapsed) => Err(WatchEnd::Failed("idle refresh window elapsed".to_owned())),
                Ok(Ok(0)) => Err(WatchEnd::Failed("connection closed".to_owned())),
                Ok(Ok(n)) => Ok(Some(n)),
                Ok(Err(error)) => Err(WatchEnd::Failed(format!("read: {error}"))),
            }
        }
        WatchStep::Changed => {
            let changed = MailEvent::FolderChanged {
                account: account.clone(),
                folder: FolderId::new(INBOX),
            };
            if events.send_async(changed).await.is_err() {
                shutdown.store(true, Ordering::Relaxed);
                return Err(WatchEnd::ChannelClosed);
            }
            Ok(None)
        }
    }
}

async fn write_all(connection: &mut Connection, bytes: &[u8]) -> Result<(), String> {
    connection
        .stream
        .write_all(bytes)
        .await
        .map_err(|error| format!("write: {error}"))?;
    connection
        .stream
        .flush()
        .await
        .map_err(|error| format!("flush: {error}"))
}
