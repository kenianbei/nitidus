# feature - Secrets - v1

Roadmap 1d.18: keyring integration + credential-command shell-out; 0600
discipline. The config layer already names secret _sources_ without holding
secret material; this feature makes the keyring source actually work, adds a way
to store secrets into it from inside the app, enforces permission discipline on
plaintext password files, and tightens how resolved secrets travel through the
process.

## 1. Current Design

- `config/account.rs` defines
  `Auth { Keyring (default), PasswordFile, PasswordCmd, Oauth2 }`.
  `password_file` and `password_cmd` are fully functional; `keyring` and
  `oauth2` are parse-only stubs.
- `config/secrets.rs::resolve_password(&Auth, config_dir)` does the work:
  password files read the first non-empty line (`~` expansion, relative paths
  against the config dir); password commands run under `sh -c` and take the
  first stdout line. `Auth::Keyring` bails with "keyring auth lands with the 1d
  secrets work"; `Auth::Oauth2` likewise (stays stubbed until 1d.19).
- Two call sites resolve secrets: `bootstrap.rs` (once, when registering the
  IMAP backend at startup) and `outbox/delivery.rs::build_transport` (on every
  SMTP send).
- Resolved secrets travel as plain `String` fields: `ImapConfig.password`
  (nitidus-mail `imap/mod.rs`) and `SmtpCredentials.password` (`send/mod.rs`).
  Only the final AUTH PLAIN step in `send/smtp.rs` wraps in
  `secrecy::SecretString`. Both structs derive `Debug`/`Clone`, so a stray debug
  log could print a password.
- No permission checks anywhere: a world-readable password file is read
  silently.
- The specification lists **keyring 4.1** (OS-native Secret Service / Keychain /
  Credential Manager) as the intended crate, and requires "no plaintext secrets"
  in the out-of-box config.

## 2. Proposal

1. **Keyring resolution.** Add the `keyring` crate (v4) to nitidus.
   `Auth::Keyring` resolves via `Entry::new("nitidus", <account-name>)`. A
   missing entry produces an actionable startup notice — "no keyring secret for
   <account> — :set-password stores one" — instead of a raw error. v4 requires
   choosing platform backends by feature flag; on Linux that is
   `sync-secret-service` (DBus, persists via GNOME Keyring / KWallet /
   keepassxc) and/or `linux-native` (kernel keyutils, cleared on reboot).
2. **`:set-password` / `:delete-password` commands.** `:set-password` prompts
   with masked input and writes the entry for the account; the account wizard
   (1d.20) will reuse the same helper. `:delete-password` removes the entry.
   Both act on the active account.
3. **0600 discipline.** Password-file resolution checks the file's mode and
   refuses group/world-readable files with a `chmod 600` hint (mutt precedent).
   Unix-only check; a no-op on other platforms.
4. **SecretString end to end.** `ImapConfig.password` and
   `SmtpCredentials.password` become `secrecy::SecretString`: redacted `Debug`,
   zeroized on drop, exposed only at the LOGIN/AUTH call sites.
   `resolve_password` returns `SecretString` accordingly.
5. **Live config migration.** After merge, switch
   `~/.config/nitidus/config.toml` to `auth = "keyring"` for norman.kerr.dev and
   store the app password via `:set-password`, retiring the 0600 file path from
   daily use (file stays as fallback).

Out of scope: OAuth2 (1d.19), the account wizard (1d.20), caching resolved
secrets between sends (each send re-resolves, picking up rotations — cheap and
already the behavior for `password_cmd`).

## 3. Discussion

### 3.1 R1 Questions

1. **Linux keyring backend.** `keyring` v4 needs a feature choice:
   `sync-secret-service` talks DBus to a Secret Service daemon (GNOME Keyring,
   KWallet, KeePassXC — persists across reboots), `linux-native` uses kernel
   keyutils (no daemon, but secrets vanish on reboot). Do you run a Secret
   Service daemon on your Arch setup? Proposal: enable `sync-secret-service`
   (with `crypto-rust`), and treat "no daemon reachable" as the same actionable
   notice as a missing entry.
