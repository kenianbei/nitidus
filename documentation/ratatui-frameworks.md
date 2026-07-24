# Ratatui Rendering Frameworks — Analysis

Survey of application-architecture layers that build on ratatui, assessed
against nitidus's demands: a long-running app; multiple screens/tabs; a
virtualized 100k-row message list; modal keybindings plus a `:` command
line; popup dialogs and forms; focus management across many widgets; tokio
async integration (IMAP sync events streaming into UI state); terminal
image rendering (ratatui-image); custom themes.

The stated preference is bevy_ratatui + plurimus (proven in vcard_tui).
This document assesses whether ECS is the right fit and what the
alternatives are. Versions and activity verified against crates.io/GitHub
as of July 2026.

**Baseline facts**: ratatui 0.30.2 (2026-06); 0.30 (Dec 2025) split out
ratatui-core, so "supports 0.30" is the current ecosystem litmus test.
ratatui-image 11.0.6 renders into an ordinary ratatui `Buffer` via a
`StatefulWidget`, so it works with anything that hands you a ratatui
`Frame` — and with nothing that doesn't (iocraft, r3bl_tui, cursive).

## 1. Raw ratatui + hand-rolled loop (the baseline)

- **Architecture**: immediate mode; you own the loop. The official
  ratatui/templates repo ships the patterns: an `Event` enum
  (input/tick/backend messages) through an mpsc channel into a single
  `update()` dispatch, then a full-frame `draw()` — The Elm Architecture
  by hand, which the ratatui docs themselves recommend.
- **What production apps actually do**: hand-rolling is overwhelmingly the
  norm — gitui (Component trait tree + crossbeam + worker threads), yazi
  (tokio + custom scheduler), atuin, bottom, television, gitu, jjui,
  OpenAI's codex TUI. None use a framework crate.
- **Async**: first-class by construction — the IMAP task sends typed
  events into the same channel the input stream feeds.
- **Virtualized 100k list**: trivially natural in immediate mode — render
  `rows[offset..offset+height]`, done. Arguably easier without a
  framework.
- **Cost**: you write focus ring, mode enum, dialog stack, command line
  yourself — a few hundred well-trodden lines you own forever.
- **Fit**: the safe default; maximum control, zero dependency risk,
  perfect ratatui-image/theme compatibility.

## 2. bevy_ratatui + plurimus (ECS — the incumbent preference)

- **Health**: bevy_ratatui 0.11.1 (2026-02), ratatui-org-hosted,
  legitimately maintained; targets bevy 0.18 while **bevy 0.19 is already
  out** — being one release behind is the permanent condition of the Bevy
  orbit (3–4 breaking releases/year). plurimus 0.1.0 (2026-03): 25
  downloads, 2 commits, 1 star — a brand-new single-author experiment;
  adopting it means co-maintaining it from day one (mitigated if that
  author is us).
- **Architecture**: `RatatuiContext` draw resource, crossterm events as
  Bevy messages, kitty keyboard support; runs headless via
  `ScheduleRunnerPlugin::run_loop(1/60s)` — **a fixed-rate tick loop, not
  an event-driven idle loop**. An idle email client wakes 60×/second
  unless we write a custom runner.
- **Async**: bevy task pools aren't tokio; the IMAP stream crosses a
  channel boundary into ECS state — the same bridging work as the
  baseline. Async is a wash, not a win.

### What bevy genuinely buys

- **Change detection** (`Changed<T>`, resource ticks) — skip rebuilding
  view-models when nothing changed; though a hand-rolled dirty
  flag/version counter does the same in ~10 lines.
- **Plugins** — each screen as a plugin with its own systems is genuinely
  nice code organization (vcard_tui demonstrates this).
- **`States` + run conditions** — `in_state(Mode::Compose)` gating input
  systems is an elegant modal-keymap machine, for free.
- **Schedule ordering** — deterministic input → update → render, solved
  once.

### What it costs

- **Compile times and dependency weight** — bevy is heavy for an app
  whose UI is text cells; iteration speed suffers.
- **Idle CPU** — the 60fps runner vs blocking on events; fixable, but
  bespoke work.
- **API churn** — a permanent migration tax (or freezing on an old bevy)
  for an app meant to be used for years.
- **The mismatch tax** — the 100k-row list lives in a Resource (a
  Vec/DB handle), not as entities, so ECS queries buy nothing for the
  central data structure; focus/forms/command-line have no mature
  ECS-native solutions — plurimus's `UiFocusable` is a sketch of what
  rat-focus and tui-realm already do maturely.

## 3. tui-realm (veeso) — the battle-tested framework

- **Health**: tuirealm 4.1.0 (2026-05); 4.0 modernized to ratatui 0.30
  with a testing story. 972 stars, 211k downloads, tui-realm-stdlib in
  lockstep. Single primary author but a reliable 2021→now release
  history. The most battle-tested framework in the list.
- **Architecture**: Elm+React hybrid — `Application` owns a `View` of
  mounted components by `Id`; events route to the focused component;
  components emit `Msg`s consumed by your `Update`; `Sub`/`SubClause`
  subscriptions deliver events to unfocused components conditionally —
  exactly how a global `:` line and modal keymaps would be built. Focus
  is built into the View. Async via event "ports" with a tokio feature.
- **Criticisms (still true in v4)**: props flow through a
  dynamically-typed `Attribute`/`AttrValue` map — stringly-typed
  configuration the compiler can't check; every component wraps a
  `MockComponent` plus event→Cmd→CmdResult→Msg translation — boilerplate
  that compounds across a 15-screen app. ratatui-image integration is
  awkward (image state outside the props system) but done in the wild.
- **Fit**: genuinely viable; the subscription system is the best-fit
  modal-input machinery of any framework here. The props tax is real.

