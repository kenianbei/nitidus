# Nitidus — Roadmap v2

Supersedes `roadmap.md`, retired in `d2d50ff` and readable in the history before
it. That document's Phase 1 (items 1–30) is complete and shipped; its Phases 2–5
remain valid as a feature backlog and are carried forward here, re-sequenced.
Item numbering continues from 30 so design doc names never collide across the
two roadmaps.

## Why v2

Three inputs arrived after v1 was written.

**The Pimalaya ecosystem grew up.** `io-maildir`, `io-gmail`, `io-msgraph`,
`io-webdav` and `io-pim-discovery` did not exist, or were not viable, when v1
made its "hand-roll it" calls. `refactor-himalaya-sync-v1` takes a standing
position on which of their crates we track. v1's "Phase dependencies" section
points at `rust-libraries.md`, which was deleted in `c40060a` — that section is
replaced by the sync doc.

**We read the reference consumer.** himalaya-tui is a ratatui mail TUI by the
Pimalaya author — a direct peer at 6.3k lines against our 43.5k. It has no
cache, no threading, no search and no incremental sync, so it is behind us on
features. But it is the crate author's own usage, and it exposed a set of io-\*
capabilities we are not using, one type design that is better than ours, a pane
arrangement adopted here by explicit decision, and three UI affordances we lack.

**Structural debt accumulated.** 39 non-test source files exceed the 300-line
ceiling in `.claude/rules/code.md`, the worst at 653. Phase 1 was built fast and
in order; the seams that shipped are not all the seams we want. The same is true
of types: `MailError` is one stringly-typed variant behind ~85 `format!` sites,
and `MailBackend` faces three separate signature changes if each lands ad hoc.

**Non-goals, stated once:** JMAP (himalaya-tui ships it; no nitidus user wants
it; revisit only on demand) and m2dir (a different on-disk format, per the sync
doc).

## Standing principles

These apply to every phase below, not to a phase of their own. A phase that
cannot honour them is scoped wrong.

1. **Every phase leaves the app working.** Clippy clean, suite green, and
   manually drivable. No phase depends on a later phase to be coherent.
2. **Touch it, fix it.** Any file a phase edits comes out under 300 lines and
   under the nesting/parameter limits, or the phase says in its design doc why
   not. Debt is repaid by the work that walks past it, not by a cleanup phase
   that never gets scheduled.
3. **One pattern, written once.** Three call sites of the same shape is the
   trigger to extract, not before (`code.md`: three lines of duplication beat a
   premature abstraction). Phase B exists because we are about to write the
   fourth.
4. **Model the domain, not the wire.** Shared types own the semantics; adapters
   translate. A backend-specific concept that leaks into `nitidus-ui-kit` or the
   index is a design error.
5. **Refactors prove behavior preservation.** Per the refactor template: clippy,
   full test run with pass counts, and the existing integration suite as the
   contract. Behavior changes are never smuggled inside a refactor; they get
   their own item.

---

## Phase A — Protocol foundation

The layer everything else stands on. Ordered so each item makes the next one
smaller: the swap first while it is a pure refactor, then the trait redesigned
once, then the implementations against a stable target. Nothing here is
user-visible except item 35; that is the point.

### 31. Version catch-up

`io-imap` 0.2 → 0.3, `io-smtp` 0.2.0 → 0.2.3, both keeping
`default-features = false`. One compile break (`ImapMailboxWatchError::Select*`
→ `Examine*`). Inherits the Fastmail `COPYUID` fix, the SORT date-ordering fix,
and the watcher's SELECT → EXAMINE change.

Mechanical, and first because everything downstream targets 0.3.

_Doc:_ `refactor-himalaya-sync-v1` Phase 1 — already written.

### 32. The maildir swap

Replace the hand-rolled `nitidus-mail/src/maildir/` with `io-maildir` 0.2.0,
driven the way `imap/session.rs` drives `io-imap`. `folder_ops.rs` survives as
the validation and refusal layer in front of their unguarded coroutines;
`envelope.rs` and `watch.rs` are untouched.

