//! In-memory mail data the UI reads: folder lists and date-sorted
//! envelope lists per folder, reconciled with scans the same way the
//! disk cache is (job stamps, prune on scan completion).

use std::collections::{BTreeMap, HashMap, HashSet};

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeId, EnvelopeSummary, FolderId, FolderMeta, JobId};

/// Stamp for warm-loaded rows; any live scan's `done` prunes them.
const WARM_JOB: JobId = JobId(0);

#[derive(Resource, Default)]
pub struct MailStore {
    folders: BTreeMap<AccountId, Vec<FolderMeta>>,
    envelopes: HashMap<(AccountId, FolderId), FolderEnvelopes>,
}

impl MailStore {
    pub fn folders(&self, account: &AccountId) -> &[FolderMeta] {
        self.folders.get(account).map_or(&[], Vec::as_slice)
    }

    /// Date-descending; ties break on envelope id for a stable order.
    pub fn envelopes(&self, account: &AccountId, folder: &FolderId) -> &[EnvelopeSummary] {
        self.envelopes
            .get(&(account.clone(), folder.clone()))
            .map_or(&[], |cached| cached.sorted.as_slice())
    }

    pub fn set_folders(&mut self, account: AccountId, folders: Vec<FolderMeta>) {
        self.envelopes.retain(|(entry_account, folder), _| {
            entry_account != &account || folders.iter().any(|meta| &meta.id == folder)
        });
        self.folders.insert(account, folders);
    }

    pub fn hydrate(&mut self, account: AccountId, folder: FolderId, warm: Vec<EnvelopeSummary>) {
        self.entry(account, folder).upsert(WARM_JOB, warm, false);
    }

    pub fn apply_batch(
        &mut self,
        account: &AccountId,
        folder: &FolderId,
        job: JobId,
        batch: Vec<EnvelopeSummary>,
        done: bool,
    ) {
        self.entry(account.clone(), folder.clone())
            .upsert(job, batch, done);
    }

    fn entry(&mut self, account: AccountId, folder: FolderId) -> &mut FolderEnvelopes {
        self.envelopes.entry((account, folder)).or_default()
    }
}

#[derive(Default)]
struct FolderEnvelopes {
    sorted: Vec<EnvelopeSummary>,
    index: HashMap<EnvelopeId, usize>,
    stamps: HashMap<EnvelopeId, JobId>,
}

impl FolderEnvelopes {
    fn upsert(&mut self, job: JobId, batch: Vec<EnvelopeSummary>, done: bool) {
        for envelope in batch {
            self.stamps.insert(envelope.id.clone(), job);
            match self.index.get(&envelope.id) {
                Some(&position) => self.sorted[position] = envelope,
                None => self.sorted.push(envelope),
            }
        }
        if done {
            self.sorted
                .retain(|envelope| self.stamps.get(&envelope.id) == Some(&job));
            self.stamps.retain(|_, stamp| *stamp == job);
        }
        self.resort();
    }

