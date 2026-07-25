# feature - Account Wizard - v1

Roadmap 1d.20: a guided `:new-account` flow that writes config + secrets, so
adding a mailbox never requires hand-editing TOML. It composes the pieces the
last two features built — masked prompts, keyring storage, `:authorize` — and
absorbs two parked follow-ups: live account registration (no more "restart to
connect") and per-provider folder presets (the O365 `Sent Items`/self-filing
learnings from the 1d.19 smoke).

## 1. Current Design

- Adding an account means editing `~/.config/nitidus/config.toml` by hand;
  `config/load.rs` only reads (parse → validate → `LoadedConfig`), nothing in
  the app writes config.
- All the per-account features the wizard must cover already work when
  hand-written: identity (`display_name` builds the compose From line, `aliases`
  feed reply self-filtering), folder mapping
  (`folders.{drafts, sent, trash, archive, save_sent}` drive
  postpone/sent-copy/recall), and `signature`/`signature_file` (appended to new
  compose bodies).
- Secrets and grants are interactive already: `:set-password` (masked prompt →
  keyring) and `:authorize` (code/device grant → keyring) — but both only take
  effect at the next start, because accounts register with the engine once, in
  `bootstrap::register_accounts`. The engine itself can add accounts at any time
  (`add_account`/`watch_imap` are ordinary calls on the running engine).
- Provider knowledge shipped so far: OAuth endpoints/scopes per provider in
  `config/presets.rs`; the operational folder facts (Gmail self-files sent mail,
  `[Gmail]/Drafts`; Exchange self-files too, `Sent Items`/`Deleted Items`) live
  only in the design docs and Norman's live config.
- Prompt/picker machinery: chained `PromptRequest`s (compose header chain is the
  model), masked input, and the overlay `PickerSpec` for list choices.
- The active account is `IndexView.account`; `:authorize`/`:set-password` target
  it, and it defaults to the first configured account.

## 2. Proposal

1. **`:new-account` prompt chain**, deliberately minimal — four to six steps,
   everything else editable in config later:
   - _name_ (config key; validated unique) and _email_;
   - _provider picker_: _Gmail_, _Outlook / Office 365_, or _Custom IMAP_.
     Picking a provider fills backend host, outgoing SMTP, **and the folder
     preset** (Gmail: `save_sent = false`, drafts `[Gmail]/Drafts`; O365:
     `Sent Items`, `Deleted Items`, `save_sent = false`). Custom prompts for
     IMAP host and SMTP host with sane defaults (ports/encryption from the
     existing config defaults);
   - _auth picker_: _OAuth2_ (prompts `client_id`, optional `client_secret`;
     Microsoft also defaults `flow = "code"` since the live smoke showed that is
     what real tenants take) or _password_ (keyring; chains straight into the
     masked `:set-password` prompt) or _password command_ (prompts the command);
   - _display name_ (optional, Enter skips).
2. **Config writing by append.** The wizard serializes one `[[accounts]]` block
   and appends it to `config.toml` (creating the file if absent) under a
   `# added by :new-account` comment. Appending never touches existing content,
   so hand-written comments and formatting survive; no `toml_edit` dependency.
   The block is round-tripped through the real serializer and re-parsed before
   writing — what lands in the file is guaranteed loadable.
3. **Live registration** — the restart contract ends:
   - a new `accounts::register_live` helper updates the `Config` resource, calls
     the existing bootstrap registration for just the new account, and kicks the
     folder list + INBOX sync;
   - `:new-account` finishes through it (after the password prompt, or after the
     `:authorize` grant lands, which the wizard chains into automatically for
     OAuth accounts);
   - `:set-password` and `:authorize` on an existing-but-unregistered account
     also finish through it, retiring the ":set-password stores one — restart"
     notices to cover the from-scratch case only.
4. **Validation before write**: unique account name, non-empty email with an
   `@`, non-empty hosts. Failures re-prompt with the reason in the statusline
   rather than aborting the chain.

