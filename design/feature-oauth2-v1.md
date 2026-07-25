# feature - OAuth2 - v1

Roadmap 1d.19: io-oauth flows (Google installed-app, Microsoft device-code),
token refresh, per-provider presets.
`auth = { oauth2 = { provider = "google" } }` stops being a parse-only stub:
`:authorize` runs the interactive grant once, the refresh token lives in the OS
keyring (1d.18 infrastructure), and IMAP/SMTP authenticate via XOAUTH2 with
automatic access-token refresh.

## 1. Current Design

- `Auth::Oauth2(Oauth2Auth { provider: Google | Microsoft })` parses but
  `resolve_password` bails with "oauth2 auth lands with the 1d auth work".
- 1d.18 delivered the storage layer: `keyring-core` + zbus Secret Service store,
  `store_password`/`delete_password` helpers, masked prompt, mock store for
  tests.
- **The SASL side already exists in our deps**: io-imap 0.2 ships
  `ImapAuthXoauth2` (and OAUTHBEARER via rfc7628), io-smtp 0.2 ships
  `SmtpAuthXoauth2` — both `new(user, token, opts)` coroutines identical in
  shape to the `ImapLogin`/`SmtpAuthPlain` we pump today.
- io-oauth 0.2 (Pimalaya, not yet a dependency) provides sans-IO coroutines for
  the authorization-code grant (`rfc6749`), PKCE (`rfc7636`), the device grant
  (`rfc8628`), and `refresh_access_token` — same `resume()`/WantsRead/WantsWrite
  pattern our `imap/pump.rs` and `send/pump.rs` already drive over
  `net.rs::RemoteStream` (TLS). No HTTP client exists in the workspace and none
  is needed.
- `ImapConfig`/`SmtpCredentials` carry `user` + `password: SecretString` only —
  no notion of a bearer token, and no way for a mid-session reconnect to obtain
  a fresher credential than the one captured at registration.
- Gmail quirk that motivates the feature: app passwords work today, but Google
  is deprecating them for consumer accounts and OAuth is the supported path
  (scope `https://mail.google.com/` covers IMAP + SMTP).

## 2. Proposal

1. **Config.** `Oauth2Auth` grows `client_id: String` (plaintext — Google treats
   installed-app credentials as non-confidential) and optional
   `client_secret: String`. Provider presets compile in the endpoints and
   scopes: Google (auth + token endpoints, `https://mail.google.com/`, loopback
   redirect), Microsoft (`login.microsoftonline.com/common`,
   `https://outlook.office.com/IMAP.AccessAsUser.All` + `SMTP.Send` +
   `offline_access`, device grant).
