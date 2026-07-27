# refactor - Himalaya Sync - v1

Replace the hand-rolled Maildir backend with Pimalaya's `io-maildir`, and adopt
a standing position on the rest of the Pimalaya ecosystem: which of their crates
nitidus tracks today, which it should track next, and which it should keep out.
The goal is to stay inline with Himalaya as they develop, so their protocol work
lands here as version bumps instead of as parallel implementations.

## 1. Current Design

### 1.1 What we already take from Pimalaya

| Crate      | We pin | Latest | Himalaya 2.0.0          |
| ---------- | ------ | ------ | ----------------------- |
| `io-imap`  | 0.2.0  | 0.3.1  | `^0.3`                  |
| `io-smtp`  | 0.2.0  | 0.2.3  | `^0.2`                  |
| `io-oauth` | 0.2.0  | 0.2.0  | not a direct dependency |
| `io-http`  | 0.3.0  | 0.3.0  | not a direct dependency |

We are one minor version behind on `io-imap` and three patches behind on
`io-smtp`. Both pin `default-features = false`, which matters more than it looks
— see §2.2. We supply the stream/TLS layer ourselves through `net.rs` (89 lines)
on `rustls`/`tokio-rustls`, async over tokio.

The integration pattern is already the one these crates expect. They are
I/O-free: `io-imap` hands us `ImapCoroutine`/`ImapYield` state machines and we
drive them against our own sockets — `imap/session.rs` (262 lines) plus
`net.rs`. We use a wide slice of that surface:
`rfc3501::{append, create, delete, expunge, fetch, greeting, list, login, raw, rename, search, select, status, starttls}`,
`rfc2177::idle`, and the raw escape hatch.

### 1.2 The maildir backend we would replace

`nitidus-mail/src/maildir/` is 701 lines, ~484 of them non-comment code:
`backend.rs` (173), `message.rs` (134), `folder_ops.rs` (98), `folders.rs` (73),
`mod.rs` (6). Around it sit 135 lines of inline unit tests and a 279-line
integration suite. `envelope.rs` (76 lines) is the shared header summarizer used
by both backends and is not maildir-specific; the notify-based watching lives at
engine level in `watch.rs`, deliberately, because a long-running watch cannot
sit inside a `&mut backend` method.

It was hand-rolled on a recorded recommendation
(`documentation/rust-libraries.md` §5, deleted in `c40060a`, readable at
`c40060a^`): the candidate crates were small and quiet, the format is ~500 LOC,
and owning it controlled flag semantics. That reasoning predates `io-maildir`
existing — its first release was 2026-06-05, well after the decision.

`io-maildir` 0.2.0 (2026-07-16) depends only on `log`, `thiserror`, and
optionally `gethostname` and `mail-parser` 0.11 — the same parser we already
use. It carries no runtime, so it slots into the same drive-it-yourself pattern
as `io-imap`.

### 1.3 The ecosystem, as it stands today

Queried from crates.io on 2026-07-27. Himalaya 2.0.0 is the reference consumer.

**Mail backends (the io-\* generation):**

| Crate        | Latest | First → last release      | Rel | Himalaya 2.0.0 |
| ------------ | ------ | ------------------------- | --- | -------------- |
| `io-imap`    | 0.3.1  | 2026-06-03 → 2026-07-25   | 4   | `^0.3` (opt)   |
| `io-smtp`    | 0.2.3  | 2026-06-03 → 2026-07-25   | 5   | `^0.2` (opt)   |
| `io-maildir` | 0.2.0  | 2026-06-05 → 2026-07-16   | 2   | `^0.2` (opt)   |
| `io-m2dir`   | 0.2.0  | 2026-06-05 → 2026-07-16   | 2   | `^0.2` (opt)   |
| `io-jmap`    | 0.2.1  | 2026-06-05 → 2026-07-25   | 3   | `^0.2` (opt)   |
| `io-gmail`   | 0.2.2  | 2026-07-15 → 2026-07-25   | 3   | `^0.2` (opt)   |
| `io-msgraph` | 0.2.1  | 2026-07-15 → 2026-07-25   | 3   | `^0.2` (opt)   |
| `io-webdav`  | 0.1.0  | 2026-07-17 (only release) | 1   | no             |