This is a pure behavior-preserving refactor, and it stays one: it keeps
`MaildirFlagsSet` replace semantics because that is what our `MailBackend`
trait specifies today, and the 279-line integration suite is the contract. The
known wart — a replace erases flags we do not model, such as `P` set by another
client — predates the swap (`imap/backend.rs` uses `StoreType::Replace`; the
hand-rolled rename writes only five letters) and is fixed by item 34, not
smuggled in here.

Adopts himalaya-tui's `Deref`/`DerefMut` wrapper over the upstream client, so
every method is reachable without re-export boilerplate. Keeps two things they
gave up: our 64 KB windowed header read with streamed batches (their
`read_entries` loads every body in full before parsing), and our folder counts.
Upstream issues (their non-ASCII flag letter ordering) are filed from this
item's R2 round.

Sequenced before the flag model deliberately: the swap shrinks the maildir
backend to a ~60-line wrapper, so item 34 rewires that instead of 700 lines of
code this item deletes.

_Doc:_ `refactor-himalaya-sync-v1` Phases 2–4 — written, R2 answered.

That doc also carries a **Phase 5**: adopting `pimalaya-stream` and converting
every remote transport — IMAP, SMTP, OAuth and IDLE — from async tokio to
blocking sockets on `spawn_blocking`, deleting `net.rs`. It is an alignment
decision (R2 A1), taken knowing the crate is blocking-only today and that the
tokio sibling module its docs anticipate does not exist yet. It runs last in
that doc and has no separate item number here.

### 33. `MailBackend` target shape

A design-only round. Three items would otherwise each mutate the `MailBackend`
trait in sequence — flag patch ops (34), streaming bodies (35), many-valued
message metadata for the REST backends (48/49) — and every mutation touches
every backend, of which there will be four by Phase E. Design the trait's
target shape once: flag operations, body sink/source, metadata surface, error
contract. Items 34 and 35 then implement slices of a stable target instead of
three successive signatures.

Costs a document, saves three rounds of cross-backend churn.

_Doc:_ `refactor-backend-trait-v1` — to write.

### 34. The flag model

**The keystone refactor.** Replace `Flags(u8)` — five bits, IANA-only, no
keywords — with a shared flag type carrying the raw wire spelling plus an
optional IANA classification, after himalaya-tui's `email/flag.rs`. Equality,
ordering and hashing are IANA-first, so `\Seen`, `$seen` and `seen` collapse to
one logical flag while custom keywords compare case-insensitively.

Why it is the keystone: a Gmail label, an Outlook category, a notmuch tag and a
user-defined keyword are all the same thing — many-valued per-message metadata
we currently cannot represent at all. Four separate roadmap items are blocked on
answering this once:

- Gmail label round-tripping (item 48)
- Outlook categories (item 49)
- Custom tags/labels and tag-driven operations (item 52)
- notmuch tag workflows (item 65)

This is also where flag mutation switches from replace to patch semantics —
the trait's `set_flags(folder, id, flags)` becomes add/remove operations per
item 33's target shape, the IMAP backend moves from `StoreType::Replace` to
`Add`/`Remove`, and the maildir wrapper moves from `MaildirFlagsSet` to
`add_flags`/`remove_flags`. That closes the long-standing clobbering of flags
we do not model.

Not a wholesale copy of theirs. Their `Flag` allocates a `String` per flag and
they have no cache; our envelope cache packs flags into one byte across
potentially 100k rows. The design round decides the split — likely a bitset for
the IANA set plus a separate keyword list — and must cover the SQLite schema
migration.

Touches `nitidus-mail/src/types.rs`, both backends, the cache schema, the index
flag column and the batch-ops path.

_Doc:_ `refactor-flag-model-v1` — to write.

### 35. Streaming bodies