2. **Entry identity.** Service `"nitidus"`, user = the account `name` from
   config (proposal), or the email address? Name is the stable config key; email
   survives account renames.
3. **Strictness of the 0600 check.** Hard-refuse a group/world-readable password
   file (my proposal — fail loudly at startup with the chmod hint), or just warn
   and proceed?
4. **Masked prompt.** The prompt line currently echoes typed text.
   `:set-password` should render `•` per character — a small `masked` flag on
   `PromptRequest`. Confirm that's the right UX (vs. no echo at all).
5. **SecretString in nitidus-mail's public API.** `ImapConfig` and
   `SmtpCredentials` are public structs; their `password` fields changing to
   `SecretString` touches the library API and tests. OK?
6. **Also store IMAP+SMTP under one entry?** Gmail uses one app password for
   both, and nitidus config has one `auth` per account, so a single keyring
   entry per account covers both directions. Confirm there's no need for
   separate incoming/outgoing secrets in v1.

### 3.2 R1 Answers

1. Let's go with option b, similar to himalaya. My current arch linux may have
   some keyrings installed, however if it's possible, I'd prefer to not have the
   user install packages for the app to work, unless option b will not work
   without it. Does himalaya require a keyring?
2. go with proposed
3. strict
4. confirm
5. ok
6. confirm

### 3.3 R2 Questions

1. (from Norman, re R1.1) Option B, himalaya-style — but does himalaya require
   a keyring daemon? Prefer not to make users install packages for the app to
   work, unless option B cannot work without it.

### 3.4 R2 Answers

1. Himalaya's keyring backend does need a running Secret Service _provider_
   and errors without one (users then fall back to `secret.cmd`). But that
   provider is a stock desktop component, not an extra install: GNOME ships
   GNOME Keyring, KDE ships KWallet/ksecretd, KeePassXC can serve the role
   too. Verified on this machine: ksecretd (kwallet 6.28) already owns
   `org.freedesktop.secrets`, so zero installs are needed here. Nitidus
   itself never hard-requires the daemon — `keyring` is one of four auth
   sources; on a daemon-less box `auth = "keyring"` resolves to the
   actionable notice and `password_file`/`password_cmd` keep working. So:
   option B with `sync-secret-service`, graceful degradation, no new user
   packages.

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **SecretString end to end.** Add `secrecy` to nitidus;
   `resolve_password` returns `SecretString`; `ImapConfig.password` and
   `SmtpCredentials.password` become `SecretString`, exposed only at the
   IMAP LOGIN and SMTP AUTH call sites. Update affected tests.
2. **0600 discipline.** Unix permission gate in the password-file arm:
   mode bits `0o077` set → hard error with a `chmod 600` hint. Tests: 0644
   refused, 0600 accepted.
3. **Keyring resolution.** Add `keyring` 4 (default features — zbus Secret
   Service on Linux). `resolve_password` gains the account name;
   `Auth::Keyring` reads `Entry::new("nitidus", <account-name>)`. Missing
   entry or unreachable daemon maps to an error naming `:set-password`,
   which `bootstrap::register_accounts` already surfaces as a startup
   notice. Unit tests against `keyring::mock` (process-global default
   builder set once behind `std::sync::Once`).
4. **Masked prompt.** `masked` flag on `PromptRequest` (builder method);
   the prompt line renders `•` per character. Unit test.
5. **`:set-password` / `:delete-password`.** Store/delete helpers beside
   resolution in `config/secrets.rs`, command-table entries, masked prompt
   chain acting on the active account, status-line feedback. App-level
   tests on the mock store.