**Supporting crates:**

| Crate              | Latest | Note                                               |
| ------------------ | ------ | -------------------------------------------------- |
| `io-oauth`         | 0.2.0  | OAuth flows; we use it directly                    |
| `io-http`          | 0.3.0  | HTTP client under the REST backends                |
| `io-pim-discovery` | 0.3.3  | Autoconfig/autodiscover; Himalaya depends on it    |
| `pimalaya-stream`  | 0.1.2  | Stream, TLS and SASL utils; `io-imap` 0.3 needs it |
| `pimalaya-config`  | 0.1.1  | Config utils                                       |
| `pimalaya-cli`     | 0.1.3  | CLI utils — not for us                             |
| `io-keyring`       | 0.0.2  | Last released 2025-09-11                           |
| `io-process`       | 0.0.2  | Last released 2025-09-11                           |
| `io-stream`        | 0.0.2  | Last released 2025-08-03                           |

The whole io-\* mail family is weeks to months old and pre-1.0, releasing
frequently — `io-imap`, `io-smtp`, `io-jmap`, `io-gmail` and `io-msgraph` all
shipped on the same day (2026-07-25), which reads as coordinated releases. The
three `io-{keyring,process,stream}` coroutine crates are the older 0.0.x
generation and have been quiet for ~10 months.

**The previous generation**, superseded but still published: `email-lib`
(0.27.0), `mml-lib` (1.1.2), `secret-lib` (1.0.0), `process-lib` (1.0.0),
`keyring-lib` (1.0.3), `oauth-lib` (2.0.0), `pgp-lib` (1.0.0), `pimalaya-tui`
(0.3.1). Himalaya has dropped all of them. There is no `io-notmuch`, `io-pgp`,
`io-mml` or `io-carddav` — those capabilities live only in the older `-lib`
crates or not at all.

## 2. Proposal

### 2.1 Swap the maildir backend

`MaildirBackend` keeps its `MailBackend` implementation and its place in the
actor; `maildir/message.rs` and `maildir/folders.rs` give way to `io-maildir`
calls, driven the way `imap/session.rs` drives `io-imap`. `folder_ops.rs`
survives to whatever extent `io-maildir` does not cover create/delete/rename.
`envelope.rs` stays shared, and `watch.rs` is untouched — `io-maildir` carries
no watching and should not.

Behavior is unchanged, with one caveat worth stating plainly: this swaps our
`:2,` flag handling and Maildir++ dot-name decoding for theirs. Any difference
in edge-case semantics — flag ordering, unusual folder names, `new/` vs `cur/`
placement on flag change — is a behavior change we would be accepting sight
unseen. The existing 279-line integration suite is what proves it, and it stays
as the contract.

### 2.2 Catch up to Himalaya's versions

`io-imap` 0.2 → 0.3 and `io-smtp` 0.2.0 → 0.2.3, so we sit where Himalaya sits.

The version bump itself is small. The API surface we consume is unchanged except
for `io_imap::watch`, where 0.3.0 renamed the `ImapMailboxWatchError` `Select*`
variants to `Examine*` and switched the watcher from SELECT to EXAMINE so it
stops resetting `\Recent` on every re-open. 0.3.0 also fixes `ImapMessageMove`
losing `COPYUID` when the server reports it in an untagged `OK` (Fastmail does),
and fixes the client-side SORT date ordering.

**Correction, from implementing Phase 1.** The claim that the rename is "our one
compile break" was wrong: we do not use `io_imap::watch` at all. `imap/watch.rs`
drives `rfc2177::ImapIdle` directly and issues its own
`rfc3501::select::ImapMailboxSelect` (`watch.rs:136`), so nothing we compile
names `ImapMailboxWatchError`. The bump is a clean no-op at the source level.

