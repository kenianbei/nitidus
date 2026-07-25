//! The engine owns the tokio runtime, the channels, and the account
//! registry. It is the only type the UI layer touches.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::actor::run_account_actor;
use crate::backend::MailBackend;
use crate::command::MailCommand;
use crate::error::MailError;
use crate::event::MailEvent;
use crate::types::{AccountId, FolderId, JobId};

const COMMAND_CHANNEL_CAPACITY: usize = 256;
const EVENT_CHANNEL_CAPACITY: usize = 1024;
const MIN_WORKER_THREADS: usize = 2;
const MAX_WORKER_THREADS: usize = 4;

pub struct MailEngine {
    runtime: tokio::runtime::Runtime,
    events_tx: flume::Sender<MailEvent>,
    events_rx: flume::Receiver<MailEvent>,
    accounts: HashMap<AccountId, flume::Sender<MailCommand>>,
    watchers: HashMap<AccountId, Vec<tokio::task::JoinHandle<()>>>,
    next_job: AtomicU64,
}

impl MailEngine {
    /// Worker threads scale with the expected account count, one extra
    /// for shared work, clamped to a small pool.
    pub fn new(account_hint: usize) -> Result<Self, MailError> {
        let workers = (account_hint + 1).clamp(MIN_WORKER_THREADS, MAX_WORKER_THREADS);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .thread_name("nitidus-mail")
            .enable_time()
            .enable_io()
            .build()?;
        let (events_tx, events_rx) = flume::bounded(EVENT_CHANNEL_CAPACITY);
        Ok(Self {
            runtime,
            events_tx,
            events_rx,
            accounts: HashMap::new(),
            watchers: HashMap::new(),
            next_job: AtomicU64::new(1),
        })
    }

    pub fn add_account<B: MailBackend>(&mut self, id: AccountId, backend: B) {
        let (commands_tx, commands_rx) = flume::bounded(COMMAND_CHANNEL_CAPACITY);
        self.runtime.spawn(run_account_actor(
            id.clone(),
            backend,
            commands_rx,
            self.events_tx.clone(),
        ));
        self.accounts.insert(id, commands_tx);
    }

    /// Tears the account down: the actor ends when its command
    /// channel closes, and its watchers are aborted. Returns whether
    /// the account existed.
    pub fn remove_account(&mut self, id: &AccountId) -> bool {
        for watcher in self.watchers.remove(id).unwrap_or_default() {
            watcher.abort();
        }
        self.accounts.remove(id).is_some()
    }

    pub fn has_account(&self, id: &AccountId) -> bool {
        self.accounts.contains_key(id)
    }

    pub(crate) fn track_watcher(&mut self, id: AccountId, handle: tokio::task::JoinHandle<()>) {
        self.watchers.entry(id).or_default().push(handle);
    }

    pub fn accounts(&self) -> impl Iterator<Item = &AccountId> {
        self.accounts.keys()
    }

    pub fn send(&self, account: &AccountId, command: MailCommand) -> Result<(), MailError> {
        let sender = self
            .accounts
            .get(account)
            .ok_or_else(|| MailError::UnknownAccount(account.to_string()))?;
        sender
            .send(command)
            .map_err(|_disconnected| MailError::ChannelClosed)
    }

    pub fn try_recv_event(&self) -> Option<MailEvent> {
        self.events_rx.try_recv().ok()
    }

    pub fn next_job(&self) -> JobId {
        JobId(self.next_job.fetch_add(1, Ordering::Relaxed))
    }

    /// Transmits one message on the mail runtime; `SendDone` on
    /// success, `JobFailed` on error. No cancellation — once submitted
    /// the undo window is over.
    pub fn submit(
        &self,
        account: AccountId,
        transport: crate::send::OutgoingTransport,
        envelope: crate::send::SendEnvelope,
        message: Vec<u8>,
        job: JobId,
    ) {
        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let event = match crate::send::transmit(&transport, &envelope, message).await {
                Ok(()) => MailEvent::SendDone { account, job },
                Err(error) => MailEvent::JobFailed {
                    account,
                    job: Some(job),
                    error,
                },
            };
            let _sent = events.send_async(event).await;
        });
    }

    /// Runs the pure JWZ computation off-thread over a snapshot and
    /// emits `MailEvent::Threads`. Superseded jobs need no cancellation
    /// — consumers keep only the newest job's rows.
    pub fn compute_threads(
        &self,
        account: AccountId,
        folder: FolderId,
        envelopes: Vec<crate::types::EnvelopeSummary>,
        job: JobId,
    ) {
        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let rows =
                tokio::task::spawn_blocking(move || crate::thread::compute_thread_rows(&envelopes))
                    .await
                    .unwrap_or_default();
            let event = MailEvent::Threads {
                account,
                folder,
                job,
                rows,
            };
            let _sent = events.send_async(event).await;
        });
    }

    pub(crate) fn events_sender(&self) -> flume::Sender<MailEvent> {
        self.events_tx.clone()
    }

    /// Handle for app-side tasks that must run on the mail runtime
    /// (interactive OAuth grants).
    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }
}

impl Drop for MailEngine {
    fn drop(&mut self) {
        for sender in self.accounts.values() {
            let _requested = sender.try_send(MailCommand::Shutdown);
        }
        self.accounts.clear();
    }
}