2. **Credential plumbing.** `ImapConfig.password`/`SmtpCredentials` become an
   auth enum in nitidus-mail: `Login { password }` (today's path) or
   `Xoauth2 { tokens: Arc<dyn TokenSource> }`. `TokenSource` is a small
   nitidus-mail trait — `fn access_token(&self) -> Result<SecretString>` —
   called at every (re)connect, so reconnects always authenticate with a live
   token.
3. **Refresh.** The app-side `TokenSource` impl holds client credentials, the
   keyring-stored refresh token, and a cached access token + expiry. When stale,
   it runs io-oauth's `refresh_access_token` coroutine over the shared TLS pump,
   persists a rotated refresh token back to the keyring, and returns the fresh
   access token. One retry on XOAUTH2 rejection forces a refresh (server-side
   revocation heals without restart).
4. **`:authorize` command** on the active account:
   - _Google (installed-app):_ generate PKCE, open the system browser
     (`xdg-open`, with the URL printed in the statusline as fallback), run a
     loopback listener on `127.0.0.1:<random>`, exchange the code, store the
     refresh token in the keyring, notice "authorized — restart to connect"
     (same restart contract as `:set-password`).
   - _Microsoft (device grant):_ overlay showing `user_code` + verification URL,
     poll the token endpoint in the background until granted or expired.
5. **Keyring layout.** Refresh token under service `nitidus`, user
   `<account>/oauth-refresh` — separate from the password entry so
   `:delete-password` and `:deauthorize` (removes the token) stay independent.
   Access tokens are short-lived and cached in memory only.
6. **Startup UX.** Missing refresh token resolves to the actionable notice
   ":authorize connects <account>", mirroring the `:set-password` notice.

Out of scope: OAUTHBEARER (XOAUTH2 covers Gmail and Outlook; the rfc7628
coroutines exist if a provider ever requires it), dynamic client registration,
credential-command token storage, and the account wizard (1d.20 — it will chain
`:authorize` after writing config).

## 3. Discussion

### 3.1 R1 Questions

1. **Google client credentials.** Live-testing the Google flow needs an OAuth
   client of type "Desktop app" created once in Google Cloud Console (free, a
   few clicks, for the norman.kerr.dev account; Gmail API or restricted-scope
   consent screen in testing mode is enough for our own mailbox). Are you up for
   creating one, and pasting `client_id` + `client_secret` into the config?
   Without it the Google flow ships mock-tested only.
2. **Microsoft live testing.** No Microsoft mailbox exists in this setup, so the
   device flow would ship implemented against a scripted in-process token server
   but never exercised against real Outlook. OK to mark it experimental until a
   real account exists?
3. **Token plumbing.** Is the `TokenSource` trait in nitidus-mail (refresh
   executed inside the app impl, nitidus-mail just calls `access_token()` on
   every connect) the shape you want? Alternative: keep nitidus-mail dumb
   (static token in config) and have the app proactively refresh + re-register
   the account — simpler crate boundary, but a mid-IDLE reconnect after token
   expiry would fail until the app notices.
4. **Browser opening.** Is shelling out to `xdg-open` acceptable for the Google
   flow (statusline shows the URL as fallback for headless/SSH sessions)? A
   config override (`browser_cmd`) can wait until someone needs it.
5. **Restart contract.** Like `:set-password`, `:authorize` stores the token and
   asks for a restart rather than hot-registering the account — the live
   re-register follow-up stays parked for 1d.20. Confirm.
6. **Scopes.** Google `https://mail.google.com/` is the full-mail scope
   (IMAP+SMTP via XOAUTH2 need it). Confirm using it rather than narrower Gmail
   API scopes that XOAUTH2 does not accept.

### 3.2 R1 Answers

1. Let's mock test only, you can give me directions for smoke testing.
2. I have an account I can use to smoke test, but let's just mock test on your
   end.
3. option 1
4. yes
5. confirm
6. sure

Feel free to include me in the testing process for manual smoke testing.

### 3.3 R2 Questions

1. (from Norman) What auth mechanism works for a school account like
   nkerr@uoregon.edu? UO's Thunderbird guide was the reference.

### 3.4 R2 Answers

1. UO officially supports IMAP/SMTP **with OAuth2 required** — exactly
   our XOAUTH2 — so the mechanism is fine; the school-tenant wrinkles
   are the client registration (solved by using Thunderbird's public
   client id, `9e5f94bc-…`, which UO's documented Thunderbird support
   implies is consented in their tenant) and the grant shape
   (Thunderbird authenticates with the browser code flow, not the
   device flow our Microsoft preset defaulted to). Decision: add an
   optional `flow = "code" | "device"` override to the oauth2 config,
   defaulting to the provider preset, and live-smoke UO with
   Thunderbird's client id + `flow = "code"` before committing.

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **Auth enum in nitidus-mail.** `ImapConfig`/`SmtpCredentials` carry
   `MailAuth { Login(SecretString), Xoauth2(Arc<TokenRefresher>) }`.
   `imap/session.rs` and `send/smtp.rs` match on it (`ImapAuthXoauth2` /
   `SmtpAuthXoauth2`); an XOAUTH2 rejection invalidates the cached token
   and retries once. `TokenRefresher` lands as a stub (static token
   constructor for tests) so this phase compiles standalone.
2. **io-oauth refresh machinery.** New `nitidus-mail::oauth` module:
   `TokenRefresher` holds token-endpoint target, client credentials, the
   refresh token, a cached access token + expiry (60s staleness margin),
   and a persistence callback invoked on refresh-token rotation. Refresh
   pumps io-oauth's `refresh_access_token` over `net.rs` (plaintext
   allowed for in-process test servers, mirroring `ImapEncryption::None`).
   Scripted token-server tests: cache hit, refresh, rotation persisted,
   error surfaced.
3. **Config, presets, resolution.** `Oauth2Auth` gains
   `client_id`/`client_secret`; a presets module maps provider → endpoints
   + scopes. `bootstrap` and `outbox::delivery` build `MailAuth::Xoauth2`
   from config + keyring (`<account>/oauth-refresh` entry via new
   `secrets` helpers); a missing token resolves to the ":authorize
   connects <account>" notice. Password accounts keep today's path.