## 4. rat-salsa family (thscharler) — the functional best-fit

- **Health**: rat-salsa 4.0.3, rat-widget 3.2.1 (2026-03), ratatui 0.30,
  self-declared stable with semver, 3,328 commits, an actual book, 13+
  example apps. **Bus factor: 1** and low adoption — the risk is one
  prolific developer.
- **Architecture**: event-loop framework — events dispatch to handlers
  returning `Control` flow; built-in message queue, **rat-focus** (focus
  traversal), **rat-dialog** (dialog/window stack), timers,
  `spawn_async` (tokio) — the IMAP-into-UI story is directly supported.
- **Virtualization: verified** — rat-ftable's `TableData` trait renders
  only visible cells ("rendering time depends on screen size, not data
  size"); a 100k–1M-row message list is its design case. Richest widget
  set in the ratatui world: masked inputs, date/number fields, editor,
  menus, popups, splits, theming.
- **Crucial option**: rat-widget, rat-focus, rat-ftable, rat-event all
  **work standalone with plain ratatui** — usable from a hand-rolled loop
  (or in principle from inside bevy draw closures) without adopting the
  rat-salsa loop.
- **Fit**: strongest functional match of any framework; idiosyncratic
  API, one maintainer, you debug alone.

## 5. Ruled out / watch list

| Project | Status | Verdict |
|---|---|---|
| rooibos | Not on crates.io; pre-alpha; 5 stars; slowing | Conceptually ideal (signals = async state streaming) but one person's experiment. Watch. |
| widgetui | Last release Oct 2024; README says unmaintained | Dead. |
| intuitive | Last release 2023 | Dead. |
| matetui | Stale, tiny | Skip. |
| tui-react | Alive (gitoxide internal helper) | Not an app framework. |
| ratatui-kit | 0.10.3 (2026-07), React-style hooks, input layers for modal+overlay, tokio-native; young, mid-migration to 0.30 | The most interesting new entrant — re-evaluate in 6–12 months. |
| iocraft | Very active, React-like, **own renderer, not ratatui** | Forfeits ratatui-image + widget ecosystem. Wrong base. |
| r3bl_tui | Own engine, serves its own apps | Same forfeit. |
| cursive | Slow-moving (Aug 2024), retained-mode elder | Good dialogs/forms but no ratatui ecosystem. Not compelling. |

Prior art note: no mature ratatui email client exists to copy — nitidus
(joshka) is a proof-of-concept Himalaya frontend; himalaya-tui is early
development. Nitidus breaks ground either way.

## 6. Verdict: is ECS the right fit?

**Reasonable, but not advantageous.** ECS earns its keep with many
homogeneous entities carrying orthogonal, composable behaviors — game
worlds. An email client is database-shaped: one big ordered collection
(messages, which must live in a Resource, not as entities) plus a modest
fixed set of UI regions. Everything bevy provides here (change detection,
states, ordering, plugin modularity) is replicable in a hand-rolled loop
with an enum and a dirty flag; nothing hard about the email client
(virtualized list, focus, forms, modality, images) gets easier under ECS.

**The strongest non-ECS alternative**: a hand-rolled TEA/component loop
(started from ratatui's async/component templates), selectively importing
the standalone rat-* crates — rat-ftable for the message list, rat-focus
for traversal, rat-dialog/rat-widget where they fit. That is the proven
architecture of every major production ratatui app, with a verified
O(screen) table, first-class tokio integration, and no framework lock-in;
each rat-* crate stays replaceable behind our own traits, hedging the
bus-factor risk.

## 7. Considerations specific to nitidus

Factors the generic analysis can't weigh, which cut in bevy's favor:

- **vcard_tui exists on the bevy stack** — the theme system, layout
  closures, builders, editor state-machine, and the contacts plugins port
  directly only if nitidus stays on bevy + plurimus. On any other
  architecture the *patterns* port but the code must be rewritten
  (the theme and vCard data layers are UI-framework-agnostic and port
  regardless).
- **plurimus is controllable** — its immaturity is a different risk when
  we can fix/extend it ourselves, and nitidus is the forcing function
  that matures it.
- **Team familiarity** — one developer already fluent in the bevy
  reconcile pattern ships faster on it than on a new architecture.

And the two costs that remain real regardless: bevy's upgrade treadmill,
and the idle 60fps runner (mitigable: draw-skip on no-change, lower tick
rate, or a custom park-on-event runner).

**Decision framing**: this is a two-way door with a deadline. The M0
spikes (10k-envelope stream into a scrolling table; $EDITOR
suspend/resume) should be built twice if there is any doubt — once on
bevy+plurimus, once as a hand-rolled loop borrowing rat-ftable — and the
winner judged on: idle CPU, code size of the index screen, and how it
feels to add one new popup. After M1 the architecture is effectively
permanent.

## Sources

- crates.io/docs.rs records for ratatui, ratatui-image, bevy_ratatui,
  plurimus, tuirealm, tui-realm-stdlib, rat-salsa, rat-widget, widgetui,
  ratatui-kit, iocraft, r3bl_tui, cursive, tui-react, intuitive, matetui
- github.com/ratatui/bevy_ratatui · github.com/kenianbei/plurimus ·
  bevy.org/news/bevy-0-18 · Bevy 0.19 release coverage
- github.com/veeso/tui-realm (+ CHANGELOG) ·
  github.com/thscharler/rat-salsa (+ book:
  thscharler.github.io/rat-salsa) · github.com/aschey/rooibos ·
  github.com/yexiyue/ratatui-kit · github.com/ccbrown/iocraft
- github.com/ratatui/templates · ratatui discussion #220 (architecture
  best practices) · awesome-ratatui · ratatui.rs/showcase
- Prior art: github.com/joshka/nitidus · github.com/pimalaya/himalaya