The consequence is that the EXAMINE improvement is **not** inherited. Their
watcher stopped resetting `\Recent`; ours still issues SELECT and still resets
it. `rfc3501::examine::ImapMailboxExamine` exists in 0.3.1 with the same
`new(mailbox, opts)` shape as `ImapMailboxSelect`, so the switch is a small one —
but it is a behavior change, and principle 5 keeps behavior changes out of a
version bump. Deferred to roadmap item 36 (session hardening), which already
owns the IDLE/session-hygiene surface.

**The `pimalaya-stream` premise in R1 Q3 was wrong, in two ways.** It is not a
hard dependency of `io-imap` 0.3: it is optional, reached only through the
`client`/`rustls-*`/`native-tls` features, and our `default-features = false` pin
means 0.3 does not pull it in. It is in the tree regardless: `cargo tree -i`
resolves `pimalaya-stream 0.1.0` through `io-http 0.3.0` → `io-oauth 0.2.0` →
`nitidus-mail`, because `io-http` is declared without `default-features = false`.
Nothing calls it — `oauth/mod.rs` imports only the coroutine types and drives
them against our own async `RemoteStream`. So the choice is not whether to carry
the dependency; we already compile it. And `pimalaya-stream` 0.1.2 is
**blocking-only**: its own docs say the `std` module "is the blocking runtime
layer" and "a future async runtime would gain a sibling module (tokio) next to
it". Its `src/` has no `async fn` at all. `StreamStd::connect_tcp` /
`connect_tls` / `upgrade_tls` all return a synchronous `Read + Write` handle.

Our IMAP and SMTP pumps are async tokio end to end (`imap/session.rs` drives
coroutines against an async `RemoteStream`). Adopting `pimalaya-stream` therefore
is not "delete most of `net.rs`" — it is converting both remote transports to
blocking sockets behind `spawn_blocking`, including IDLE, which currently parks
on an async read and would have to become a blocking read with timeouts. That is
a transport rewrite with real risk to the one path that works today, in exchange
for deleting 89 lines. §3.3 R2 Q1 re-asks the question on the corrected facts.

### 2.3 A standing position on the rest

Adopt now, with the maildir swap:

- `io-maildir` — this refactor.
- `pimalaya-stream` — Phase 5, per R2 A1. Blocking-only today, so adopting it
  converts every remote transport; taken as an alignment decision rather than a
  line-count one.

Adopt when the feature that needs it arrives, mapped to the roadmap:

- **`io-gmail`** and **`io-msgraph`** — Phase 3 provider fidelity. The roadmap
  already names both as alternatives to hand-rolled REST clients; they now exist
  and are actively released, which settles that question in their favour.
- **`io-pim-discovery`** — autoconfig for the account wizard. Not a roadmap item
  today, but it is what Himalaya uses to avoid asking users for host names and
  ports, and our wizard asks for all of them.
- **`io-webdav`** — Phase 5 CardDAV sync, listed in the roadmap as "io-webdav /
  libdav". One release, 18 downloads: revisit when Phase 5 starts, not before.
- **`io-jmap`** — no roadmap item. JMAP is the protocol Fastmail speaks; worth
  knowing exists, worth nothing until someone wants it.

Keep out:

- **`io-m2dir`** — m2dir is a _different storage format_, not an improvement to
  Maildir. Supporting it means supporting a second on-disk layout, and it does
  nothing for the mbsync/offlineimap compatibility we promise. (This also
  answers `chore-spec-sync-v1` R1 Q3: the `(+ io-m2dir)` parenthetical in the
  spec is simply wrong.)
- **`pimalaya-cli`**, **`pimalaya-config`** — CLI and config utilities shaped
  for Himalaya's own CLI; nitidus has its own config layer with `toml_edit` and
  a TUI, not a CLI.
