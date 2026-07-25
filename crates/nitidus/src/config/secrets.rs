//! Secret resolution: the config names a source (keyring entry, file,
//! command) — never secret material; resolution happens at account
//! registration and per outgoing send. Storage lives in `keyring`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use nitidus_mail::SecretString;

use super::account::Auth;

/// The account's secret from its configured source: keyring entry,
/// first non-empty line of a file, or first stdout line of a command.
/// OAuth2 resolves to a descriptive error until 1d.19 lands.
pub fn resolve_password(
    auth: &Auth,
    config_dir: &Path,
    account_name: &str,
) -> anyhow::Result<SecretString> {
    match auth {
        Auth::PasswordFile(file) => {
            let path = resolve_path(&file.path, config_dir)?;
            ensure_private(&path)?;
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("reading password file {}", path.display()))?;
            first_line(&content)
                .with_context(|| format!("password file {} is empty", path.display()))
        }
        Auth::PasswordCmd(cmd) => {
            let output = Command::new("sh")
                .arg("-c")
                .arg(&cmd.command)
                .output()
                .with_context(|| format!("running password command {:?}", cmd.command))?;
            if !output.status.success() {
                bail!(
                    "password command {:?} exited with {}",
                    cmd.command,
                    output.status
                );
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            first_line(&stdout)
                .with_context(|| format!("password command {:?} printed nothing", cmd.command))
        }
        Auth::Keyring => super::keyring::load_password(account_name),
        Auth::Oauth2(_) => bail!("oauth2 accounts authenticate via :authorize, not passwords"),
    }
}

const GROUP_OR_WORLD_BITS: u32 = 0o077;

/// Refuses password files other users can touch, fetchmail-style.
#[cfg(unix)]
fn ensure_private(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .with_context(|| format!("checking password file {}", path.display()))?
        .permissions()
        .mode();
    if mode & GROUP_OR_WORLD_BITS != 0 {
        bail!(
            "password file {} is accessible by others (mode {:03o}) — chmod 600 it",
            path.display(),
            mode & 0o777
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

fn resolve_path(path: &Path, config_dir: &Path) -> anyhow::Result<PathBuf> {
    if let Ok(stripped) = path.strip_prefix("~") {
        let home = etcetera::home_dir().context("cannot resolve home dir for ~ expansion")?;
        return Ok(home.join(stripped));
    }
    if path.is_relative() {
        return Ok(config_dir.join(path));
    }
    Ok(path.to_path_buf())
}

fn first_line(content: &str) -> Option<SecretString> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(SecretString::from)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use nitidus_mail::ExposeSecret;

    use super::*;
    use crate::config::account::{PasswordCmdAuth, PasswordFileAuth};

    fn write_secret(path: &Path, content: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, content).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn resolved(auth: &Auth, config_dir: &Path) -> String {
        resolve_password(auth, config_dir, "test-account")
            .unwrap()
            .expose_secret()
            .to_owned()
    }

    #[test]
    fn reads_first_nonempty_line_of_a_password_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");
        write_secret(&path, "\n  hunter2  \nsecond\n");
        let auth = Auth::PasswordFile(PasswordFileAuth { path });
        assert_eq!(resolved(&auth, dir.path()), "hunter2");
    }

    #[test]
    fn relative_paths_resolve_against_the_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        write_secret(&dir.path().join("kenianbei-password"), "s3cret\n");
        let auth = Auth::PasswordFile(PasswordFileAuth {
            path: PathBuf::from("kenianbei-password"),
        });
        assert_eq!(resolved(&auth, dir.path()), "s3cret");
    }

    #[test]
    fn password_command_takes_first_stdout_line() {
        let auth = Auth::PasswordCmd(PasswordCmdAuth {
            command: "printf 'top\\nrest\\n'".to_owned(),
        });
        assert_eq!(resolved(&auth, Path::new("/nonexistent")), "top");
    }

    #[test]
    fn group_or_world_readable_password_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loose");
        std::fs::write(&path, "hunter2\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let auth = Auth::PasswordFile(PasswordFileAuth { path });
        let message = resolve_password(&auth, dir.path(), "test-account")
            .unwrap_err()
            .to_string();
        assert!(message.contains("chmod 600"), "{message}");
    }

    #[test]
    fn keyring_source_resolves_through_the_stored_entry() {
        crate::config::keyring::use_mock_keyring();
        crate::config::keyring::store_password("resolve-keyring", "k3yr1ng").unwrap();
        let secret =
            resolve_password(&Auth::Keyring, Path::new("/nonexistent"), "resolve-keyring").unwrap();
        assert_eq!(secret.expose_secret(), "k3yr1ng");
    }

    #[test]
    fn failing_sources_error_with_context() {
        let dir = tempfile::tempdir().unwrap();
        let missing = Auth::PasswordFile(PasswordFileAuth {
            path: dir.path().join("absent"),
        });
        assert!(resolve_password(&missing, dir.path(), "test-account").is_err());

        let failing = Auth::PasswordCmd(PasswordCmdAuth {
            command: "exit 3".to_owned(),
        });
        let message = resolve_password(&failing, dir.path(), "test-account")
            .unwrap_err()
            .to_string();
        assert!(message.contains("exit"), "{message}");
    }
}
