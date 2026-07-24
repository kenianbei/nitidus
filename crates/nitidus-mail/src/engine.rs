//! The engine owns the tokio runtime, the channels, and the account
//! registry. It is the only type the UI layer touches.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::actor::run_account_actor;
use crate::backend::MailBackend;
use crate::command::MailCommand;
use crate::error::MailError;
use crate::event::MailEvent;
use crate::types::{AccountId, JobId};

const COMMAND_CHANNEL_CAPACITY: usize = 256;
const EVENT_CHANNEL_CAPACITY: usize = 1024;
const MIN_WORKER_THREADS: usize = 2;
const MAX_WORKER_THREADS: usize = 4;

pub struct MailEngine {
    runtime: tokio::runtime::Runtime,
    events_tx: flume::Sender<MailEvent>,
    events_rx: flume::Receiver<MailEvent>,
    accounts: HashMap<AccountId, flume::Sender<MailCommand>>,
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
            .build()?;
        let (events_tx, events_rx) = flume::bounded(EVENT_CHANNEL_CAPACITY);
        Ok(Self {
            runtime,
            events_tx,
            events_rx,
            accounts: HashMap::new(),
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
}

impl Drop for MailEngine {
    fn drop(&mut self) {
        for sender in self.accounts.values() {
            let _requested = sender.try_send(MailCommand::Shutdown);
        }
        self.accounts.clear();
    }
}
