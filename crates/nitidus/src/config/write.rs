//! Config-file mutation for the account wizard: append, update, or
//! remove one `[[accounts]]` block in `config.toml`, leaving
//! everything the user hand-wrote — comments included — untouched.
//! New blocks are appended as serialized text (toml_edit's structured
//! insert scrambles sub-table ordering); removal is structured via
//! toml_edit. Every mutation re-parses and validates the result
//! before it touches disk, and the write is atomic (tmp + rename).

use std::path::Path;

use anyhow::{Context, bail};
use toml_edit::{DocumentMut, Item};

use super::account::AccountConfig;
use super::schema::Config;

const ACCOUNTS_KEY: &str = "accounts";
const WIZARD_MARKER: &str = "# added by :new-account";

pub fn append_account(config_path: &Path, account: &AccountConfig) -> anyhow::Result<()> {
    let existing = read_or_empty(config_path)?;
    write_validated(config_path, appended(&existing, account)?)
}

/// Replaces the block `original_name` points at (the account may have
/// been renamed): structured removal, then re-append — the edited
/// account moves to the end of the file.
pub fn update_account(
    config_path: &Path,
    original_name: &str,
    account: &AccountConfig,
) -> anyhow::Result<()> {
    let without = removed(&read_or_empty(config_path)?, original_name, config_path)?;
    write_validated(config_path, appended(&without, account)?)
}

pub fn remove_account(config_path: &Path, name: &str) -> anyhow::Result<()> {
    let without = removed(&read_or_empty(config_path)?, name, config_path)?;
    write_validated(config_path, without)
}

fn read_or_empty(config_path: &Path) -> anyhow::Result<String> {
    match std::fs::read_to_string(config_path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("reading {}", config_path.display())),
    }
}

/// The account serialized through the real config serializer, appended
/// as text — guaranteed to say exactly what the parser will read.
fn appended(existing: &str, account: &AccountConfig) -> anyhow::Result<String> {
    #[derive(serde::Serialize)]
    struct AccountsOnly<'a> {
        accounts: [&'a AccountConfig; 1],
    }
    let wrapper = AccountsOnly {
        accounts: [account],
    };
    let block = toml::to_string(&wrapper).context("serializing the account")?;
    let separator = if existing.is_empty() || existing.ends_with("\n\n") {
        ""
    } else if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Ok(format!("{existing}{separator}{WIZARD_MARKER}\n{block}"))
}

fn removed(existing: &str, name: &str, config_path: &Path) -> anyhow::Result<String> {
    let mut document: DocumentMut = existing
        .parse()
        .with_context(|| format!("parsing {}", config_path.display()))?;
    let Some(Item::ArrayOfTables(accounts)) = document.get_mut(ACCOUNTS_KEY) else {
        bail!("no account named {name:?} in the config file");
    };
    let position = accounts
        .iter()
        .position(|table| {
            table
                .get("name")
                .and_then(|item| item.as_str())
                .is_some_and(|candidate| candidate == name)
        })
        .with_context(|| format!("no account named {name:?} in the config file"))?;
    accounts.remove(position);
    Ok(document.to_string())
}

fn write_validated(config_path: &Path, rendered: String) -> anyhow::Result<()> {
    let reparsed: Config = toml::from_str(&rendered).context("mutated config does not parse")?;
    super::load::validate(&reparsed).context("mutated config does not validate")?;
    let temp_path = config_path.with_extension("toml.tmp");
    let directory = config_path.parent().context("config path has no parent")?;
    std::fs::create_dir_all(directory)
        .with_context(|| format!("creating {}", directory.display()))?;
    std::fs::write(&temp_path, rendered)
        .with_context(|| format!("writing {}", temp_path.display()))?;
    std::fs::rename(&temp_path, config_path)
        .with_context(|| format!("replacing {}", config_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn sample(name: &str) -> AccountConfig {
        AccountConfig {
            name: name.to_owned(),
            email: format!("{name}@example.com"),
            ..Default::default()
        }
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn append_creates_the_file_and_parses_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        append_account(&path, &sample("first")).unwrap();
        let config: Config = toml::from_str(&read(&path)).unwrap();
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].email, "first@example.com");
    }

    #[test]
    fn mutations_preserve_hand_written_comments_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "# precious comment\n[ui]\ntheme = \"tailwind-dark\" # inline note\n",
        )
        .unwrap();
        append_account(&path, &sample("kept")).unwrap();
        append_account(&path, &sample("second")).unwrap();
        remove_account(&path, "second").unwrap();
        let mut updated = sample("kept");
        updated.display_name = "Kept Name".to_owned();
        update_account(&path, "kept", &updated).unwrap();

        let content = read(&path);
        assert!(content.contains("# precious comment"), "{content}");
        assert!(content.contains("# inline note"), "{content}");
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].display_name, "Kept Name");
    }

    #[test]
    fn two_appends_round_trip_with_sub_tables_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut first = sample("first");
        first.folders.drafts = "[Gmail]/Drafts".to_owned();
        let mut second = sample("second");
        second.folders.sent = "Sent Items".to_owned();
        append_account(&path, &first).unwrap();
        append_account(&path, &second).unwrap();
        let config: Config = toml::from_str(&read(&path)).unwrap();
        assert_eq!(config.accounts[0].folders.drafts, "[Gmail]/Drafts");
        assert_eq!(config.accounts[1].folders.sent, "Sent Items");
    }

    #[test]
    fn removing_an_unknown_account_errors_and_leaves_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        append_account(&path, &sample("only")).unwrap();
        let before = read(&path);
        assert!(remove_account(&path, "absent").is_err());
        assert_eq!(read(&path), before);
    }

    #[test]
    fn duplicate_name_append_fails_validation_and_leaves_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        append_account(&path, &sample("twin")).unwrap();
        let before = read(&path);
        assert!(append_account(&path, &sample("twin")).is_err());
        assert_eq!(read(&path), before);
    }
}
