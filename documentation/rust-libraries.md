# Rust Libraries — Ecosystem Review

> **Note (2026-07-24):** This file is historical research, not authority.
> Library choices follow [specification.md](specification.md) and
> [roadmap.md](roadmap.md); where this file disagrees with them (e.g. its
> async-imap recommendation), the specification wins. The same applies to
> every file in `documentation/` other than those two.

Comprehensive review of Rust crates for nitidus's feature set. All versions and
dates verified against the crates.io API and GitHub on 2026-07-23. The UI stack
(bevy 0.18 + bevy_ratatui + plurimus + ratatui 0.30 + ratatui-image

- tui-prompts) is covered in [ratatui-frameworks.md](ratatui-frameworks.md);
  this document covers everything behind the `MailBackend` trait and the domain
  layer, assuming a dedicated tokio runtime for mail I/O.

## Headline findings

1. **The Pimalaya ecosystem has pivoted.** `imap-client` was archived
   2026-05-17; email-lib 0.27 is in de-facto maintenance mode and depends on
   that archived client. All energy is in the new sans-IO **io-\*** generation
   (io-imap 0.2, 2026-07-15), which Himalaya v2 alpha already uses — but
   everything there is weeks-old 0.x. **Consequence: nitidus should NOT build
   its first backend on email-lib** (this supersedes the earlier plan's
   "email-lib first" assumption and effectively answers the M0 backend-fit
   spike).
2. **async-imap is the only actively maintained IMAP crate with verified Gmail
   X-GM parsing**, IDLE, a raw-command escape hatch, tokio, and XOAUTH2.
3. **No Rust MIME parser implements format=flowed (RFC 3676)** — plan to
   hand-roll that decoder (~small, well-specified) regardless of parser.
4. Stalwart's library crates (mail-parser, mail-builder, mail-send, calcard) are
   **Apache-2.0 OR MIT** — AGPL applies only to their server.

## 1. Email meta-frameworks

| Crate        | Version | Released   | Status                             |
| ------------ | ------- | ---------- | ---------------------------------- |
| email-lib    | 0.27.0  | 2026-02-19 | Works, but generationally orphaned |
| imap-client  | 0.3.1   | 2026-05-17 | **Archived** → pimalaya/io-imap    |
| io-imap      | 0.2.0   | 2026-07-15 | Active, pre-1.0, weeks old         |
| melib (meli) | 0.8.13  | 2026-01-05 | Mature alternative; **EUPL**       |

- email-lib is genuinely modular (23 feature flags; imap/maildir/notmuch/
  smtp/sendmail backends, oauth2, keyring, pgp, thread, sync, watch) but its
  IMAP backend sits on the archived imap-client, and Himalaya master dropped
  email-lib entirely.
- The io-\* rewrite is real: io-imap, io-smtp, io-maildir, io-jmap, io-gmail,
  io-msgraph, io-oauth, io-webdav. io-imap 0.2 is sans-IO with IDLE,
  CONDSTORE/QRESYNC, SORT/THREAD with client fallback, XOAUTH2 — but **no Gmail
  X-GM extensions** yet. NLnet-funded through 2026. `pimalaya/himalaya-tui`
  (ratatui-based, active) is both competitor and reference code.

**Recommendation**: go **direct-protocol** (async-imap + mail-parser + lettre +
own Maildir) as `MailBackend` implementation #1; keep the trait boundary so a
Pimalaya io-\* backend can be added when it stabilizes (re-evaluate in 6–12
months).

## 2. IMAP

| Crate             | Version                             | Released   | Verdict                             |
| ----------------- | ----------------------------------- | ---------- | ----------------------------------- |
| **async-imap**    | 0.11.3                              | 2026-07-17 | **Recommended**                     |
| imap (sync)       | 2.4.1 (2021); 3.0.0-alpha.15 (2025) |            | Stalled; seeking maintainers        |
| imap-codec/-types | 2.0.0-alpha.9                       | 2026-07-19 | Active, strictly typed, **no X-GM** |
| io-imap           | 0.2.0                               | 2026-07-15 | Watch                               |

async-imap (chatmail/Delta Chat; MIT/Apache) checks every requirement, verified:

- Production-backed, 4 releases in 12 months; `runtime-tokio` feature;
  bring-your-own-stream TLS → tokio-rustls slots in directly.