    fn resort(&mut self) {
        self.sorted.sort_by(|a, b| {
            b.date_epoch_secs
                .cmp(&a.date_epoch_secs)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        self.index = self
            .sorted
            .iter()
            .enumerate()
            .map(|(position, envelope)| (envelope.id.clone(), position))
            .collect();
    }
}

/// Which folders have a scan in flight or completed this session —
/// the lazy-sync ledger. Folders outside it stay warm-cache-only until
/// first view.
#[derive(Resource, Default)]
pub struct SyncTracker {
    in_flight: HashMap<(AccountId, FolderId), JobId>,
    synced: HashSet<(AccountId, FolderId)>,
}

impl SyncTracker {
    pub fn begin(&mut self, account: AccountId, folder: FolderId, job: JobId) {
        self.in_flight.insert((account, folder), job);
    }

    /// Ignores completions of superseded jobs so a cancelled scan's
    /// stray `done` cannot mark the folder synced.
    pub fn finish(&mut self, account: &AccountId, folder: &FolderId, job: JobId) {
        let key = (account.clone(), folder.clone());
        if self.in_flight.get(&key) == Some(&job) {
            self.in_flight.remove(&key);
            self.synced.insert(key);
        }
    }

    pub fn fail(&mut self, job: JobId) {
        self.in_flight.retain(|_, in_flight| *in_flight != job);
    }

    pub fn in_flight_job(&self, account: &AccountId, folder: &FolderId) -> Option<JobId> {
        self.in_flight
            .get(&(account.clone(), folder.clone()))
            .copied()
    }

    pub fn is_tracked(&self, account: &AccountId, folder: &FolderId) -> bool {
        let key = (account.clone(), folder.clone());
        self.in_flight.contains_key(&key) || self.synced.contains(&key)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn envelope(id: &str, date: i64) -> EnvelopeSummary {
        EnvelopeSummary {
            id: EnvelopeId::new(id),
            subject: format!("subject {id}"),
            from_display: String::new(),
            from_addr: String::new(),
            date_epoch_secs: date,
            flags: Default::default(),
        }
    }

    fn ids<'a>(store: &'a MailStore, account: &AccountId, folder: &FolderId) -> Vec<&'a str> {
        store
            .envelopes(account, folder)
            .iter()
            .map(|e| e.id.as_str())
            .collect()
    }

    #[test]
    fn batches_accumulate_date_descending() {
        let mut store = MailStore::default();
        let (account, folder) = (AccountId::new("a"), FolderId::new("INBOX"));
        store.apply_batch(
            &account,
            &folder,
            JobId(1),
            vec![envelope("old", 100)],
            false,
        );
        store.apply_batch(&account, &folder, JobId(1), vec![envelope("new", 200)], true);
        assert_eq!(ids(&store, &account, &folder), vec!["new", "old"]);
    }

    #[test]
    fn rescan_prunes_unseen_and_updates_seen() {
        let mut store = MailStore::default();
        let (account, folder) = (AccountId::new("a"), FolderId::new("INBOX"));
        store.hydrate(
            account.clone(),
            folder.clone(),
            vec![envelope("stale", 100), envelope("kept", 200)],
        );
        let mut kept = envelope("kept", 200);
        kept.subject = "updated".to_owned();
        store.apply_batch(&account, &folder, JobId(7), vec![kept], true);
        assert_eq!(ids(&store, &account, &folder), vec!["kept"]);
        assert_eq!(store.envelopes(&account, &folder)[0].subject, "updated");
    }

    #[test]
    fn removed_folder_loses_its_envelopes() {
        let mut store = MailStore::default();
        let (account, folder) = (AccountId::new("a"), FolderId::new("Archive"));
        store.hydrate(account.clone(), folder.clone(), vec![envelope("m", 1)]);
        store.set_folders(
            account.clone(),
            vec![FolderMeta {
                id: FolderId::new("INBOX"),
                name: "INBOX".to_owned(),
                unread: 0,
                total: 0,
            }],
        );
        assert!(store.envelopes(&account, &folder).is_empty());
        assert_eq!(store.folders(&account).len(), 1);
    }

    #[test]
    fn superseded_job_completion_does_not_mark_synced() {
        let mut tracker = SyncTracker::default();
        let (account, folder) = (AccountId::new("a"), FolderId::new("INBOX"));
        tracker.begin(account.clone(), folder.clone(), JobId(1));
        tracker.begin(account.clone(), folder.clone(), JobId(2));
        tracker.finish(&account, &folder, JobId(1));
        assert_eq!(tracker.in_flight_job(&account, &folder), Some(JobId(2)));
        tracker.finish(&account, &folder, JobId(2));
        assert_eq!(tracker.in_flight_job(&account, &folder), None);
        assert!(tracker.is_tracked(&account, &folder));
    }

    #[test]
    fn failed_job_leaves_folder_untracked() {
        let mut tracker = SyncTracker::default();
        let (account, folder) = (AccountId::new("a"), FolderId::new("INBOX"));
        tracker.begin(account.clone(), folder.clone(), JobId(3));
        tracker.fail(JobId(3));
        assert!(!tracker.is_tracked(&account, &folder));
    }
}