6. **Verification & live migration.** Workspace clippy + full test run
   with pass counts. Pty smoke: switch the live config to
   `auth = "keyring"`, store the norman.kerr.dev app password via
   `:set-password` (lands in ksecretd), restart, confirm IMAP loads INBOX
   and a send authenticates. Fill §5–§7.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **278 passed, 0 failed**
  (was 270 before the feature; +7 secrets/accounts/prompt tests, +1
  cmdline regression test).
- New unit coverage: keyring round-trip and missing-entry message on the
  mock store, 0644 file refused with the chmod hint / 0600 accepted,
  masked prompt renders `*` and never the value (TestBackend buffer),
  `:set-password`/`:delete-password`/empty-submit flows, and the
  cmdline→prompt mode regression.
- Pty smoke (live config on `auth = "keyring"`): startup notice "no
  keyring secret for norman.kerr.dev — :set-password stores one"
  rendered; `:set-password` opens the masked prompt; submit reached the
  zbus store. The headless write failed with an actionable statusline
  error while kdewallet was locked (GUI unlock only); Norman ran
  `:set-password` interactively (KWallet password = login password,
  pam_kwallet-created wallet), after which everything verified live:
  - stored entry byte-identical to the app-password file (sha256);
  - restart authenticated IMAP from the keyring (`1/1` connected,
    INBOX synced, clean log);
  - self-send "keyring smoke 1d18-16194" queued ("sending in 10s — z
    undoes"), transmitted via SMTP-from-keyring ("message sent"),
    arrived back in INBOX over IDLE (3→4), and confirmed server-side by
    direct UID fetch in both INBOX and `[Gmail]/Sent Mail`.

## 6. Implementation Report

- **keyring-core + zbus-secret-service-keyring-store** (feature
  `rt-async-io-crypto-rust`) instead of the `keyring` facade: the v4
  facade hard-codes its store choice on first use, which would have made
  the mock store uninjectable in tests. `ensure_keyring_store` installs
  the zbus store once; tests pre-install `keyring_core::mock`.
- The masked prompt uses tui-prompts' built-in
  `TextRenderStyle::Password` (renders `*`, not the `•` sketched in the
  proposal).
- **Pre-existing bug found by the live smoke:** the command line
  dispatched the action and then unconditionally reset the input mode to
  Normal, clobbering the prompt `:set-password` had just opened — the
  typed secret then leaked into normal-mode key routing. Fixed by
  closing the command line before dispatch; regression-tested. Any
  future prompt-opening command benefits.
- Locked-collection UX: the zbus store runs the Secret Service
  unlock-prompt dance, so on the desktop `:set-password` pops the
  KWallet dialog and proceeds; the statusline shows the store error if
  the user dismisses it.
- Follow-ups: OAuth2 arm still stubbed (1d.19); the wizard (1d.20)
  reuses `store_password`; consider `keyring` auth caching between sends
  only if the per-send keyring read ever becomes noticeable.



- Live-migration note: the account registers once at bootstrap, so a
  secret stored mid-session takes effect on the next start (all syncs
  in the storing session log "unknown account"). Re-registering live
  after `:set-password` is a follow-up, natural to fold into the 1d.20
  wizard.

## 7. Testing and Cleanup

- Cleanup scope: the branch diff vs main. Two comments fixed (the
  `config/secrets` module doc still described the pre-keyring,
  startup-only world; `accounts` module doc cited roadmap info). No
  compiler- or clippy-flagged dead code; the `keyring` facade dep was
  already dropped for `keyring-core` during implementation.
- A `git add -A` swept two pre-existing rustfmt-only diffs
  (`nitidus-mail/src/event.rs`, `thread.rs`) into the feature commit —
  token-identical code, formatting only. Workspace-wide `cargo fmt`
  remains deferred as the standing chore.
- Final verification: `cargo clippy --workspace --all-targets` zero
  warnings; `CARGO_INCREMENTAL=0 cargo test --workspace` **278 passed,
  0 failed** (suite-level counts confirmed present).