Adopt `io-imap`'s `ImapMessageFetchStream` and `ImapMessageAppendStream`,
available since 0.2.0 and currently unused. Today a fetched or appended message
lands in memory whole — a hard memory ceiling on large attachments, and the only
user-visible item in this phase.

Implements the sink/source surface from item 33's target shape. Worth doing
before the REST backends implement the trait, not after.

_Doc:_ `feature-streaming-bodies-v1` — to write.

### 36. Session hardening

The remaining unused `io-imap` surface, grouped because it is all one
concern — sessions that survive contact with real servers.

- **IMAP ID (`rfc2971`)** — some providers reject login without it. himalaya-tui
  shipped a fix for exactly this.
- **SASL: SCRAM-SHA-256 (`rfc7677`), `auth_plain`, `auth_login`** — we use the
  bare `LOGIN` command, which a growing number of servers disable. We already
  use `auth_plain` on the SMTP side only.
- **`unselect`, `logout`, `noop`** — session hygiene. Decide here whether the
  command connection gets a keepalive (himalaya-tui pings every 60 s) or stays
  on lazy reconnect. Our IDLE path already has a 20-minute read timeout with
  backoff, so this is only about the command connection.

_Doc:_ `feature-session-hardening-v1` — to write.

---

## Phase B — Shared client substrate

What the REST backends will stand on, built while there are still only two
consumers to prove it against.

### 37. One coroutine pump

We drive `io-imap` coroutines in `imap/session.rs` and `io-http` coroutines in
`oauth/mod.rs`. Items 48 and 49 would add `io-gmail` and `io-msgraph`, both
`io-http`-shaped with bearer auth and near-identical pump loops. That is the
fourth instance, which is where principle 3 says extract.

Factor one substrate over the shared shape — resume, match the yield, perform
I/O against our async transport, feed the reply — parameterised over the
protocol's yield/reply vocabulary. `imap/session.rs` and `oauth/mod.rs` are
rewritten onto it as proof, so the REST backends inherit a layer with two
working consumers rather than a speculative one.

Explicitly **not** himalaya-tui's `shared/client.rs`, which is 283 lines of one
enum with the same eight-arm match repeated per method. Our `MailBackend` trait
is the better dispatch; this item is about the transport pump beneath it, not
the dispatch above it.

_Doc:_ `refactor-coroutine-pump-v1` — to write.

### 38. Error model

`MailError` is essentially `Backend(String)` — one stringly-typed variant fed
by ~85 `format!` sites. `rust.md` mandates structured thiserror variants, and
the REST backends cannot be written without them: a 401 must trigger token
refresh and retry, a 429 must trigger backoff, and neither decision can be made
against a string. Structured errors also let the UI's severity routing (toasts
vs log) classify failures instead of guessing.

Belongs before items 48/49; the retry/backoff policy lives in the pump (37),
which is why they are neighbours.

_Doc:_ `refactor-error-model-v1` — to write.

### 39. Backend test fixtures

Both REST backend docs ask the same question (their R1 Q6): fixtured-response
harness or live smoke only? `norman.kerr.dev` is the sole live account, and
Graph has no test account at all. REST responses are JSON we can record once
and replay against the coroutines — which are I/O-free by construction, so the
harness is a natural fit. One shared harness for both backends, plus the
existing maildir corpus formalised, per principle 3.

_Doc:_ `feature-test-harness-v1` — to write.

### 40. Structural debt in the router path

`command/table.rs` (653), `action.rs` (556) and `keymap/mod.rs` (542) are the
three largest non-test files and sit on the path every phase below touches.
Split by responsibility before the phases that edit them, not after.

Scoped tightly: these three files plus whatever they cleave into. The other
oversized files are repaid under principle 2 by the phases that walk past them.

_Doc:_ `refactor-router-split-v1` — to write.

---

## Phase C — Discovery & onboarding

### 41. Account autoconfig

Adopt `io-pim-discovery` so the wizard discovers IMAP/SMTP settings from the
user's address instead of asking for them, retiring the host/port half of the
hardcoded Gmail and Outlook presets.

