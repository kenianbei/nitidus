# feature - Autocomplete - v1

Roadmap 1e.23, the last item of the differentiator phase: address autocomplete
in To/Cc/Bcc fed by two sources — the contact book and a frecency-ranked store
of addresses harvested from mail traffic — plus the two bridges that finally
connect contacts to mail: `:add-contact` from a message's sender and
`:compose-to` from a contact.

## 1. Current Design

- **Compose headers are plain prompts.** `ComposeSession` holds `to`/`cc`/`bcc`
  as comma-separated strings; `prompt_header` opens a standard `PromptRequest`
  prefilled with the current value. No completion of any kind — the spec's
  "contact autocomplete in To/Cc/Bcc" is unstarted.
- **Completion precedent exists in the command line**: `CommandLineState` keeps
  live matches, Tab cycles them, and a Helix-style bottom panel
  (`cmdline/panel.rs`) shows the candidates above the statusline. The prompt
  (`prompt.rs`) has none of this — `PromptRequest` is label + initial + masked +
  callbacks.
- **Address data available today**:
  - the contact book (names, emails with TYPE params) is in memory
    (`ContactStore`), rebuilt orderings on change — but nothing indexes it for
    lookup;
  - `EnvelopeSummary` carries `from_display`/`from_addr` only (no To/Cc —
    io-imap's envelope parse and the maildir scan both drop them), and every
    envelope batch already flows through one choke point: the cache writer's
    `record` (and `MailStore` on the app side);
  - the send path has clean recipient data twice: the session's header strings
    at `queue_send`, and the outbox metadata's parsed `recipients: Vec<String>`.
- **Persistence tier is pre-decided**:
  [persistence.md](../documentation/persistence.md) §3 places the harvested
  frecency store in `mail.db` (cache-tier, deletable — losing it only costs
  re-learning), never in the precious contact files. The cache has a migration
  mechanism (`user_version`, currently at schema v2) and typed `CacheOp`s
  applied on a writer thread; warm start loads folders and envelopes before the
  UI boots.
- **Matching machinery**: `nucleo-matcher` is already a dependency (overlay
  picker filtering); the roadmap line says "`ContactIndex` prefix map", written
  before nucleo was in the tree.
- **Bridges absent**: nothing creates a contact from a message (`:new-contact`
  types everything by hand), and nothing starts a composition from a contact
  (the contact tab is a dead end toward mail). `A` is unbound in the index and
  pager contexts; `m` is unbound in the contacts context.

## 2. Proposal

1. **Harvesting, two sources.** (a) Every queued send records its recipients —
   the strongest signal, these are people _you_ write to. (b) Every envelope
   batch records its senders — bulk, cheap, already flowing through the store.
   Records land in a new `mail.db` table
   (`harvested_addresses: addr PRIMARY KEY, display, uses, last_seen`) via a new
   `CacheOp`; `uses` increments, `last_seen` takes the newest date, display
   names fill in when a record finally has one.
2. **Frecency.** Score = `uses` decayed by age of `last_seen` (half-life ~30
   days) — an address you mailed yesterday outranks one that got twenty
   newsletters last year. Warm start loads the table into an in-memory
   `AddressBookIndex` resource alongside the contact-derived entries; harvest
   events update it live.
3. **`AddressBookIndex`** (app-side resource): completion entries from both
   sources — every contact email becomes `Display Name <addr>` (contacts always
   rank above harvested; they are deliberate), every harvested address likewise
   with its frecency score. Matching via nucleo fuzzy over name+address, ties
   broken by source then frecency — a deliberate upgrade from the roadmap's
   literal "prefix map", using the matcher already in the tree and consistent
   with how the overlay picker filters.
4. **Prompt completion.** `PromptRequest` grows an optional completion source
   (`with_completions(Fn(&str) -> Vec<String>)`); when present, the prompt gets
   live candidates in a bottom panel above the statusline (mirroring the command
   line's) and Tab cycles them. For address fields the completed unit is the
   segment after the last comma, so multi-recipient fields compose naturally.
   `prompt_header` passes the completion source for To/Cc/Bcc; every other
   prompt is unchanged.
5. **`A`/`:add-contact`** (index and pager): takes the selection's (pager's open
   message wins) `from_display`/`from_addr`; if the address is already in the
   book → statusline notice; otherwise a name prompt prefilled with the display
   name chains into creating + saving the contact with that email, exactly like
   `n` but pre-populated.
6. **`m`/`:compose-to`** (contacts tab): activates the mail tab, then starts a
   composition with To prefilled as `Display Name <primary email>` of the
   selected contact. A contact without an email gets a notice instead.

Out of scope: To/Cc harvesting from opened or fetched messages (needs plumbing
through the pager; a recorded follow-up), extending `EnvelopeSummary` with
recipient lists (schema + both backends for marginal gain), harvest pruning
policies (the table is bounded by unique addresses), and any UI for
browsing/purging harvested addresses.

## 3. Discussion

### 3.1 R1 Questions

1. **Harvest sources.** Send recipients + envelope senders for v1, with
   opened-message To/Cc as a follow-up. The sender stream includes mailing lists
   and newsletters — mitigated by contacts always outranking harvested entries
   and frecency favoring what you actually interact with. OK?
2. **Fuzzy over prefix.** The roadmap says "prefix map"; I propose nucleo fuzzy
   matching instead (typing `kj` finds `Katherine Johnson <kj@nasa.example>`),
   frecency as tiebreak. Cheaper to build than a trie, consistent with the
   picker, strictly more useful. Confirm the deviation?
3. **Completion UX.** Live candidate panel above the statusline while a
   To/Cc/Bcc prompt is open, Tab cycles, completion replaces the segment after
   the last comma. Enter always submits the field as typed. Confirm?
4. **Keys.** `A` → `:add-contact` in index and pager; `m` → `:compose-to` in the
   contacts context (mirroring mail's `m` = compose). Confirm?
5. **Frecency shape.** `uses × 0.5^(age_days/30)`, computed at rank time. Any
   preference for the half-life, or fine to tune later by feel?
6. **Smoke.** You drive again: complete a To from both a contact and a harvested
   address (after sending yourself something), `A` a real sender into the book,
   `m` from a contact into a prefilled composition. OK?

### 3.2 R1 Answers

1. ok
2. confirm
3. confirm
4. confirm
5. no pref
6. ok

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **Harvest store.** `mail.db` schema v3: `harvested_addresses`
   (addr primary key, display, uses, last_seen). The cache writer
   harvests senders from every envelope batch it already records; a
   new public `harvest` op takes explicit entries (the send path);
   `MailCache::load_addresses` feeds warm start. Upsert semantics:
   uses accumulates, last_seen takes the max, display fills in when
   one finally arrives. Cache tests for accumulation and reload.
2. **`AddressBookIndex`.** App resource: harvested entries loaded at
   bootstrap, updated live from envelope batches in the engine drain
   and from sends; contact entries derived on demand from the book.
   `complete(query)` formats `Display Name <addr>`, nucleo-fuzzy over
   name+address, contacts ranked above harvested, frecency
   (`uses × 0.5^(age_days/30)`) as tiebreak. Unit tests for ranking,
   decay, and live updates.
3. **Prompt completion.** `PromptRequest::with_completions(fn)`:
   candidates recompute as the buffer edits, a bottom panel above the
   statusline shows them (mirroring the command line's), Tab cycles
   and replaces the active comma-segment, Enter submits the field as
   typed. Non-completing prompts are pixel-identical to today. Tests:
   cycling rewrites only the last segment, panel rows, masked prompts
   unaffected.
4. **Wiring and bridges.** To/Cc/Bcc prompts pass the index's
   completion source; `queue_send` harvests its recipients (index +
   cache). `A`/`:add-contact` in index and pager resolves the sender
   (open message wins), refuses known addresses with a notice, and
   chains a prefilled name prompt into a saved contact.
   `m`/`:compose-to` in the contacts context activates the mail tab
   and starts a composition with To prefilled; no email → notice.
   E2e tests for both bridges and header completion.
5. **Verification & smoke handoff.** Clippy + full workspace run with
   counts; Norman's checklist: complete a To from a contact and from
   a harvested sender, `A` a live sender into the book, `m` out of a
   contact. Fill §5–§7.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **378 passed, 0
  failed** (was 366 at branch start).
- New coverage:
  - cache: harvest upserts accumulate uses, keep the newest sighting,
    fill blank display names, and reload across restarts;
  - address index (unit): contacts outrank harvested regardless of
    frecency, fuzzy matches with frecency tiebreak, half-life decay
    math, recipient merge case-insensitivity, address parsing of
    `Name <a@b>` / bare / quoted-with-comma shapes;
  - prompt (unit): Tab cycles a frozen candidate list rewriting only
    the active comma-segment and wraps, typing recomputes and resets
    the cycle, Enter submits as typed, non-completing prompts ignore
    Tab entirely;
  - e2e: `A` prefills the sender's display name, saves the contact,
    and refuses a known address on the second press; seeded envelope
    senders appear as harvested candidates ranked below contacts;
    `m` on a contact lands on the mail tab with the To prompt
    prefilled and the session capturing it.
- Live smoke (Norman): **PASSED** — contact completion with the panel
  and Tab cycling, a harvested INBOX sender completing below contact
  matches, recipient harvest surfacing after a self-send, `A` adding a
  live sender (and refusing the second press), and `m` bridging into
  a prefilled composition — all as expected, no fixtures needed (his
  live vdir and INBOX were the data).

## 6. Implementation Report

- **Senders never needed the new table.** The envelope cache already
  is the sender history — `AddressIndex` aggregates
  `uses = COUNT(*), last_seen = MAX(date)` per address lazily from
  `MailStore`, cached against a store content fingerprint, so rescans
  can never double-count and warm start gets senders for free. The
  `harvested_addresses` table (schema v3) holds only send recipients,
  which have no other home. This supersedes §2.1(b)'s
  record-per-batch wording with something strictly simpler.
- Completion is a prompt capability, not compose code: an opt-in
  closure over a candidate snapshot taken at prompt-open, ranked per
  keystroke. The Tab cycle freezes its list — recomputing against the
  just-inserted candidate would strand the rotation.
- The `:compose-to` roadmap name was already taken by the composer's
  own To-editing command; the bridge shipped as **`:mail-to`**
  (`m` in the contacts context regardless).
- `AddressIndex` initializes in `RouterPlugin` beside the other
  router-read resources, so every embedder gets completion-capable
  prompts without ordering trivia; the real app overwrites it with
  the warm-loaded history.
- Follow-ups: To/Cc harvesting from opened messages, a purge command
  for harvested addresses if ranking ever gets haunted, and richer
  RFC 5322 address parsing (groups, comments) if real headers demand
  it.

## 7. Testing and Cleanup

- Cleanup scope: the branch diff vs main. Comments state invariants
  (the frozen Tab-cycle list, fingerprint-keyed sender aggregation,
  cache-tier deletability, the `:mail-to` naming); no dead code —
  clippy silent, every helper has callers. No smoke artifacts to
  remove: this smoke ran entirely on live data.
- Final verification after the smoke:
  `cargo clippy --workspace --all-targets` zero warnings;
  `CARGO_INCREMENTAL=0 cargo test --workspace` **378 passed, 0
  failed** (suite counts confirmed present).
