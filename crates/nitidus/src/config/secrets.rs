//! Password resolution at account registration. Secrets stay out of
//! config values: the config names a file or command, and this module
//! turns it into the secret string exactly once at startup.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use super::account::Auth;

/// First line of the configured source, trimmed. Keyring and OAuth2
/// resolve to a descriptive error that becomes a startup notice until
/// the 1d auth work lands.
pub fn resolve_password(auth: &Auth, config_dir: &Path) -> anyhow::Result<String> {
    match auth {
        Auth::PasswordFile(file) => {
            let path = resolve_path(&file.path, config_dir)?;
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
        Auth::Keyring => bail!("keyring auth lands with the 1d secrets work"),
        Auth::Oauth2(_) => bail!("oauth2 auth lands with the 1d auth work"),
    }
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

fn first_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::config::account::{PasswordCmdAuth, PasswordFileAuth};

    #[test]
    fn reads_first_nonempty_line_of_a_password_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");
        std::fs::write(&path, "\n  hunter2  \nsecond\n").unwrap();
        let auth = Auth::PasswordFile(PasswordFileAuth { path });
        assert_eq!(resolve_password(&auth, dir.path()).unwrap(), "hunter2");
    }

    #[test]
    fn relative_paths_resolve_against_the_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("kenianbei-password"), "s3cret\n").unwrap();
        let auth = Auth::PasswordFile(PasswordFileAuth {
            path: PathBuf::from("kenianbei-password"),
        });
        assert_eq!(resolve_password(&auth, dir.path()).unwrap(), "s3cret");
    }

    #[test]
    fn password_command_takes_first_stdout_line() {
        let auth = Auth::PasswordCmd(PasswordCmdAuth {
            command: "printf 'top\\nrest\\n'".to_owned(),
        });
        assert_eq!(
            resolve_password(&auth, Path::new("/nonexistent")).unwrap(),
            "top"
        );
    }

    #[test]
    fn failing_sources_error_with_context() {
        let dir = tempfile::tempdir().unwrap();
        let missing = Auth::PasswordFile(PasswordFileAuth {
            path: dir.path().join("absent"),
        });
        assert!(resolve_password(&missing, dir.path()).is_err());

        let failing = Auth::PasswordCmd(PasswordCmdAuth {
            command: "exit 3".to_owned(),
        });
        let message = resolve_password(&failing, dir.path())
            .unwrap_err()
            .to_string();
        assert!(message.contains("exit"), "{message}");

        assert!(resolve_password(&Auth::Keyring, dir.path()).is_err());
    }
}
