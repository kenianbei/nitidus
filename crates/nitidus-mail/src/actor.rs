//! One long-lived task per account. Commands process sequentially,
//! except during a streaming scan, where the actor keeps listening so
//! `Cancel` and `Shutdown` interrupt mid-stream; other commands queue
//! and run after the scan completes.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use tokio_util::sync::CancellationToken;

use crate::backend::MailBackend;
use crate::command::MailCommand;
use crate::error::MailError;
use crate::event::MailEvent;
use crate::types::{AccountId, ConnectionState, FolderId, JobId};

const SCAN_LOCAL_BUFFER: usize = 2;

pub async fn run_account_actor<B: MailBackend>(
    account: AccountId,
    mut backend: B,
    commands: flume::Receiver<MailCommand>,
    events: flume::Sender<MailEvent>,
) {
    send_connection(&events, &account, ConnectionState::Connected).await;
    let mut deferred = VecDeque::new();
    loop {
        let command = match deferred.pop_front() {
            Some(command) => command,
            None => match commands.recv_async().await {
                Ok(command) => command,
                Err(_) => break,
            },
        };
        if handle_command(
            &account,
            &mut backend,
            &commands,
            &events,
            &mut deferred,
            command,
        )
        .await
        .is_break()
        {
            break;
        }
    }
    send_connection(&events, &account, ConnectionState::Disconnected).await;
}

async fn handle_command<B: MailBackend>(
    account: &AccountId,
    backend: &mut B,
    commands: &flume::Receiver<MailCommand>,
    events: &flume::Sender<MailEvent>,
    deferred: &mut VecDeque<MailCommand>,
    command: MailCommand,
) -> ControlFlow<()> {
    match command {
        MailCommand::Shutdown => return ControlFlow::Break(()),
        MailCommand::Cancel(_) => {}
        MailCommand::ListFolders => send_folder_list(account, backend, events).await,
        MailCommand::CreateFolder { name } => {
            let result = backend.create_folder(&name).await;
            reply_folder_op(account, backend, events, result).await;
        }
        MailCommand::DeleteFolder { folder } => {
            let result = backend.delete_folder(&folder).await;
            reply_folder_op(account, backend, events, result).await;
        }
        MailCommand::RenameFolder { folder, new_name } => {
            let result = backend.rename_folder(&folder, &new_name).await;
            reply_folder_op(account, backend, events, result).await;
        }
        MailCommand::SyncEnvelopes { folder, job } => {
            return run_scan(account, backend, commands, events, deferred, folder, job).await;
        }
        MailCommand::FetchMessage { folder, id, job } => {
            let result = backend.fetch_message(&folder, &id).await;
            let event = match result {
                Ok(raw) => MailEvent::Message {
                    account: account.clone(),
                    folder,
                    id,
                    job,
                    raw,
                },
                Err(error) => job_failed(account, Some(job), error),
            };
            let _sent = events.send_async(event).await;
        }
        MailCommand::SetFlags { folder, id, flags } => {
            if let Err(error) = backend.set_flags(&folder, &id, flags).await {
                let _sent = events.send_async(job_failed(account, None, error)).await;
            }
        }
        MailCommand::AppendMessage {
            folder,
            bytes,
            flags,
        } => {
            if let Err(error) = backend.append_message(&folder, bytes, flags).await {
                let _sent = events.send_async(job_failed(account, None, error)).await;
            }
        }
    }
    ControlFlow::Continue(())
}

async fn send_folder_list<B: MailBackend>(
    account: &AccountId,
    backend: &mut B,
    events: &flume::Sender<MailEvent>,
) {
    let event = match backend.list_folders().await {
        Ok(folders) => MailEvent::Folders {
            account: account.clone(),
            folders,
        },
        Err(error) => job_failed(account, None, error),
    };
    let _sent = events.send_async(event).await;
}

/// A successful folder op answers with the refreshed folder list, so
/// every consumer converges through the one `Folders` event.
async fn reply_folder_op<B: MailBackend>(
    account: &AccountId,
    backend: &mut B,
    events: &flume::Sender<MailEvent>,
    result: Result<(), MailError>,
) {
    match result {
        Ok(()) => send_folder_list(account, backend, events).await,
        Err(error) => {
            let _sent = events.send_async(job_failed(account, None, error)).await;
        }
    }
}

async fn run_scan<B: MailBackend>(
    account: &AccountId,
    backend: &mut B,
    commands: &flume::Receiver<MailCommand>,
    events: &flume::Sender<MailEvent>,
    deferred: &mut VecDeque<MailCommand>,
    folder: FolderId,
    job: JobId,
) -> ControlFlow<()> {
    let token = CancellationToken::new();
    let (batch_tx, batch_rx) = flume::bounded(SCAN_LOCAL_BUFFER);
    let scan_folder = folder.clone();
    let scan = backend.scan_envelopes(&scan_folder, batch_tx);
    tokio::pin!(scan);
    let mut outcome = ControlFlow::Continue(());
    let result = loop {
        tokio::select! {
            result = &mut scan => {
                // A completed scan has dropped its sender; forward any
                // batches still sitting in the local buffer.
                forward_remaining(account, &folder, job, &batch_rx, events).await;
                break result;
            }
            batch = batch_rx.recv_async() => {
                if let Ok(batch) = batch {
                    let event = MailEvent::EnvelopeBatch {
                        account: account.clone(),
                        folder: folder.clone(),
                        job,
                        batch,
                        done: false,
                    };
                    let _sent = events.send_async(event).await;
                }
            }
            () = token.cancelled() => {
                drop(batch_rx);
                break scan.await;
            }
            command = commands.recv_async() => match command {
                Ok(MailCommand::Cancel(cancelled)) if cancelled == job => token.cancel(),
                Ok(MailCommand::Shutdown) => {
                    token.cancel();
                    outcome = ControlFlow::Break(());
                }
                Ok(other) => deferred.push_back(other),
                Err(_) => {
                    token.cancel();
                    outcome = ControlFlow::Break(());
                }
            }
        }
    };
    let event = match result {
        Ok(()) => MailEvent::EnvelopeBatch {
            account: account.clone(),
            folder,
            job,
            batch: Vec::new(),
            done: true,
        },
        Err(error) => job_failed(account, Some(job), error),
    };
    let _sent = events.send_async(event).await;
    outcome
}

async fn forward_remaining(
    account: &AccountId,
    folder: &FolderId,
    job: JobId,
    batch_rx: &flume::Receiver<Vec<crate::types::EnvelopeSummary>>,
    events: &flume::Sender<MailEvent>,
) {
    while let Ok(batch) = batch_rx.recv_async().await {
        let event = MailEvent::EnvelopeBatch {
            account: account.clone(),
            folder: folder.clone(),
            job,
            batch,
            done: false,
        };
        let _sent = events.send_async(event).await;
    }
}

fn job_failed(account: &AccountId, job: Option<JobId>, error: MailError) -> MailEvent {
    MailEvent::JobFailed {
        account: account.clone(),
        job,
        error,
    }
}

async fn send_connection(
    events: &flume::Sender<MailEvent>,
    account: &AccountId,
    state: ConnectionState,
) {
    let event = MailEvent::Connection {
        account: account.clone(),
        state,
    };
    let _sent = events.send_async(event).await;
}
