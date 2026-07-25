//! Address completion: contacts first, then addresses learned from
//! mail traffic. Senders are aggregated lazily from the in-memory
//! store (the envelope cache is already their history — no double
//! counting across rescans); send recipients persist in `mail.db` and
//! merge in memory as they happen. Ranking is nucleo fuzzy with
//! frecency as tiebreak.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use nitidus_contacts::ContactBook;
use nitidus_mail::cache::HarvestedAddress;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher, Utf32Str};

use crate::store::MailStore;

pub const COMPLETION_LIMIT: usize = 8;
const HALF_LIFE_DAYS: f64 = 30.0;
const SECONDS_PER_DAY: f64 = 86_400.0;

/// One completion candidate, ready to rank.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub formatted: String,
    pub is_contact: bool,
    pub frecency: f64,
}

#[derive(Resource, Default)]
pub struct AddressIndex {
    recipients: HashMap<String, HarvestedAddress>,
    senders: Vec<HarvestedAddress>,
    senders_fingerprint: Option<u64>,
}

impl AddressIndex {
    pub fn from_loaded(entries: Vec<HarvestedAddress>) -> Self {
        Self {
            recipients: entries
                .into_iter()
                .map(|entry| (entry.addr.to_lowercase(), entry))
                .collect(),
            senders: Vec::new(),
            senders_fingerprint: None,
        }
    }

    /// Same merge semantics as the cache table: uses accumulate, the
    /// newest sighting wins, a display name fills in once known.
    pub fn record_recipients(&mut self, entries: &[HarvestedAddress]) {
        for entry in entries {
            let merged = self
                .recipients
                .entry(entry.addr.to_lowercase())
                .or_insert_with(|| HarvestedAddress {
                    uses: 0,
                    ..entry.clone()
                });
            merged.uses += entry.uses;
            merged.last_seen_epoch = merged.last_seen_epoch.max(entry.last_seen_epoch);
            if merged.display.is_empty() && !entry.display.is_empty() {
                merged.display = entry.display.clone();
            }
        }
    }

    /// The full candidate set at this moment — snapshot it into a
    /// prompt's completion closure and rank per keystroke.
    pub fn candidates(
        &mut self,
        store: &MailStore,
        book: &ContactBook,
        now_epoch: i64,
    ) -> Vec<Candidate> {
        self.refresh_senders(store);
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        for contact in book.iter() {
            for addr in contact.emails() {
                seen.insert(addr.to_lowercase());
                candidates.push(Candidate {
                    formatted: format_entry(contact.display_name(), addr),
                    is_contact: true,
                    frecency: 0.0,
                });
            }
        }
        for entry in self.recipients.values().chain(self.senders.iter()) {
            if !seen.insert(entry.addr.to_lowercase()) {
                continue;
            }
            candidates.push(Candidate {
                formatted: format_entry(&entry.display, &entry.addr),
                is_contact: false,
                frecency: frecency(entry.uses, entry.last_seen_epoch, now_epoch),
            });
        }
        candidates
    }

    fn refresh_senders(&mut self, store: &MailStore) {
        let fingerprint = store.content_fingerprint();
        if self.senders_fingerprint == Some(fingerprint) {
            return;
        }
        let mut by_addr: HashMap<String, HarvestedAddress> = HashMap::new();
        for envelope in store.iter_envelopes() {
            if envelope.from_addr.is_empty() {
                continue;
            }
            let merged = by_addr
                .entry(envelope.from_addr.to_lowercase())
                .or_insert_with(|| HarvestedAddress {
                    addr: envelope.from_addr.clone(),
                    display: String::new(),
                    uses: 0,
                    last_seen_epoch: i64::MIN,
                });
            merged.uses += 1;
            merged.last_seen_epoch = merged.last_seen_epoch.max(envelope.date_epoch_secs);
            if merged.display.is_empty() && !envelope.from_display.is_empty() {
                merged.display = envelope.from_display.clone();
            }
        }
        self.senders = by_addr.into_values().collect();
        self.senders_fingerprint = Some(fingerprint);
    }
}

/// Snapshot for a prompt's completion closure: the full candidate set
/// right now (index may be absent in minimal embeddings — empty then).
pub fn snapshot_candidates(world: &mut World) -> Vec<Candidate> {
    let now_epoch = jiff::Timestamp::now().as_second();
    if world.get_resource::<AddressIndex>().is_none() {
        return Vec::new();
    }
    world.resource_scope(|world, mut index: Mut<AddressIndex>| {
        let empty = ContactBook::default();
        let store = world.resource::<MailStore>();
        let book = world
            .get_resource::<crate::contacts::ContactStore>()
            .map_or(&empty, |contacts| &contacts.0);
        index.candidates(store, book, now_epoch)
    })
}

/// Records the recipients of a sent message — in memory now, in the
/// cache for the next start.
pub fn harvest_recipients(world: &mut World, header_fields: &[&str]) {
    let now_epoch = jiff::Timestamp::now().as_second();
    let entries: Vec<HarvestedAddress> = header_fields
        .iter()
        .flat_map(|field| field.split(','))
        .filter_map(parse_address)
        .map(|(display, addr)| HarvestedAddress {
            addr,
            display,
            uses: 1,
            last_seen_epoch: now_epoch,
        })
        .collect();
    if entries.is_empty() {
        return;
    }
    if let Some(mut index) = world.get_resource_mut::<AddressIndex>() {
        index.record_recipients(&entries);
    }
    if let Some(cache) = world.get_resource::<crate::engine::CacheResource>() {
        cache.0.harvest(entries);
    }
}