- IDLE via `Session::idle()` (0.11.2 fixed the 29-min re-issue timeout).
- Escape hatch: `run_command`, `run_command_untagged` + `read_response()`.
- Gmail: imap-proto parses **X-GM-LABELS / X-GM-MSGID / X-GM-THRID**; async-imap
  0.11.2 added `Fetch::gmail_labels()` / `gmail_msg_id()`. Gap: no
  `gmail_thrid()` accessor yet though the parser produces it — small upstream
  PR. X-GM-RAW works via raw SEARCH.
- XOAUTH2 via `Client::authenticate` with an `Authenticator` impl.

imap-codec 2.0-alpha is excellent engineering (typed, fuzzed) but has no X-GM
support and no escape hatch for unknown FETCH attributes — Gmail responses
likely become parse errors. Disqualifying today.

## 3. MIME parse/build

| Crate            | Version | Released   | License                    |
| ---------------- | ------- | ---------- | -------------------------- |
| **mail-parser**  | 0.11.5  | 2026-07-08 | Apache-2.0/MIT             |
| **mail-builder** | 0.4.4   | 2025-08-12 | Apache-2.0/MIT             |
| mailparse        | 0.16.1  | 2025-02-27 | 0BSD; slow-moving fallback |
| tnef             | 0.1.1   | 2019       | Dormant; attachments-only  |

- **mail-parser** (Stalwart): zero-copy, RFC 2045-2049 + encoded-words + RFC
  2231, nested multipart, 41 charsets incl. UTF-7, fuzzed. Bonus:
  `Message::thread_name()` implements RFC 5256 base-subject extraction — feeds
  JWZ threading directly. Gotcha: no format=flowed (hand-roll).
- **mail-builder**: RFC 5322 generation, multipart/alternative, automatic CTE
  selection, zero required deps.
- **TNEF/winmail.dat**: no maintained pure-Rust option. Detect
  `application/ms-tnef` and extract attachments with vendored `tnef` (~437 LOC);
  escalate to ytnef FFI only if MAPI properties become necessary.

## 4. SMTP

| Crate      | Version           | XOAUTH2 | Notes                                                 |
| ---------- | ----------------- | ------- | ----------------------------------------------------- |
| **lettre** | 0.11.22 (2026-05) | Yes     | 13.5M downloads; DSN PR open; ships SendmailTransport |
| mail-send  | 0.6.1 (2026-07)   | Yes     | Leaner; pairs natively with mail-builder              |

**Recommendation**: lettre 0.11 (`tokio1`, `tokio1-rustls-tls`) — the
conservative choice, and it covers the sendmail transport requirement too.

## 5. Maildir + file watching

- maildir 0.6.4 (crates.io release 2023, repo alive) and maildirs 0.2.2 (quiet 2
  years) are both under ~1k LOC.
- **Recommendation: hand-roll** (~500 LOC): the spec (unique name in `tmp/`,
  link to `new/`, rename into `cur/` with `:2,` flags) is tiny; owning it
  controls flag semantics and fits the `MailBackend` trait.
- **notify 8.2.0** + **notify-debouncer-full 0.7.0** (9.0-rc imminent): the
  standard answer. Gotchas: watch each folder's `new/` and `cur/`
  **non-recursively** (inotify allocates per directory); debounce because
  maildir delivery emits tmp-write + rename pairs.

## 6. OAuth2

- **oauth2 5.0.0** (ramosbugs) — recommended; all nitidus flows verified in the
  v5 API: Device Authorization Grant RFC 8628 (Microsoft), Auth Code + PKCE
  (Google installed-app), `exchange_refresh_token`; typestate client makes
  misconfiguration a compile error; async via reqwest. One code path for both
  providers.
- yup-oauth2 12.1.2: has both flows + free disk token caching but is
  Google-focused (no documented Microsoft device-flow path). Fallback for Google
  only.
- Token cache is ours to build (refresh token + expiry into the keyring /
  credential command) — trivial.

## 7. Secrets

- **keyring 4.1.5** (2026-07) — recommended. v4 restructure: API in keyring-core
  1.0; platform backends as separate store crates. Gotchas: the API is
  synchronous — **wrap calls in `spawn_blocking`** (Linux blocks on D-Bus);
  gnome-keyring issues in headless environments.
- Credential-command support (pass, etc.) is a small `tokio::process` shell-out
  we own, as aerc/himalaya do.

## 8. HTML → text / sanitization (tier 1)

- **html2text 0.17.1** (jugglerchris, active) — recommended. Verified rich API:
  `config::rich()` → `TaggedLine`s of spans tagged with `RichAnnotation` (links,
  emphasis, colors) — maps 1:1 onto ratatui `Span`/`Style`. `css` feature parses
  `<style>` + inline styles; real table layout with width-aware wrapping.