- **The `-lib` generation** — `email-lib` and friends are what the io-\* crates
  replaced. Nothing to gain.
- **`io-keyring`**, **`io-process`**, **`io-stream`** — the 0.0.x coroutine
  crates, quiet for ~10 months. We already have working equivalents
  (`keyring-core` + the zbus store, plain `std::process`, and `net.rs` until
  Phase 5 replaces it), and 0.0.2 is not a version to bet a working feature on.
(`pimalaya-stream` was listed here until R2 A1 moved it to "adopt now" — see
§3.5 and Phase 5.)

### 2.4 The standing policy this implies

Track Himalaya's version requirements for every io-\* crate we share with them,
and treat their releases as our upgrade cadence rather than upgrading only when
something breaks. These crates are pre-1.0 and coordinate their releases, so
drifting a minor version behind — as we have on `io-imap` — is how the "stay
inline" goal quietly stops being true.

## 3. Discussion

### 3.1 R1 Questions

1. **One doc or two?** The maildir swap and the `io-imap` 0.3 upgrade are
   independent: different crates, different risks, and the version bump touches
   the working IMAP path while the swap touches the local one. Do them in one
   refactor, or split the version catch-up into its own doc and keep this one to
   maildir?
2. **How much behavior drift is acceptable?** §2.1 is honest that their `:2,`
   and Maildir++ semantics may differ from ours in edge cases. Is "the
   integration suite still passes" the bar, or do you want a written comparison
   of their flag handling against ours before the swap?
3. **`pimalaya-stream`.** `io-imap` 0.3 depends on it. Adopt it and delete most
   of `net.rs` (more Pimalaya surface, less of our code), or keep `net.rs` and
   let `pimalaya-stream` sit as an unused transitive dependency?
4. **`io-pim-discovery`.** Autoconfig would remove most of the account wizard's
   questions. It is not on the roadmap. Add it as a Phase 2 roadmap line, open a
   feature doc for it, or leave it noted here and move on?
5. **What if `io-maildir` is missing something?** It is two releases old with
   420 downloads, and I have not read its API in depth — only its dependency
   graph and that it is I/O-free. If it turns out not to cover folder
   create/delete/rename, or not to expose flags the way `set_flags` needs, do we
   keep a thin hand-rolled layer alongside it, contribute upstream, or abandon
   the swap?

### 3.2 R1 Answers

1. one doc
2. Let's keep to himilaya behavior as much as possible. I can remove my current
   accounts so we don't need to worry about migration. Also this project isn't
   used yet in the wild so we don't need to worry about version compatibility.
3. adopt it.
4. open a feature doc and branch
5. any missing features will be addressed after analysis, but definitely adopt.

Let's open feature docs + create branches for both io-gmail and io-msgraph as
well.

### 3.3 R2 Findings

R1 A5 asked for analysis before adoption. `io-maildir` 0.2.0 was read in full
(6175 lines across `src/`, plus its 397-line integration suite). It covers
everything the backend needs, and the swap is viable. The divergences below are
the complete list of places where its behavior differs from ours.

**The shape of the integration.** The crate is `no_std` with an optional
`client` feature giving `MaildirClient`, a std-blocking pump over `std::fs` —
the exact counterpart of `imap/session.rs`. `MaildirClient::run` takes `&self`,
so `MaildirBackend` holds one client and calls it inside the existing
`spawn_blocking`. We want `default-features = false, features = ["client"]`: the
`parser` and `serde` features only re-expose `mail-parser`, which `envelope.rs`
already owns.

**Divergences we accept** (R1 A2: keep to their behavior):

1. **Flag letters are not ASCII-sorted.** `MaildirFlags` is a
   `BTreeSet<MaildirFlag>`, so `Display` emits them in enum declaration order —
   `P, R, S, T, D, F`. Draft+Replied+Seen writes `:2,RSD` where we write
   `:2,DRS`. The Maildir convention is ASCII order, and our
   `flag_suffix_is_ascii_sorted` unit test asserts it. Readers (mbsync,
   offlineimap, mutt) parse the set order-insensitively, so this is cosmetic on
   disk — but it is a deviation from the format, and worth an upstream issue.