/// Ranks a candidate snapshot for a query: contacts above harvested,
/// nucleo match score, then frecency. An empty query keeps that same
/// order without matching.
pub fn rank(candidates: &[Candidate], query: &str) -> Vec<String> {
    let mut scored: Vec<(&Candidate, u32)> = if query.is_empty() {
        candidates.iter().map(|candidate| (candidate, 0)).collect()
    } else {
        let mut matcher = Matcher::new(MatcherConfig::DEFAULT);
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut buffer = Vec::new();
        candidates
            .iter()
            .filter_map(|candidate| {
                let haystack = Utf32Str::new(&candidate.formatted, &mut buffer);
                pattern
                    .score(haystack, &mut matcher)
                    .map(|score| (candidate, score))
            })
            .collect()
    };
    scored.sort_by(|a, b| {
        b.0.is_contact
            .cmp(&a.0.is_contact)
            .then(b.1.cmp(&a.1))
            .then(b.0.frecency.total_cmp(&a.0.frecency))
            .then(a.0.formatted.cmp(&b.0.formatted))
    });
    scored
        .into_iter()
        .take(COMPLETION_LIMIT)
        .map(|(candidate, _)| candidate.formatted.clone())
        .collect()
}

/// `uses` decayed by the age of the last sighting.
fn frecency(uses: u32, last_seen_epoch: i64, now_epoch: i64) -> f64 {
    let age_days = ((now_epoch - last_seen_epoch).max(0) as f64) / SECONDS_PER_DAY;
    f64::from(uses) * 0.5_f64.powf(age_days / HALF_LIFE_DAYS)
}

pub fn format_entry(display: &str, addr: &str) -> String {
    if display.is_empty() {
        addr.to_owned()
    } else {
        format!("{display} <{addr}>")
    }
}

/// `Name <a@b>`, `<a@b>`, or `a@b` → (display, addr).
pub fn parse_address(segment: &str) -> Option<(String, String)> {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(open) = trimmed.rfind('<') {
        let addr = trimmed[open + 1..].trim_end_matches('>').trim();
        let display = trimmed[..open].trim().trim_matches('"');
        (!addr.is_empty()).then(|| (display.to_owned(), addr.to_owned()))
    } else {
        trimmed
            .contains('@')
            .then(|| (String::new(), trimmed.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn harvested(addr: &str, display: &str, uses: u32, last_seen: i64) -> HarvestedAddress {
        HarvestedAddress {
            addr: addr.to_owned(),
            display: display.to_owned(),
            uses,
            last_seen_epoch: last_seen,
        }
    }

    fn candidate(formatted: &str, is_contact: bool, frecency: f64) -> Candidate {
        Candidate {
            formatted: formatted.to_owned(),
            is_contact,
            frecency,
        }
    }

    #[test]
    fn contacts_outrank_harvested_regardless_of_frecency() {
        let candidates = [
            candidate("Zed Harvested <zed@x.example>", false, 99.0),
            candidate("Ada Contact <ada@x.example>", true, 0.0),
        ];
        let ranked = rank(&candidates, "x.example");
        assert_eq!(ranked[0], "Ada Contact <ada@x.example>");
    }

    #[test]
    fn fuzzy_matches_names_and_frecency_breaks_ties() {
        let candidates = [
            candidate("kold@example.com", false, 1.0),
            candidate("knew@example.com", false, 5.0),
            candidate("unrelated@other.example", false, 50.0),
        ];
        let ranked = rank(&candidates, "k");
        assert_eq!(ranked, ["knew@example.com", "kold@example.com"]);
    }

    #[test]
    fn frecency_decays_with_half_life() {
        let now = 100 * 86_400;
        let fresh = frecency(2, now, now);
        let month_old = frecency(2, now - 30 * 86_400, now);
        assert!((fresh - 2.0).abs() < f64::EPSILON);
        assert!((month_old - 1.0).abs() < 1e-9, "{month_old}");
    }

    #[test]
    fn recipient_merges_accumulate_and_fill_display() {
        let mut index = AddressIndex::from_loaded(vec![harvested("kj@x.example", "", 1, 100)]);
        index.record_recipients(&[harvested("KJ@x.example", "Katherine", 2, 50)]);
        let candidates = index.candidates(&MailStore::default(), &ContactBook::default(), 100);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].formatted, "Katherine <kj@x.example>");
    }

    #[test]
    fn parse_address_handles_all_shapes() {
        assert_eq!(
            parse_address("Ada Lovelace <ada@x.example>"),
            Some(("Ada Lovelace".to_owned(), "ada@x.example".to_owned()))
        );
        assert_eq!(
            parse_address(" ada@x.example "),
            Some((String::new(), "ada@x.example".to_owned()))
        );
        assert_eq!(
            parse_address("\"Lovelace, Ada\" <ada@x.example>"),
            Some(("Lovelace, Ada".to_owned(), "ada@x.example".to_owned()))
        );
        assert_eq!(parse_address("not an address"), None);
        assert_eq!(parse_address(""), None);
    }
}