- **ammonia 4.1.4** (2026-07-22, servo html5ever) — recommended sanitizer.
  Strips script/style with contents; **`attribute_filter` callback** is exactly
  the hook for blocking/rewriting remote `img src` (remote-content stripping
  with a load-remote toggle); `url_schemes` allowlist covers `mailto:`/`cid:`
  policy.
- Reader mode (optional): dom_smoothie 0.18 (2026-06) is the live
  Mozilla-readability port; the old readability crates are dead.
- fast_html2md: wrong output shape (Markdown). Skip.

## 9. Headless Chromium / CDP (tier 3)

| Crate             | Version          | Model                                             |
| ----------------- | ---------------- | ------------------------------------------------- |
| **chromiumoxide** | 0.9.1 (2026-02)  | async, **tokio-only as of 0.9**                   |
| headless_chrome   | 1.0.22 (2026-06) | sync, thread-based                                |
| cdp-html-shot     | 0.2.4            | 1-star hobbyist; copy its API shape, don't depend |

chromiumoxide stalled after 0.7 but revived (0.8 Nov 2025, 0.9 Feb 2026) and
went tokio-only — matching nitidus's runtime exactly. `Page::screenshot`
(png/jpeg/webp), element capture, `fetcher` feature auto-downloads pinned
Chromium. **Recommendation: chromiumoxide**; fallback headless_chrome via
`spawn_blocking`.

## 10. Calendar / iTIP

- **calcard 0.3.7** (Stalwart, 2026-07, Apache/MIT) — recommended parser: iCal +
  vCard + JSCalendar, Postel's-law parsing that handles Outlook-style
  proprietary VTIMEZONEs (the differentiator for real Exchange invites), RRULE
  expansion.
- icalendar 0.17.12 — fallback / ergonomic typed REPLY building
  (Attendee/PartStat). `ical` crate: archived 2024 — rule out.
- **No crate implements RFC 5546 iTIP workflow** (UID/SEQUENCE matching, minimal
  REPLY with only the responding ATTENDEE, CANCEL handling) — that's a few
  hundred lines we own.
- TZ gotcha: calcard/icalendar are chrono + chrono-tz — convert at the calendar
  boundary even if the app standardizes on jiff.

## 11. Contacts / CardDAV

- **vCard**: **calcard** (already in for calendars) is the pragmatic single
  dependency — tolerates the 3.0-isms real CardDAV servers (Google, iCloud)
  still emit, converts vCard↔JSContact. vcard4 0.7.3 is the strict-RFC 6350
  alternative (parse + `VcardBuilder`, zeroization). vcard_parser 0.2.3 (used by
  vcard_tui) is smaller and parse/validate focused — nitidus builds on calcard
  directly (no vcard_tui code is imported).
- **CardDAV**: **libdav 0.10.6** (2026-06, **ISC**, whynothugo/pimsync — the
  vdirsyncer successor) — recommended: CalDAV+CardDAV over hyper 1 + tokio +
  rustls, address-book discovery (well-known/SRV), resource fetch. Gotcha: RFC
  6578 sync-collection not yet released — sync by ETag comparison for now (fine
  at address-book scale; upstream work in progress). kitchen-fridge:
  CalDAV-only, dormant. fast-dav-rs: has RFC 6578 but LGPL-3.0. Fallback:
  hand-roll REPORT/PROPFIND with quick-xml + reqwest.

## 12. PGP

- gpgme bindings: dormant since 2022, fragile builds — avoid.
- sequoia-openpgp 2.4: excellent but **LGPL** and doesn't give gpg's trust-DB
  semantics.
- pgp (rPGP) 0.20: MIT/Apache, very active, pure Rust — but a format library (no
  keyring/agent/trust). Right tool for a _future_ native mode.