2. **Moves mint a new id.** `MaildirEntryMove` deliberately refuses to reuse the
   source basename, because it may carry folder-specific metadata such as
   mbsync's `,U=<uid>` infix. Ours preserves the filename. Theirs is correct;
   the consequence is that a moved message changes `EnvelopeId`, which the
   envelope cache keys on.
3. **Delivered ids change format.** Ours is `{nanos}.{pid}.nitidus`; theirs is
   `{secs}.#{counter:x}M{nanos}P{pid}.{hostname}`, the standard delivery
   convention with a process-wide counter shared across store/copy/move so two
   deliveries in one tick cannot collide. Strictly better. R1 A2 waives the
   migration concern.
4. **Flags are parsed after the last comma, not after `:2,`.** `MaildirFlags::
   from(&path)` uses `rsplit_once(',')`; we use `split_once(":2,")`. Both handle
   mbsync's `1234.host,U=5:2,S` identically. They differ only on a name with a
   comma and no info suffix, where theirs reads the trailing segment as flag
   letters and discards the unrecognised ones — same empty result, more log
   noise.
5. **`P` (Passed) is dropped.** They model it; our `Flags` is a 5-bit set
   without it. Since `MaildirFlagsSet` replaces the whole set, a `P` written by
   another client is erased the first time we set flags on that message. Our
   IMAP backend has the same 5-flag vocabulary, so adding `Passed` is a
   `types.rs` change, not a maildir one — out of scope here, noted.

**Gaps where our code stays** — these are not missing features, they are
deliberate differences in scope:

6. **`create_maildir` is idempotent; `delete_maildir` is `remove_dir_all`.**
   Neither guards anything. Our `folder_ops.rs` refuses to create over an
   existing folder, refuses to delete a non-empty folder or one with children,
   and validates that folder-name components are non-empty and free of `.`
   before encoding. That file's header calls it out: "no destructive path exists
   here by design." `folder_ops.rs` survives as the validation layer in front of
   their coroutines rather than being replaced by them.
7. **No envelope scanning.** `MaildirEntryList` returns path-only handles
   (`BTreeSet<MaildirEntry>`), and the only body reader is `read_entry`, a full
   `fs::read`. We read a 64 KB header window and stream batches of 500 through a
   channel. `MaildirEntry::path()` is public, so we list through them and keep
   our own windowed read — batching, memory ceiling and streaming all preserved.
8. **No folder counts.** `FolderMeta.unread`/`total` have no counterpart; our
   `count_files` stays.
9. **`new/` vs `cur/` SEEN stripping.** We strip `SEEN` for anything in `new/`.
   `MaildirEntry` exposes no subdir accessor, but the parent directory name is
   right there in `path()`, so this is derivable at the listing site.

**The one genuine loss:**

10. **A store is either fs-layout or Maildir++, never both.** `MaildirList`
    filters on exactly that: in Maildir++ it keeps only dot-prefixed children,
    in fs layout it skips them as hidden. Our `discover()` accepts *any* child
    directory containing cur/new/tmp and only uses the dot for display decoding,
    so it lists `.Archive` and `Archive` alike. Since `folder_ops.rs` writes
    dot-names and the mbsync/offlineimap compatibility we promise is Maildir++,
    the plan sets `maildirpp = true` — which means a plain undotted child folder
    that we list today would stop appearing. R2 Q2.

### 3.4 R2 Questions

1. **`pimalaya-stream`, re-asked.** §2.2 corrects the premise your "adopt it"
   answered: `io-imap` 0.3 does not pull it in under our `default-features =
   false` pin (though `io-http` already does, unused), and it is blocking-only,
   so adopting it means converting the IMAP and SMTP transports — IDLE
   included — from async tokio to blocking sockets on `spawn_blocking`, to
   delete 89 lines. Keep `net.rs` and take `io-imap` 0.3 without it, or still
   convert?
