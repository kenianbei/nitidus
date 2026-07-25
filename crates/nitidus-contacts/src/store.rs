//! Vdir persistence: one `.vcf` per contact, UID as filename, atomic
//! writes (documentation/persistence.md §3). Loading is lenient — a
//! malformed file becomes a reported issue, never a failure — and a
//! foreign file whose name is not its UID keeps its filename on save.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::contact::{Contact, ContactError};

pub const VCF_EXTENSION: &str = "vcf";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("contact store I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Contact(#[from] ContactError),
}

/// A file that could not be loaded: reported, skipped, left on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadIssue {
    pub file: String,
    pub problem: String,
}

pub fn load_dir(dir: &Path) -> Result<(Vec<Contact>, Vec<LoadIssue>), StoreError> {
    if !dir.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == VCF_EXTENSION))
        .collect();
    paths.sort();
    let mut contacts = Vec::new();
    let mut issues = Vec::new();
    for path in paths {
        match load_file(&path) {
            Ok(contact) => contacts.push(contact),
            Err(problem) => issues.push(LoadIssue {
                file: file_name(&path),
                problem,
            }),
        }
    }
    Ok((contacts, issues))
}

fn load_file(path: &Path) -> Result<Contact, String> {
    let input = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut contact = Contact::from_vcf(&input).map_err(|error| error.to_string())?;
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned());
    if let Some(stem) = stem
        && stem != contact.uid()
    {
        contact.source_stem = Some(stem);
    }
    Ok(contact)
}

/// Atomic write-via-rename: readers never observe a half-written card.
pub fn save_contact(dir: &Path, contact: &Contact) -> Result<PathBuf, StoreError> {
    std::fs::create_dir_all(dir)?;
    let target = dir.join(format!("{}.{VCF_EXTENSION}", file_stem(contact)));
    let temporary = tempfile::NamedTempFile::new_in(dir)?;
    std::fs::write(temporary.path(), contact.to_vcf())?;
    temporary
        .persist(&target)
        .map_err(|error| StoreError::Io(error.error))?;
    Ok(target)
}

pub fn delete_contact(dir: &Path, contact: &Contact) -> Result<(), StoreError> {
    let target = dir.join(format!("{}.{VCF_EXTENSION}", file_stem(contact)));
    std::fs::remove_file(target)?;
    Ok(())
}

fn file_stem(contact: &Contact) -> String {
    let stem = contact
        .source_stem
        .clone()
        .unwrap_or_else(|| contact.uid().to_owned());
    sanitize(&stem)
}

/// Foreign UIDs can contain anything; the filename must not.
fn sanitize(stem: &str) -> String {
    stem.chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '@' | '-' => character,
            _ => '_',
        })
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn missing_directory_loads_empty() {
        let root = tempfile::tempdir().unwrap();
        let (contacts, issues) = load_dir(&root.path().join("absent")).unwrap();
        assert!(contacts.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn save_then_load_round_trips_under_uid_filename() {
        let root = tempfile::tempdir().unwrap();
        let contact = Contact::new("Ada Lovelace");
        let path = save_contact(root.path(), &contact).unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            format!("{}.vcf", contact.uid())
        );
        let (contacts, issues) = load_dir(root.path()).unwrap();
        assert!(issues.is_empty());
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].display_name(), "Ada Lovelace");
        assert_eq!(contacts[0].uid(), contact.uid());
    }

    #[test]
    fn malformed_file_becomes_issue_not_failure() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("junk.vcf"), "not a vcard").unwrap();
        save_contact(root.path(), &Contact::new("Good")).unwrap();
        let (contacts, issues) = load_dir(root.path()).unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].file, "junk.vcf");
    }

    #[test]
    fn foreign_filename_is_kept_on_save() {
        let root = tempfile::tempdir().unwrap();
        let foreign = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:abc-123\r\nFN:Ada\r\nEND:VCARD\r\n";
        std::fs::write(root.path().join("from-khard.vcf"), foreign).unwrap();
        let (mut contacts, _) = load_dir(root.path()).unwrap();
        let mut contact = contacts.remove(0);
        let fn_index = contact
            .entry_indices()
            .into_iter()
            .find(|&index| {
                contact.entry_at(index).unwrap().name == calcard::vcard::VCardProperty::Fn
            })
            .unwrap();
        contact.edit_entry(fn_index, "Ada Lovelace").unwrap();
        let path = save_contact(root.path(), &contact).unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "from-khard.vcf"
        );
        let listing: Vec<String> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(listing, ["from-khard.vcf"], "no orphan uid-named copy");
    }

    #[test]
    fn delete_removes_the_backing_file() {
        let root = tempfile::tempdir().unwrap();
        let contact = Contact::new("Gone");
        save_contact(root.path(), &contact).unwrap();
        delete_contact(root.path(), &contact).unwrap();
        let (contacts, _) = load_dir(root.path()).unwrap();
        assert!(contacts.is_empty());
    }

    #[test]
    fn nonstandard_uid_characters_are_sanitized_in_filenames() {
        let root = tempfile::tempdir().unwrap();
        let weird = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid/9?b\r\nFN:W\r\nEND:VCARD\r\n";
        let contact = Contact::from_vcf(weird).unwrap();
        let path = save_contact(root.path(), &contact).unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "urn_uuid_9_b.vcf"
        );
    }
}