- **Recommendation: shell out to the `gpg` binary with `--status-fd`/`--batch`**
  (the aerc/mutt model; himalaya's `pgp-commands` backend). ~100–200 lines of
  GOODSIG/VALIDSIG/ DECRYPTION_OKAY parsing (aerc's Go code is a direct
  reference); exact gpg keyring/pinentry semantics; zero license entanglement.
  Behind a trait so rPGP can become a native backend later.

## 13. Storage / cache (100k+ envelopes)

| Crate        | Version          | Assessment                                                  |
| ------------ | ---------------- | ----------------------------------------------------------- |
| **rusqlite** | 0.40.1 (2026-06) | **Recommended** — bundled SQLite 3.53, `fts5` feature       |
| redb         | 4.1.0 (2026-04)  | Healthy, but v2→v3→v4 file-format churn within a year       |
| fjall        | 3.1.8 (2026-07)  | Active LSM; young, no named production users                |
| native_db    | 0.8.2            | On redb; API unstable; double migration risk                |
| sled         | 0.34.7 (2024)    | Perpetual beta; own README says "use SQLite". **Rule out.** |

**Recommendation: SQLite via rusqlite** (`bundled` + `fts5`). Envelope data is
relational (headers, flags, mailbox/date scans → indexes), sync state is
transactional with it, FTS5 lives in the same file, the format is eternally
stable, flag updates are trivial. Async pattern: tokio-rusqlite 0.7 (dedicated
connection thread) or `spawn_blocking` on the mail runtime. Envelope blobs
inside SQLite: rmp-serde; cache keys: blake3.

## 14. Full-text search

- **SQLite FTS5** — free once rusqlite is chosen: one file, transactional with
  envelopes, external-content tables avoid duplicating bodies. Weak CJK
  tokenization.
- tantivy 0.26.1 — alive post-Quickwit/Datadog; real power, real cost (separate
  index dir ≈ indexed text size). Graduate to it only if ranking/CJK/fuzzy
  outgrow FTS5.
- notmuch bindings 0.8 — semi-dormant, system libnotmuch, **GPL-3.0+ (linking
  contaminates the binary)**. Prefer shelling out to the `notmuch` CLI (no
  linking, no license coupling), or an off-by-default cargo feature clearly
  documented as producing a GPL build.

## 15. JWZ threading

- **mail-threading 0.1.3** (2026-06, MIT/Apache): JWZ + RFC 5256
  THREAD=REFERENCES with phantom containers, pruning, subject fallback — but
  brand new, ~487 LOC, 0 stars. Nothing else exists.
- **Recommendation: implement JWZ from the spec in one file**, using
  mail-parser's `message_id()`/`in_reply_to()`/`references()`/ `thread_name()`
  as inputs; vendoring mail-threading as a reference is equally defensible. No
  battle-tested incumbent either way.

## 16. Utility belt (verified alive)

| Crate                    | Version / date   | Note                                                                                                                                                                             |
| ------------------------ | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| tokio                    | 1.53.1 / 2026-07 | —                                                                                                                                                                                |
| tokio-util               | 0.7.19           | CancellationToken for job cancellation                                                                                                                                           |
| flume                    | 0.12.0 / 2025-12 | The bevy↔mail-runtime bridge                                                                                                                                                     |
| serde                    | 1.0.229          | —                                                                                                                                                                                |
| toml                     | 1.1.3 / 2026-07  | Use toml+serde directly; **skip figment** (dormant since 2024)                                                                                                                   |
| crokey                   | 1.4.0 / 2026-02  | Keybinding parsing; pins crossterm 0.29 = ratatui 0.30's default — compatible                                                                                                    |
| nucleo-matcher           | 0.3.1            | Helix's fuzzy engine; MPL-2.0 (file-level, safe to link). Prefer over fuzzy-matcher (dead 2020)                                                                                  |
| textwrap                 | 0.16.2           | —                                                                                                                                                                                |
| unicode-width            | 0.2.2            | ratatui also depends on it                                                                                                                                                       |
| blake3                   | 1.8.5            | Cache hashing                                                                                                                                                                    |
| rmp-serde                | 1.3.1            | msgpack blobs                                                                                                                                                                    |
| url                      | 2.5.8            | Parses `mailto:` (~30 lines of glue); ignore the `mailto` crate (it's a CLI)                                                                                                     |
| list-unsubscribe         | 0.1.3 / 2026-06  | RFC 2369 + 8058 parsing into typed actions; tiny/new — vet or vendor                                                                                                             |
| tracing                  | 0.1.44           | —                                                                                                                                                                                |
| thiserror / anyhow       | 2.0.19 / 1.0.104 | thiserror in lib crates, anyhow at app edge (per project rules)                                                                                                                  |
| etcetera                 | 0.11.0           | Prefer over dirs for explicit XDG strategy                                                                                                                                       |
| image                    | 0.25.10          | Needed by ratatui-image anyway                                                                                                                                                   |
| uuid                     | 1.24.0           | Contact UIDs                                                                                                                                                                     |
| **jiff**                 | 0.2.34 / 2026-07 | **Prefer over chrono for app time**: `fmt::rfc2822` accepts obsolete offsets (EST, military zones) in real mail headers; embedded IANA tzdb. chrono only at the calcard boundary |
| notify (+debouncer-full) | 8.2.0 / 0.7.0    | §5                                                                                                                                                                               |
| rustls / tokio-rustls    | 0.23.42 / 0.26.4 | TLS everywhere                                                                                                                                                                   |
| quick-xml                | 0.41.0           | Only if hand-rolling WebDAV                                                                                                                                                      |
| rfc2047-decoder          | —                | Unneeded — mail-parser handles encoded-words                                                                                                                                     |

## Gmail / Microsoft Graph REST clients (post-MVP backends)

- graph-rs-sdk 3.0.1 (MIT): broad coverage incl. device-code OAuth, but last
  commit 2025-08 — slowing. Watch.
- google-apis-rs (`google_gmail1`): generator in maintenance mode, seeking a
  maintainer; generated crates clunky.
- **Recommendation**: hand-roll Gmail REST (messages.list/get RAW, history.list,
  labels, send) and Graph mail endpoints with reqwest + serde + the same oauth2
  tokens — a few hundred lines each. Pimalaya's io-gmail / io-msgraph may mature
  into this role.

## License flags (verified)

- **GPL-3.0+**: notmuch bindings and libnotmuch — CLI shell-out or opt-in
  feature only.
- **LGPL**: sequoia-openpgp, gpgme, fast-dav-rs — all avoided in the recommended
  stack.
- **MPL-2.0**: nucleo-matcher — safe to link.
- **EUPL-1.2**: melib, vstorage — avoided.
- **ISC**: libdav — fine.
- Stalwart libraries: Apache-2.0 OR MIT (AGPL is server-only).

## Recommended stack (summary)

| Concern        | Primary                                                     | Fallback                 |
| -------------- | ----------------------------------------------------------- | ------------------------ |
| Meta-framework | None — direct protocol crates behind `MailBackend`          | Pimalaya io-\* (~2027)   |
| IMAP           | async-imap 0.11 + tokio-rustls                              | io-imap 0.2              |
| MIME parse     | mail-parser 0.11 + own format=flowed + vendored tnef        | mailparse                |
| MIME build     | mail-builder 0.4                                            | lettre builder           |
| SMTP/sendmail  | lettre 0.11                                                 | mail-send 0.6            |
| Maildir        | hand-rolled + notify 8                                      | maildir 0.6              |
| OAuth2         | oauth2 5.0 (device-code + PKCE + refresh)                   | yup-oauth2 (Google)      |
| Secrets        | keyring 4.1 (spawn_blocking) + credential-command shell-out | secret-service           |
| HTML tier 1    | html2text 0.17 (css) → ratatui spans                        | custom on dom_query      |
| Sanitizer      | ammonia 4.1                                                 | —                        |
| HTML tier 3    | chromiumoxide 0.9                                           | headless_chrome          |
| iCal/iTIP      | calcard + own iTIP REPLY (~300 LOC)                         | icalendar                |
| vCard          | calcard                                                     | vcard4                   |
| CardDAV        | libdav 0.10 (ETag sync)                                     | quick-xml + reqwest      |
| PGP            | gpg shell-out (--status-fd), behind a trait                 | rPGP later               |
| Envelope cache | rusqlite (bundled, fts5) + rmp-serde + blake3               | redb 4                   |
| Search         | SQLite FTS5                                                 | tantivy; notmuch via CLI |
| Threading      | JWZ from spec on mail-parser accessors                      | vendor mail-threading    |
| Config         | toml + serde; crokey                                        | —                        |
| Fuzzy          | nucleo-matcher                                              | —                        |
| Time           | jiff (rfc2822); chrono at calendar boundary                 | chrono                   |

## Sources

crates.io API records for every crate listed; qualitative verification against:
pimalaya org repos (io-imap, himalaya v2 Cargo.toml, imap-client archive
notice), async-email/async-imap (CHANGELOG, fetch types), djc/tokio-imap
(imap-proto gmail.rs), duesee/imap-codec, stalwartlabs repos + stalw.art
licensing, lettre docs + issue #1147, staktrace repos, ramosbugs/oauth2-rs v5
docs, open-source-cooperative/keyring-rs, jugglerchris/rust-html2text,
rust-ammonia/ammonia, mattsse/chromiumoxide, hoodie/icalendar, tmpfs/vcard4,
sr.ht/~whynothugo/libdav + status updates, rpgp/rpgp, cberner/redb CHANGELOG,
spacejam/sled README, rusqlite releases, quickwit-oss/tantivy + Datadog
acquisition posts, vhdirk/notmuch-rs, sreeise/graph-rs-sdk,
Byron/google-apis-rs, Canop/crokey, helix-editor/nucleo, docs.rs/jiff.