2. **Undotted sibling folders.** Setting `maildirpp = true` drops folders like
   `<root>/Archive/` that lack the leading dot. Nothing nitidus creates looks
   like that, but a tree touched by another tool could. Accept the loss, or keep
   a fallback listing pass for undotted children?
3. **Flag ordering upstream.** Their non-ASCII letter order (finding 1) looks
   like an oversight rather than a decision. File an upstream issue, or leave
   it?

### 3.5 R2 Answers

1. **Convert.** Adopt `pimalaya-stream`. The reason is alignment, not the 89
   lines: nitidus should grow with the Pimalaya ecosystem and inherit what
   upstream adds rather than maintaining a parallel transport. Phase 5 is in
   scope.

   Recorded against the decision: the async runtime is the capability that
   alignment would most buy us, and it is the one that does not exist yet — 0.1.0
   ships only `std`, with tokio named as future work. So this converts to
   blocking now and likely converts back when the sibling module lands. Accepted
   as the cost of being early. This also reverses the `pimalaya-stream` non-goal
   in `roadmap-v2.md`, which is amended to match.

2. **Accept the loss.** `maildirpp = true`, listing dot-prefixed children only.
   It matches the mbsync/offlineimap compatibility we actually promise and keeps
   the swap a thin wrapper; no folder nitidus writes is undotted. A foreign
   undotted folder silently disappearing from the sidebar is the accepted cost.

3. **File it.** Open an upstream issue on the non-ASCII flag letter ordering
   (§3.3 finding 1).

## 4. Plan

Five phases. Each leaves the workspace compiling and the suite green. Phase 5 is
in scope per R2 A1 and runs last, so the transport conversion lands against a
maildir backend that is already settled.

### Phase 1 — Version catch-up

Bump `io-imap` to `0.3` and `io-smtp` to `0.2.3` in the workspace manifest, both
keeping `default-features = false`. The expected compile break is
`ImapMailboxWatchError::Select*` → `Examine*` in the watch path; the rest of the
surface we consume is unchanged. Inherit the EXAMINE switch, the `COPYUID`
fix and the SORT date fix without code changes.

Green criterion: `cargo clippy --workspace` clean, full suite green, IMAP
integration tests unchanged.

### Phase 2 — Introduce `io-maildir` behind the existing backend

Add `io-maildir = { version = "0.2", default-features = false, features =
["client"] }`. `MaildirBackend` gains an `Arc<MaildirClient>` built from its
root with `store.maildirpp = true`, alongside the current code. Port folder
listing first: `list_maildirs()` plus `store.relative()` to recover logical
names, with the empty path mapping to `INBOX`. `count_files` stays for
`FolderMeta`. `folders::discover` and `display_name` go; `folder_dir` becomes
`store.resolve`.

Green criterion: `tests/maildir.rs` folder cases pass untouched.

### Phase 3 — Port the message operations

- `scan_envelopes` — `MaildirEntryList` for the handles, our windowed
  `parse_envelope` per file, `SCAN_BATCH_SIZE` batching preserved; derive
  `in_new` from the entry's parent directory.
- `fetch_message` — `MaildirEntryLocate` for the path, then our read.
- `set_flags` — `MaildirFlagsSet`, mapping our `Flags` to `MaildirFlags`.
- `append_message` — `client.store(...)` into `Cur`.
- `move_message` — `MaildirEntryMove`.
- `delete_message` — `MaildirEntryLocate` + `fs::remove_file`.

`message.rs` is deleted at the end of this phase; `envelope.rs` and `watch.rs`
are untouched throughout.

Green criterion: the full 279-line `tests/maildir.rs` passes. The two unit tests
in `message.rs` that assert our own `:2,` encoding die with the file — the
integration suite is the contract that survives, per §2.1. Expect one assertion
change for finding 1 (flag order) and any id-format assertions from finding 3.

