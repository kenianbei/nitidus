//! Contact management for nitidus: the vCard domain model and vdir
//! persistence. Kept free of bevy — the contact tab UI lives in the
//! nitidus bin crate and drives this through plain calls.

pub mod book;
pub mod contact;
pub mod store;

pub use calcard;

pub use book::ContactBook;
pub use contact::{Contact, ContactError, PhotoSource, escape_component};
pub use store::{LoadIssue, StoreError, delete_contact, load_dir, save_contact};