4. **`:authorize` / `:deauthorize`.** `nitidus-mail::oauth` exposes the
   grant flows (auth-code + PKCE + loopback listener; device-code with
   polling); `MailEngine` exposes its runtime handle so the app can spawn
   them without blocking the UI. Results return over a channel resource
   drained by a bevy system → keyring store + statusline notice. Google:
   `xdg-open` + URL fallback. Microsoft: user-code + URL notice while
   polling. `:deauthorize` deletes the token entry.
5. **App-level tests.** Device flow end-to-end against the scripted token
   server (mock keyring); auth-code flow end-to-end with the test driving
   the loopback redirect itself (no browser); resolution-notice and
   deauthorize tests.
6. **Verification & smoke directions.** Workspace clippy + full test run
   with pass counts. Written smoke instructions for Norman: Google
   (Desktop-app client creation, config lines, `:authorize` walkthrough)
   and Microsoft (device flow with his account). Fill §5–§7; live smoke
   results appended when Norman runs them.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **295 passed, 0 failed**
  (was 278 at branch start; +17 across the phases).
- New coverage: XOAUTH2 IMAP authenticate + rejected-token retry-once
  (scripted IMAP server), XOAUTH2 SMTP delivery (scripted SMTP server),
  TokenRefresher refresh/cache/expiry/rotation-persist/denial (scripted
  token server), presets parse as https, oauth2 config parsing,
  keyring token entry separate from the password entry, resolution
  notices (`:authorize` named when no grant is stored), and two
  end-to-end grants: the device flow (codes → prompt event → pending
  poll → grant → keyring) and the code flow (browser-prompt event →
  test-driven loopback redirect with percent-encoded state → PKCE
  exchange → keyring).
- Pty regression smoke: the live norman.kerr.dev account (keyring
  password auth) still connects through the new `resolve_auth` path —
  `1/1` connected, INBOX synced, clean exit.
- Live provider smokes are Norman's manual step (R1: mock-test here,
  smoke directions below); results get appended here.
- **Google live smoke (2026-07-25, Norman): PASSED.** Fresh Cloud
  project + Desktop-app client; first attempt hit 403 access_denied
  (account missing from Test users — directions amended), second
  attempt completed consent, the loopback redirect (heavily
  percent-encoded state) validated, and the refresh token landed in
  the keyring (`1//0…` prefix verified). After restart the account
  registered without notices, INBOX loaded over IMAP XOAUTH2, and a
  self-send delivered over SMTP XOAUTH2; clean exit in the log.
  Microsoft device-flow live smoke still pending a real mailbox.
- **Microsoft live smoke (2026-07-25, Norman, nkerr@uoregon.edu on the
  University of Oregon tenant — code flow + Thunderbird client id):
  PASSED after three real-world fixes it flushed out:**
  1. *AADSTS90013*: Entra rejects CSRF states containing
     HTML-dangerous characters; io-oauth's printable-ASCII state was
     replaced with one drawn from PKCE's unreserved alphabet
     (reproduced and confirmed against the live authorize endpoint
     with curl before the fix).
  2. *AADSTS50011*: Entra matches the loopback redirect host
     literally; the redirect URI now advertises `localhost` (Google
     accepts either; the listener still binds `127.0.0.1`).
  3. *SELECT `BAD Command Argument Error`*: Exchange Online lacks
     CONDSTORE; the SELECT parameter is now sent only when the
     capability is advertised (regression-tested both ways; O365
     degrades to full rescans exactly like the existing
     missing-modseq fallback).
  End state: consent through UO SSO + Duo, grant stored, restart
  registered both accounts, folders listed and switched over IMAP
  XOAUTH2, and a test send delivered over SMTP XOAUTH2 to the gmail
  account. Sent-folder learnings recorded in §6.

### Smoke directions (Google, norman.kerr.dev)

1. Create the OAuth client (one-time): console.cloud.google.com →
   **New project** (free, no billing) → enable the **Gmail API**
   (required even for IMAP/SMTP XOAUTH2) → OAuth consent screen:
   External, stay in **Testing** mode, add norman.kerr.dev@gmail.com
   as a **test user** → Credentials → Create credentials → OAuth
   client ID → type **Desktop app**. Note: testing-mode refresh tokens
   expire after 7 days — re-run `:authorize` weekly, or publish the
   app unverified to lift the cap (warning screen instead).
2. In `~/.config/nitidus/config.toml`, replace `auth = "keyring"` with:
   `auth = { oauth2 = { provider = "google", client_id = "<id>", client_secret = "<secret>" } }`