Out of scope: editing or removing existing accounts (`:remove-account` can be a
later chore — deleting a TOML block by hand is easy), signature entry in the
wizard (a config edit, documented in the finish notice), keymaps or UI settings,
and OAuth client provisioning (the wizard links the doc recipe; it cannot click
through Cloud Console).

## 3. Discussion

### 3.1 R1 Questions

1. **Prompt-chain scope.** Is the minimal chain above right (name, email,
   provider, auth, display name) — with folders/ports/signature left to config
   edits? Or should the wizard also prompt for folder names when _Custom IMAP_
   is picked (Gmail/O365 have presets; custom servers get RFC-standard
   defaults)?
2. **Config append.** OK with append-only writing (existing file content,
   comments included, never rewritten)? The alternative is `toml_edit` for
   structured in-place editing — heavier, but it would enable future
   `:edit-account`/`:remove-account` commands to modify blocks safely.
3. **Live-registration reach.** Extending live registration to
   `:set-password`/`:authorize` (proposal 3c) touches the bootstrap path for a
   feature nominally about the wizard — but it is the same helper and finally
   kills the restart contract. Include it, or wizard-only for now?
4. **Microsoft OAuth default.** For the O365 provider preset, should the wizard
   prefill Thunderbird's public client id in the `client_id` prompt (what the UO
   smoke used; gray-zone but ecosystem-standard, and Enter accepts) — or leave
   the prompt empty and only mention it in the docs?
5. **Backend choice.** The wizard only offers IMAP backends (Gmail / O365 /
   custom). Maildir accounts stay hand-configured — they are a
   developer/power-user path. Confirm.
6. **After-finish UX.** On success: statusline "account <name> added — syncing
   INBOX", sidebar gains the account, and the new account becomes the active one
   (so the first thing you see is its INBOX filling in). Confirm that
   switch-to-new-account behavior is wanted.

### 3.2 R1 Answers

1. Let's include all, and have presets for known providers.
2. toml_edit and let's include edit and remove account commands in this feature.
3. include
4. prefill
5. confirm
6. confirm

Also, I think we should automatically enter the account wizard if the app is
started with no accounts. I'd like to smoke test this from scratch, once your
implementation is done, let's smoke test :remove-account on the current gmail
account, then I will quit, restart, and go through the full account add process
with that account.

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **Config writer on toml_edit.** New `config/write.rs`:
   `append_account`, `update_account`, `remove_account` operating on
   `DocumentMut` (append creates the file; update/remove locate the
   `[[accounts]]` entry by `name`), atomic tmp+rename writes, and every
   mutation round-trips through parse+validate before touching disk.
   Tests prove comments and formatting elsewhere in the file survive.
2. **Engine account removal.** `MailEngine` stores per-account watcher
   `JoinHandle`s (`watch_imap`/`watch_maildir` become `&mut self`);
   `remove_account` drops the command sender (the actor loop ends on
   channel close) and aborts the watchers. `CacheWriter` gains
   `purge_account`; `MailStore` gains `remove_account`. Tests: removed
   account stops answering, watcher task ends, cache rows gone.
3. **Live registration.** `accounts::register_live(world, name)` —
   idempotent: config lookup → `bootstrap` single-account registration
   on the running engine → folder list + INBOX sync → sidebar/store
   pick it up. `:set-password` submit and the OAuth `Granted` event
   finish through it when the account exists but is not registered —
   the restart notices die. Tests against the maildir backend.
4. **`:new-account` wizard.** New `accounts/wizard.rs`: prompt/picker
   chain (name → email → provider picker → custom-only host and folder
   prompts, prefilled with defaults → auth picker → oauth
   client prompts, Thunderbird client id prefilled for O365 → masked
   password or command prompt → display name), a connection+folders
   preset table per provider (Gmail / O365 from the 1d.19 learnings),
   validation with re-prompt on bad input, then: write config block,
   update the `Config` resource, store secrets, `register_live` (OAuth
   without a stored grant chains into `:authorize` and registers on
   `Granted`), switch the active account to the new one. Headless
   chain tests on the mock keyring + temp config.