himalaya-tui's `wizard/discover.rs` is a working reference and settles the
question of whether SRV needs a system resolver: it points at a fixed
DNS-over-TCP endpoint (`tcp://1.1.1.1:53`) instead of pulling `resolv-conf`.
Their probe cascade is first-hit-wins — PACC → Autoconfig ISP → ISP-fallback →
ISPDB → RFC 6186 SRV. Hardcoding a resolver is a privacy decision, not only a
technical one, and the design round should treat it as configurable.

_Doc:_ `feature-autoconfig-v1` — written, pending R1 answers.

---

## Phase D — UI restructure & debt

Two structural decisions and three affordances. The structural items land
first so the affordances are built against the final layout. All of Phase D is
independent of Phases A–C.

### 42. Pane layout

Adopt himalaya-tui's pane arrangement, superseding the miller-columns reading
pane from v1 item 29 (`refactor-ui-v1`): a left sidebar for folders, the index
filling the right main area, and — when a message is opened or a composer
started — a bottom pane splitting the right column, hosting the reader or the
composer. Esc closes the bottom pane and returns the right column to the index
alone.

Contacts follow the same design: the contact list on the right, the selected
contact on the left, and the edit pane opening below with every field a
tab-focusable form stop — the same form machinery the composer already uses
(`feature-compose-form-v1`, `feature-overlay-forms-v1`), not a second
implementation.

The overlay stack, severity-routed feedback and forms from `refactor-ui-v1`
all survive; this changes the column arrangement, not the surface system.

_Doc:_ `feature-pane-layout-v1` — to write.

### 43. aerc-style tabs

Tabs become first-class and closable, after aerc: one tab per account, one tab
for contacts. `:contacts` opens (or focuses) the contacts tab; closing it
returns to the previous tab. Opening and closing tabs is a normal operation,
not a mode switch.

This replaces the current fixed tab-bar shell semantics. It also sets the
shape for later work: a unified inbox (item 57) becomes just another tab, and
per-account state (selection, folder, scroll) lives with its tab instead of
being swapped in place.

_Doc:_ `feature-account-tabs-v1` — to write.

### 44. Scroll position

We render no `Scrollbar` anywhere in the codebase and have no scroll indicator
in the pager. In a long message there is no way to tell where you are.
himalaya-tui renders a proportional scrollbar with end symbols on the message
pane.

Belongs in `nitidus-ui-kit` as one reusable surface affordance, consumed by the
pager, the index and the sidebar — not three separate implementations.

### 45. Themes beyond one

`nitidus-ui-kit/src/theme/` is the more sophisticated system — computed
palettes, state derivation, lighten factors — but exactly one preset was ever
built on it (`tailwind_dark`). There is no light theme, so a light terminal is
unusable. himalaya-tui ships four named presets including a light one.

Two parts: build the missing presets on our existing machinery, and adopt their
override mechanism — `Style::patch` layering per-field config overrides onto a
preset, so setting only `fg` keeps the preset's `bg` and modifiers. Their
`resolve` is 14 near-identical if-lets; ours should not be.

### 46. Picker polish

himalaya-tui's copy-to/move-to dialog uses a fixed-height results frame so the
dialog does not resize as the filter narrows. Small, and it is the difference
between a picker feeling solid and feeling twitchy. Applies to our overlay
pickers generally.

_Doc:_ `feature-ui-affordances-v1` — to write, covering 44–46 as one pass over
`nitidus-ui-kit`.

---

## Phase E — Provider-native fidelity

Was v1 Phase 3. Depends on item 34 (flags as many-valued metadata) and item 37
(the pump substrate), with 38/39 (errors, fixtures) as supporting
infrastructure; neither backend is worth starting before they land.

### 47. Server-side SORT and THREAD