3. Run nitidus — expect the startup notice "no oauth grant for
   norman.kerr.dev — :authorize connects it". Type `:authorize`: the
   browser opens Google's consent page (the URL is also in the
   statusline); approve with the norman.kerr.dev account. The tab says
   "nitidus is authorized", the statusline says "authorized — restart
   to connect".
4. Restart nitidus: INBOX loads via IMAP XOAUTH2. Send yourself a mail
   (`m`) to prove SMTP XOAUTH2. `:deauthorize` + restart returns the
   notice.

### Smoke directions (Microsoft, device flow)

1. Register an app: entra.microsoft.com → App registrations → New —
   supported accounts: personal Microsoft accounts (or all), no
   redirect URI needed. On the app: Authentication → Advanced →
   **Allow public client flows: Yes**. Copy the Application (client)
   ID.
2. Config: imap `outlook.office365.com`, smtp `smtp.office365.com`
   port 587 starttls, and
   `auth = { oauth2 = { provider = "microsoft", client_id = "<app id>" } }`.
3. `:authorize` → statusline shows "enter code XXXX-XXXX at
   https://microsoft.com/devicelogin"; complete it in any browser.
   Restart to connect.

## 6. Implementation Report

- The engine side is symmetrical with password auth:
  `MailAuth::{Login, Xoauth2}` lives in nitidus-mail; both
  `imap/session.rs` and `send/smtp.rs` retry XOAUTH2 exactly once
  after a server rejection (invalidate → fresh token), since a
  rejected AUTHENTICATE leaves the connection usable.
- `TokenRefresher` (nitidus-mail::oauth) serializes concurrent
  refreshes behind an async gate so parallel IMAP/SMTP connects do one
  token round-trip, caches with a 60s staleness margin (30min
  fallback lifetime when the server omits `expires_in`), and reports
  rotated refresh tokens through a persistence callback — the app's
  callback writes the keyring `<account>/oauth-refresh` entry.
- All OAuth HTTP runs through io-oauth coroutines over the existing
  `net.rs` transport (plain TCP allowed for in-process test servers) —
  no HTTP client dependency; a single generic pump normalizes the
  per-flow resume enums.
- `keyring` storage split out of `secrets.rs` into `config/keyring.rs`
  when token entries joined password entries (file-size limit).
- The loopback listener answers stray browser requests (favicon) with
  404 and keeps waiting; the state check is CSRF-strict via io-oauth's
  `validate`.
- The statusline carries the full authorization URL as fallback when
  `xdg-open` fails; it clips visually on narrow terminals — a copyable
  overlay is a possible 1f comfort follow-up.
- R2 additions from the UO smoke: the `flow` config override; the
  Entra-safe state alphabet; the `localhost` redirect host; CONDSTORE
  negotiation by capability; engine job failures now logged to the
  file (they were statusline-only, which made the smoke undiagnosable)
  while routine scan cancellations are silenced entirely.
- Office 365 operational learnings (candidate for per-provider folder
  presets later): folders are `Sent Items`/`Deleted Items`, and
  Exchange self-files SMTP-submitted mail into Sent Items
  (`MessageCopyForSMTPClientSubmission`, default on) — so
  `save_sent = false`, same as Gmail.
- Follow-ups: live re-registration after `:authorize`/`:set-password`
  (parked for the 1d.20 wizard), OAUTHBEARER if a provider requires
  it, `:authorize <account>` argument (today it targets the active
  account, and an unauthorized account can only become active by
  being first in the config), a folder-sync progress indicator
  (Norman's ask — the first O365 scan is a full download and looks
  like a hang), Microsoft *device-flow* live validation (needs a
  client registration with public client flows enabled), and
  per-provider folder-name presets.

## 7. Testing and Cleanup

- Cleanup scope: the branch diff vs main. One comment fixed (the
  `oauth` module doc gained the `device` module reference after the
  file split); no dead code — clippy is silent and every helper has
  callers.
- File-size discipline drove two splits during implementation:
  `config/keyring.rs` out of `secrets.rs`, and `oauth/device.rs` out
  of `oauth/grant.rs`.
- Final verification re-run after cleanup and the R2 fixes:
  `cargo clippy --workspace --all-targets` zero warnings;
  `CARGO_INCREMENTAL=0 cargo test --workspace` **297 passed, 0
  failed** (suite counts confirmed present; +2 CONDSTORE-negotiation
  regression tests over the pre-R2 count).
