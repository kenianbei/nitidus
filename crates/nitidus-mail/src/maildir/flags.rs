//! Translation between our five-bit `Flags` and `io-maildir`'s
//! `MaildirFlags`. `Passed` and keywords have no representation on our
//! side yet, so they are dropped on the way in — roadmap item 34
//! replaces `Flags` with a type that can carry them.

use io_maildir::flag::{MaildirFlag, MaildirFlags};

use crate::types::Flags;

const PAIRS: [(Flags, MaildirFlag); 5] = [
    (Flags::SEEN, MaildirFlag::Seen),
    (Flags::ANSWERED, MaildirFlag::Replied),
    (Flags::FLAGGED, MaildirFlag::Flagged),
    (Flags::DELETED, MaildirFlag::Trashed),
    (Flags::DRAFT, MaildirFlag::Draft),
];

pub fn to_maildir(flags: Flags) -> MaildirFlags {
    let mut out = MaildirFlags::default();
    for (ours, theirs) in PAIRS {
        if flags.contains(ours) {
            out.insert(theirs.clone());
        }
    }
    out
}

pub fn from_maildir(flags: &MaildirFlags) -> Flags {
    let mut out = Flags::default();
    for (ours, theirs) in PAIRS {
        if flags.contains(&theirs) {
            out = out.with(ours);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn round_trips_every_flag_we_model() {
        for (ours, _) in PAIRS {
            assert_eq!(from_maildir(&to_maildir(ours)), ours);
        }
        let combined = Flags::SEEN.with(Flags::DRAFT).with(Flags::ANSWERED);
        assert_eq!(from_maildir(&to_maildir(combined)), combined);
    }

    #[test]
    fn drops_flags_we_cannot_represent() {
        let mut theirs = MaildirFlags::default();
        theirs.insert(MaildirFlag::Passed);
        theirs.insert(MaildirFlag::keyword("todo"));
        theirs.insert(MaildirFlag::Seen);
        assert_eq!(
            from_maildir(&theirs),
            Flags::SEEN,
            "Passed and keywords have no bit until item 34"
        );
    }

    #[test]
    fn suffix_follows_upstream_ordering() {
        let flags = Flags::default()
            .with(Flags::SEEN)
            .with(Flags::DRAFT)
            .with(Flags::ANSWERED);
        assert_eq!(
            to_maildir(flags).to_string(),
            "RSD",
            "upstream emits enum-declaration order, not ASCII order (finding 1)"
        );
    }
}