`io-imap`'s `rfc5256::{sort, thread}` is unused. We hand-roll JWZ threading in
`thread.rs`; Gmail and Dovecot both advertise `THREAD=REFERENCES`. Capability-
gated with our existing implementation as the fallback, so `thread.rs` stays but
stops being the only path. io-imap 0.2.0 already ships a client-side SORT
fallback we can mirror.

Sequenced first in this phase because it is protocol work that benefits every
backend, not one provider.

### 48. Gmail backend

`io-gmail` REST backend: label round-tripping, dedup by message id,
Gmail-fidelity threading, search passthrough, archive-safe expunge. Settles v1's
"raw commands or upstream contribution or REST backend" either/or in favour of
REST.

The label-vs-folder impedance mismatch is the hard part and is why item 34 comes
first.

_Doc:_ `feature-gmail-backend-v1` — written, pending R1 answers.

### 49. Microsoft Graph backend

`io-msgraph` backend for categories, Focused Inbox, server rules and search
folders. Graph is genuinely folder-shaped, so it maps onto `MailBackend`
one-to-one and carries far less design risk than Gmail — a reason to consider
landing it first of the two.

_Doc:_ `feature-msgraph-backend-v1` — written, pending R1 answers.

### 50. IMAP-fixable provider gaps

Outlook localized folder detection, correct Sent-Items APPEND, TNEF detection.
Per-column pattern-driven index colors and conditional date formats.

---

## Phase F — Power triage & search

Was v1 Phase 2, unchanged in content. Item 34 unblocks the tags work; the rest
is pure client-side and can interleave with Phase E.

51. Full pattern/query language (neomutt-class operators) for
    limit/search/tag/color.
52. Custom tags/labels and tag-driven operations — **now expressible** thanks to
    item 34.
53. Saved searches as virtual folders.
54. Snooze, mute, auto-advance after triage.
55. Sweep-style bulk hygiene.
56. Local full-text search (SQLite FTS) over cached mail.
57. Unified inbox across accounts — as a tab, per item 43; selectable keymap
    schemes.
58. Scheduled send; send-as aliases; Fcc routing; templates.
59. One-key unsubscribe (RFC 8058); `mailto:` handling.
60. Background periodic sync + notifications; config hot-reload.

---

## Phase G — Rich content & crypto

Was v1 Phase 4, unchanged. Heavy external dependencies.

61. HTML tier 2 — inline images via kitty/iTerm2/sixel with halfblock fallback.
62. HTML tier 3 — headless Chromium rendering, cached, auto-degrading.
63. Calendar invites — rendering + iTIP replies.
64. PGP via system gpg.

---

## Phase H — Ecosystem & automation

Was v1 Phase 5, unchanged except that CardDAV now has a named crate.

65. notmuch backend with tag workflows — inherits item 34's keyword model.
66. CardDAV contact sync — `io-webdav`, revisited when this phase starts (one
    release as of 2026-07-27).
67. Hooks (folder-enter, message-received/sent, pre-send, startup/shutdown).
68. Shell integration: `:pipe`, `:exec`.
69. Quick-Steps-style named macros.
70. External query-command escape hatch for address lookup.

---

## Dependency summary

```
31 version bump
      └── 32 maildir swap (pure refactor; suite is the contract)
              └── 33 backend-trait target shape (design round)
                      ├── 34 flag model ──┬── 48 Gmail
                      │                   ├── 49 Graph
                      │                   ├── 52 tags
                      │                   └── 65 notmuch
                      └── 35 streaming bodies
37 coroutine pump ──┐
38 error model    ──┼── 48 Gmail / 49 Graph
39 test fixtures  ──┘
40 router split ── everything that edits the router path
41 autoconfig            ── independent
42 panes / 43 tabs       ── independent; 42 reuses the form machinery
44–46 affordances        ── after 42/43 settle the layout
47 SORT/THREAD           ── precedes 48/49, benefits all backends
```

Item 41 and items 44–46 are the natural fillers when a larger phase is blocked
on a discussion round; 42 and 43 are the larger independent projects that can
run alongside Phases A–B entirely.
