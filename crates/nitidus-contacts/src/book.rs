//! The in-memory collection: every contact, sorted by display name.
//! Low volume by design (see documentation/persistence.md §3) — no
//! query engine, rebuilt orderings on change.

use crate::contact::Contact;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ContactBook {
    contacts: Vec<Contact>,
}

impl ContactBook {
    pub fn from_contacts(mut contacts: Vec<Contact>) -> Self {
        contacts.sort_by_key(Contact::sort_key);
        Self { contacts }
    }

    pub fn len(&self) -> usize {
        self.contacts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Contact> {
        self.contacts.get(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Contact> {
        self.contacts.iter()
    }

    pub fn position_of(&self, uid: &str) -> Option<usize> {
        self.contacts
            .iter()
            .position(|contact| contact.uid() == uid)
    }

    /// Inserts or replaces by UID, re-sorts, and returns the contact's
    /// new position.
    pub fn upsert(&mut self, contact: Contact) -> usize {
        let uid = contact.uid().to_owned();
        match self.position_of(&uid) {
            Some(position) => self.contacts[position] = contact,
            None => self.contacts.push(contact),
        }
        self.contacts.sort_by_key(Contact::sort_key);
        self.position_of(&uid).unwrap_or(0)
    }

    pub fn remove(&mut self, index: usize) -> Option<Contact> {
        (index < self.contacts.len()).then(|| self.contacts.remove(index))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn contacts_sort_by_display_name_case_insensitively() {
        let book = ContactBook::from_contacts(vec![
            Contact::new("zoe"),
            Contact::new("Ada"),
            Contact::new("mel"),
        ]);
        let names: Vec<&str> = book.iter().map(Contact::display_name).collect();
        assert_eq!(names, ["Ada", "mel", "zoe"]);
    }

    #[test]
    fn upsert_replaces_by_uid_and_returns_new_position() {
        let mut book = ContactBook::from_contacts(vec![Contact::new("Ada"), Contact::new("Zoe")]);
        let mut renamed = book.get(0).unwrap().clone();
        let fn_index = renamed.entry_indices()[1];
        renamed.edit_entry(fn_index, "Zz Renamed").unwrap();
        let position = book.upsert(renamed);
        assert_eq!(position, 1, "renamed contact must re-sort to the end");
        assert_eq!(book.len(), 2, "upsert by uid must not duplicate");
    }

    #[test]
    fn upsert_inserts_unknown_uid() {
        let mut book = ContactBook::default();
        let position = book.upsert(Contact::new("Solo"));
        assert_eq!(position, 0);
        assert_eq!(book.len(), 1);
    }

    #[test]
    fn remove_out_of_range_is_none() {
        let mut book = ContactBook::default();
        assert!(book.remove(0).is_none());
    }
}