### Phase 4 — Reduce `folder_ops.rs` to a guard layer

`create`/`delete`/`rename` keep their validation and refusal rules and delegate
the filesystem work to `MaildirCreate`/`MaildirDelete`/`MaildirRename`.
`encode_dot_name` goes — `MaildirStore::resolve` does that encoding — but its
validation does not, so it stays as a name check.

Green criterion: folder create/delete/rename cases in `tests/maildir.rs` pass,
including the refusal cases.

### Phase 5 — `pimalaya-stream`

Per R2 A1. Scoped here on the corrected facts; R2 Q1's framing of "the IMAP and
SMTP transports" understated it.

`RemoteStream` has seven consumers, not two: `imap/session.rs` (262),
`imap/pump.rs` (62), `send/smtp.rs` (176), `send/pump.rs` (51), `oauth/mod.rs`
(263) and `oauth/grant.rs` (234) name it directly, and `imap/watch.rs` (223)
reaches it through `Connection.stream` (`session.rs:39`). OAuth is
the one the earlier framing missed, and it decides the phase's shape: `net.rs`
cannot be deleted while any consumer still wants an `AsyncRead + AsyncWrite`,
so a conversion that stops at IMAP and SMTP keeps `net.rs` alive for OAuth and
buys nothing. The 89 lines only go if all seven move.

Two sub-steps, each leaving the suite green:

- **5a — the sync path.** `imap/session.rs`, `imap/pump.rs`, `send/*` and
  `oauth/*` move to `StreamStd` inside the `spawn_blocking` regions that already
  wrap them. Every one of these is request/response, so the conversion is
  mechanical: an async `read`/`write` pair becomes the blocking equivalent.
- **5b — IDLE.** The genuinely risky one. `watch.rs:190` parks on
  `tokio::time::timeout(IDLE_READ_TIMEOUT, stream.read(..))`, and `StreamStd`
  offers no such combinator, so the refresh window has to be re-expressed as a
  socket read timeout with the same backoff and the same `WatchEnd::Failed`
  behavior on elapse. `ImapIdle`'s shutdown `Arc` must still interrupt a parked
  read.

Green criterion: full suite green, clippy clean, `net.rs` deleted, and a live
IDLE smoke against `norman.kerr.dev` confirming both a real `FolderChanged` and
a clean timeout-and-reconnect cycle.

Sequenced last: it is the only phase that risks the working remote path, and it
is independent of the maildir swap above it.

## 5. Verification

### Phase 1

Resolved `io-imap 0.2.0 → 0.3.1` and `io-smtp 0.2.0 → 0.2.3`, both still
`default-features = false`; `base64 0.23.0` came in as a new transitive.

- `cargo build --workspace` — clean, no compile break (see the §2.2 correction).
- `cargo clippy --workspace` — clean, no warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **597 passed, 0 failed, 0
  ignored** across 20 binaries.

No source file changed; the diff is two manifest lines plus `Cargo.lock`.

### Phases 2–5

_Pending implementation._

## 6. Implementation Report

### Phase 1

Landed as specified, and cheaper than predicted: the one anticipated compile
break does not exist, because we never used `io_imap::watch`. The manifest bump
alone was the whole change.

Two follow-ups fall out of it:

1. **EXAMINE is not inherited** (§2.2). Our watcher still issues SELECT and
   still resets `\Recent` on every re-open. Switching to
   `rfc3501::examine::ImapMailboxExamine` is a behavior change, so it goes to
   roadmap item 36, not here.
2. **`io-smtp` was pinned `"0.2"`,** which already permitted 0.2.3 under Cargo's
   caret semantics — the lock simply had not been refreshed. The pin is now
   explicit at `"0.2.3"` so the intent is visible, but the real lesson is the
   one §2.4 states: nothing was stopping us from drifting, and nothing noticed.

### Phases 2–5

_Pending implementation._

## 7. Testing and Cleanup

_Pending implementation._
