//! Value types crossing the engine boundary. Identifier newtypes are
//! cheap-clone (`Arc<str>`) because every event carries them.

use std::fmt;
use std::sync::Arc;

macro_rules! arc_str_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(Arc<str>);

        impl $name {
            pub fn new(value: impl AsRef<str>) -> Self {
                Self(Arc::from(value.as_ref()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

arc_str_id!(AccountId);
arc_str_id!(FolderId);
arc_str_id!(EnvelopeId);

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct JobId(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags(u8);

impl Flags {
    pub const SEEN: Flags = Flags(1);
    pub const ANSWERED: Flags = Flags(1 << 1);
    pub const FLAGGED: Flags = Flags(1 << 2);
    pub const DELETED: Flags = Flags(1 << 3);
    pub const DRAFT: Flags = Flags(1 << 4);

    const KNOWN: u8 = 0b1_1111;

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn from_bits(bits: u8) -> Flags {
        Flags(bits & Self::KNOWN)
    }

    pub fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn with(self, other: Flags) -> Flags {
        Flags(self.0 | other.0)
    }

    pub fn without(self, other: Flags) -> Flags {
        Flags(self.0 & !other.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderMeta {
    pub id: FolderId,
    pub name: String,
    pub unread: u32,
    pub total: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeSummary {
    pub id: EnvelopeId,
    pub subject: String,
    pub from_display: String,
    pub from_addr: String,
    pub date_epoch_secs: i64,
    pub flags: Flags,
    /// RFC 5322 Message-ID (without angle brackets); empty when absent.
    pub message_id: String,
    /// `References` chain oldest-first, with `In-Reply-To` as the sole
    /// entry when `References` is absent.
    pub references: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Failed,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn flags_compose_and_query() {
        let flags = Flags::default().with(Flags::SEEN).with(Flags::FLAGGED);
        assert!(flags.contains(Flags::SEEN));
        assert!(flags.contains(Flags::FLAGGED));
        assert!(!flags.contains(Flags::DELETED));
        assert!(!flags.without(Flags::SEEN).contains(Flags::SEEN));
    }

    #[test]
    fn ids_clone_cheaply_and_compare() {
        let a = AccountId::new("work");
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "work");
        assert_eq!(a.to_string(), "work");
    }
}