5. **`:edit-account`, `:remove-account`, first-run entry.**
   Edit re-runs the chain prefilled from the existing block and writes
   via `update_account` + re-register (engine remove + add).
   Remove: account picker → y/n confirm → config block removed, engine
   account + watchers torn down, store/cache purged, active account
   falls back to the first remaining; keyring entries are kept
   (removal is not revocation) and the notice says so. Startup with
   zero configured accounts auto-opens the wizard. Tests for all
   three.
6. **Verification & live smoke.** Clippy + full workspace run with
   counts. Norman's scripted smoke: back up the gmail client
   credentials, `:remove-account` the live gmail account, quit,
   restart into the auto-entered wizard, re-add gmail end to end
   (oauth grant already in the keyring, so registration should
   complete without re-consenting). Fill §5–§7.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **312 passed, 0 failed**
  (was 297 at branch start).
- New coverage: config-writer append/update/remove with comment
  preservation and validation-guarded atomic writes; engine account
  removal (actor stops, second removal is a no-op) and cache purge
  scoped to one account; idempotent live registration over maildir;
  the full wizard chains headlessly (Gmail-preset + keyring path
  chaining into the masked password prompt, Custom path with host and
  folder prompts and password command, duplicate-name re-prompt,
  zero-account auto-entry); `:remove-account` teardown with
  active-view fallback and decline-keeps-account; `:edit-account`
  updating the block in place.
- Headless pty smokes: a blank config dir boots straight into the
  wizard ("Account name:" prompt on an empty index); the live config
  still connects and syncs through the untouched startup path.
- **Live smoke (2026-07-25, Norman): PASSED**, covering more than the
  script: `:remove-account` on the live gmail account (config block
  gone, teardown clean), restart into the auto-entered wizard, re-add
  via the **keyring password** path (the 1d.18 secret was found — the
  account connected live, no browser), then `:edit-account` switching
  auth to **oauth2** (the 1d.19 grant was found — re-registered live,
  no re-consent). The resulting config block is exactly the Gmail
  preset with his OAuth client.

## 6. Implementation Report

- Config writing settled on a hybrid after toml_edit's structured
  insert scrambled sub-table ordering (duplicate `[accounts.folders]`
  headers): new blocks append as text serialized by the real config
  serializer, removal is structured through toml_edit, update =
  remove + append (the edited block moves to the file end). Every
  mutation re-parses and re-validates before an atomic tmp+rename
  write, so a failed mutation can never corrupt the file.
- Engine teardown: watcher tasks are now tracked per account and
  aborted on removal; the actor ends when its command channel closes.
  Cache rows purge through a writer op; store and sync tracker drop
  their account state.
- Live registration extracted the single-account path out of the
  bootstrap loop (`register_one`); a deliberate behavior change rode
  along: a broken account (bad maildir path) now becomes a startup
  notice instead of failing the whole app.
- The wizard is a chain of `FnOnce` prompt closures threading a Draft;
  pickers clone it (their callbacks are `Fn`). Prefills use the
  prompt's initial-text mechanism, which means "edit the default"
  rather than "replace on type" — clearing requires backspaces.
- `:edit-account` re-runs the same chain with `editing: Some(original)`
  (rename-safe: the update locates the block by the original name, and
  runtime state for the old name is detached).
- Follow-ups: `Esc` mid-chain currently just abandons the draft
  silently (a "setup cancelled" notice would be nicer); the wizard
  offers IMAP only (maildir accounts stay hand-written, per R1); a
  paste-friendly multi-char prompt still renders long client ids as a
  clipped single line.

## 7. Testing and Cleanup

- Cleanup scope: the branch diff vs main. No comment fixes needed on
  review (new modules' docs state invariants — the toml_edit ordering
  workaround, removal-is-not-revocation, the prefill-append caveat is
  in §6); no dead code — clippy silent, all helpers have callers.
- File-size discipline: `wizard/presets.rs` split out of the wizard
  chain when it crossed the non-test line budget.
- Final verification after the live smoke:
  `cargo clippy --workspace --all-targets` zero warnings;
  `CARGO_INCREMENTAL=0 cargo test --workspace` **312 passed, 0
  failed** (suite counts confirmed present).
