# bevy_headless_test

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![CI](https://github.com/warmar94/bevy_headless_test/actions/workflows/ci.yml/badge.svg)](https://github.com/warmar94/bevy_headless_test/actions/workflows/ci.yml)
[![Bevy 0.20.0](https://img.shields.io/badge/Bevy-0.20.0-informational)](https://bevyengine.org)
[![bevy_replicon 0.45.2 (optional)](https://img.shields.io/badge/bevy__replicon-0.45.2%20(optional)-orange)](https://crates.io/crates/bevy_replicon)

A **test kit for [Bevy](https://bevyengine.org) apps**: a strict, headless, deterministic `App`
for your system tests, plus the helpers every Bevy project ends up rewriting: message and state
assertions, UI text read-back, and (optionally) replicon protocol checks.

It is a **dev-dependency**: nothing of it ships in your game.

```rust
use bevy::prelude::*;
use bevy_headless_test::prelude::*;

#[derive(Resource, Default)]
struct Score(u32);

fn add_one(mut score: ResMut<Score>) {
    score.0 += 1;
}

fn double(mut score: ResMut<Score>) {
    score.0 *= 2;
}

let mut app = TestApp::new();
// Both systems write `Score`. Without `.chain()`, the first step fails the test and names
// the schedule, both systems and `Score`.
app.init_resource::<Score>().add_systems(Update, (add_one, double).chain());
app.step_n(2);
assert_eq!(app.world().resource::<Score>().0, 6);
```

## Contents

- [Highlights](#highlights)
- [Quick start](#quick-start)
- [How to use it](#how-to-use-it)
  - [1. A strict test app](#1-a-strict-test-app)
  - [2. The ambiguity check and its allow-list](#2-the-ambiguity-check-and-its-allow-list)
  - [3. Time and frames](#3-time-and-frames)
  - [4. Messages](#4-messages)
  - [5. States](#5-states)
  - [6. UI text](#6-ui-text)
  - [7. Replicon apps and protocol hashes](#7-replicon-apps-and-protocol-hashes)
  - [8. Multiplayer tests over UDP loopback](#8-multiplayer-tests-over-udp-loopback)
  - [9. Several apps at once](#9-several-apps-at-once)
- [How it works](#how-it-works)
- [API reference](#api-reference)
- [Cargo features](#cargo-features)
- [Compatibility](#compatibility)
- [Examples](#examples)
- [Limitations and FAQ](#limitations-and-faq)
- [License](#license)
- [Contributing](#contributing)

## Highlights

- **Strict by default**: before every frame, `First`, `PreUpdate`, `Update`, `FixedUpdate`,
  `PostUpdate` and `Last` are built and every unordered pair of systems that conflicts on the same
  data fails the test, naming the schedule, both systems (full paths) and the data. Races are
  caught by `cargo test`, not by a playtest.
- **An allow-list for what you cannot order**: a pair, a system, a library's own internal pairs,
  a piece of data, per schedule or everywhere.
- **Deterministic time**: every frame advances by exactly the same simulated duration, from the
  very first frame (Bevy's own first frame has a zero delta). The default frame is 1/64 s, Bevy's
  fixed timestep, so `FixedUpdate` runs exactly once per frame.
- **Plugins are finished**: the first frame runs `App::finish()` + `App::cleanup()`, which
  `App::run` does and a bare `app.update()` does not. Plugins that complete their setup in
  `finish()` (replicon among them) stop silently half-working in tests.
- **Messages without boilerplate**: `watch`, `send`, `step`, then `assert_exactly_one`,
  `assert_none`, `assert_count`, or the same since a `mark`.
- **States**: `set_state`, `assert_state`, `run_until_state`.
- **UI text read-back**: every `Text` / `TextSpan` / `Text2d` string of the world or of one
  panel, with `assert_all_ascii`, `assert_charset` (your font's glyphs), `assert_at_least`; a
  failure names the entity, its `Name`, the character (with its code point) and the string.
- **Replicon** (optional): a test app that is finished right after your registrations, so client
  events drain locally; `protocol_hash` and `assert_same_protocol` for "client and server
  register the same things in the same order".
- **Clear failures, no other panics**: an assertion panics with what went wrong and where; every
  query-style function returns `Option` / a value instead.

## Quick start

Add it as a dev-dependency. From crates.io:

```toml
[dev-dependencies]
bevy_headless_test = "0.2.0"
```

or from the repository, pinned to a release tag:

```toml
[dev-dependencies]
bevy_headless_test = { git = "https://github.com/warmar94/bevy_headless_test", tag = "v0.2.0" }
```

A typical system test:

```rust
use bevy::prelude::*;
use bevy_headless_test::prelude::*;

#[derive(Message, Clone, Debug)]
struct Hit(u32);

#[derive(Message, Clone, Debug)]
struct Died;

#[derive(Resource)]
struct Health(u32);

fn apply_hits(mut hits: MessageReader<Hit>, mut health: ResMut<Health>) {
    for hit in hits.read() {
        health.0 = health.0.saturating_sub(hit.0);
    }
}

fn check_death(health: Res<Health>, mut died: MessageWriter<Died>) {
    if health.is_changed() && health.0 == 0 {
        died.write(Died);
    }
}

// In your crate this is a #[test] fn.
let mut app = TestApp::new();
app.add_message::<Hit>()
    .add_message::<Died>()
    .insert_resource(Health(10))
    .add_systems(Update, (apply_hits, check_death).chain());
app.watch::<Died>();

app.send(Hit(4)).step();
app.assert_none::<Died>();

app.send(Hit(6)).step();
app.assert_exactly_one::<Died>();
```

`cargo run --example quick_start` runs a slightly longer version (with a state transition).

## How to use it

Everything below uses `use bevy::prelude::*; use bevy_headless_test::prelude::*;`.

### 1. A strict test app

`TestApp::new()` gives you a windowless app with:

- Bevy's minimal plugins: `TaskPoolPlugin`, `FrameCountPlugin`, `TimePlugin` (no
  `ScheduleRunnerPlugin`: the test drives the frames), and `StatesPlugin`. **Do not add
  `MinimalPlugins` again**; add your own plugins as usual.
- `TimeUpdateStrategy::ManualDuration(frame)`.
- The ambiguity check on the six main schedules.

`TestApp` dereferences to `App`, so `add_plugins`, `add_systems`, `insert_resource`,
`world()`, `world_mut()` all work. Drive it with `step()` and friends: a bare `app.update()`
skips the checks and the message watchers.

To configure it, use the builder:

```rust
use bevy::prelude::*;
use bevy_headless_test::prelude::*;
use std::time::Duration;

#[derive(Resource)]
struct Difficulty(u8);

let mut app = TestApp::builder()
    .frame_duration(Duration::from_millis(20)) // 50 fps simulated
    .strict(Startup)                            // also check Startup
    .lenient(Last)                              // do not check Last
    .allow_pair("my_game::audio::duck", "my_game::audio::fade") // see section 2
    .setup(|app| {
        app.insert_resource(Difficulty(3));
    })
    .build();
app.step();
```

### 2. The ambiguity check and its allow-list

Two systems in the same schedule that access the same data, at least one of them mutably, with no
`.before()` / `.after()` / `.chain()` / set ordering between them, run in an order Bevy may change
from one build to the next. The test app lists every such pair before every frame and fails:

```text
1 unordered system pair(s) conflict on the same data:
  - Update: `my_game::add_one` and `my_game::double` access my_game::Score (at least one of them
    mutably) with no order between them
Order them (`.before()` / `.after()` / `.chain()` / a `SystemSet`), or, for a pair you cannot
order (a third-party crate's), allow it on the TestApp builder (...)
```

The right fix is almost always an ordering. For pairs you cannot order (inside a library, or a
race you have decided is harmless), the builder has an allow-list. A **name pattern** matches a
system whose full path equals it, ends with `::pattern`, or starts with `pattern::`; generic
arguments are ignored.

| builder method | allows |
|---|---|
| `allow_pair(a, b)` | that pair, either order, in every checked schedule |
| `allow_pair_in(Schedule, a, b)` | that pair in one schedule |
| `allow_system(name)` | every pair involving that system |
| `allow_internal("some_crate")` | pairs whose BOTH systems belong to that crate / module |
| `allow_among(["crate_a", "crate_b"])` | pairs whose both systems belong to any of them |
| `allow_data("my_game::Score")` | conflicts only on that data (a type-name pattern) |
| `allow_resource::<R>()` / `allow_component::<C>()` | Bevy's own `allow_ambiguous_*` for that type |
| `allow(AllowRule::..)` | any rule |
| `strict(label)` / `lenient(label)` / `strict_schedules(..)` | which schedules are checked |
| `no_ambiguity_check()` | nothing is checked |

Pairs between your systems and a library's are never covered by `allow_internal` /
`allow_among`: those are yours to order.

```rust
use bevy::prelude::*;
use bevy_headless_test::prelude::*;

#[derive(Resource, Default)]
struct Log(Vec<&'static str>);

fn a(mut log: ResMut<Log>) {
    log.0.push("a");
}

fn b(mut log: ResMut<Log>) {
    log.0.push("b");
}

// Without an order, `ambiguities()` lists the pair (and `step()` would fail).
let mut strict = TestApp::new();
strict.init_resource::<Log>().add_systems(Update, (a, b));
let found = strict.ambiguities();
assert_eq!(found.len(), 1);
assert_eq!(found[0].schedule, "Update");

// Allowed: this test does not care about the order.
let mut relaxed = TestApp::builder().allow_data("Log").build();
relaxed.init_resource::<Log>().add_systems(Update, (a, b));
relaxed.step();
```

`ambiguities()` returns the uncovered pairs without panicking (`Ambiguity { schedule, system_a,
system_b, conflicts }`, `conflicts` empty = the whole `World`); `check_ambiguities()` runs the
check without stepping; `ambiguity_policy_mut()` changes the policy of a built app.

A schedule that cannot be built at all (a cycle, a set ordered against itself) fails the same
way, with Bevy's own explanation.

### 3. Time and frames

| call | does |
|---|---|
| `step()` | one frame |
| `step_n(n)` | `n` frames |
| `step_secs(secs) -> u32` | `ceil(secs / frame)` frames; returns how many |
| `run_until(\|world\| .., max_frames) -> u32` | frames until the condition holds (checked before each frame); fails after `max_frames` |
| `frame_count()`, `elapsed()`, `frame_duration()`, `set_frame_duration(d)` | the clock |
| `finish()` | run `App::finish` + `cleanup` now (the first frame does it otherwise) |

```rust
use bevy::prelude::*;
use bevy_headless_test::prelude::*;
use std::time::Duration;

#[derive(Resource)]
struct Difficulty(u8);

let mut app = TestApp::builder().frame_duration(Duration::from_millis(20)).build();
assert_eq!(app.step_secs(1.0), 50);
assert_eq!(app.elapsed(), Duration::from_secs(1)); // exact, from frame 1 on

#[derive(Resource, Default)]
struct Ticks(u32);
app.init_resource::<Ticks>().add_systems(Update, |mut t: ResMut<Ticks>| t.0 += 1);
let frames = app.run_until(|world| world.resource::<Ticks>().0 == 3, 10);
assert_eq!(frames, 3);
```

Keep the frame at or below 250 ms: Bevy's virtual clock clamps a longer delta
(`Time<Virtual>::max_delta`). `real_pause(d)` on the builder adds a real sleep after each frame,
for tests that talk to the OS (sockets).

### 4. Messages

Watch a message type, then step: every message written in a frame is recorded with that frame,
exactly once, even though Bevy keeps it for two frames.

| call | does |
|---|---|
| `watch::<M>()` | start recording `M` (needs `M: Clone`) |
| `send(msg)` / `send_and_step(msg)` | write a message (fails if `M` is not registered) |
| `messages::<M>()` | the `M`s of the last frame |
| `mark()` / `messages_since::<M>(mark)` | the `M`s of every frame after the mark |
| `all_messages::<M>()` | everything since `watch` |
| `assert_exactly_one::<M>() -> M` / `assert_none::<M>()` / `assert_count::<M>(n)` | the last frame |
| `assert_exactly_one_since::<M>(mark)` / `assert_none_since::<M>(mark)` | since a mark |

```rust
use bevy::prelude::*;
use bevy_headless_test::prelude::*;

#[derive(Message, Clone, Debug, PartialEq)]
struct Ping(u32);

#[derive(Message, Clone, Debug, PartialEq)]
struct Pong(u32);

let mut app = TestApp::new();
app.add_message::<Ping>().add_message::<Pong>().add_systems(Update, |mut pings: MessageReader<Ping>, mut pongs: MessageWriter<Pong>| {
    for ping in pings.read() {
        pongs.write(Pong(ping.0 * 10));
    }
});
app.watch::<Pong>();

let start = app.mark();
app.send(Ping(1)).step();
assert_eq!(app.assert_exactly_one::<Pong>(), Pong(10));
app.step().step();
app.assert_none::<Pong>();
assert_eq!(app.assert_exactly_one_since::<Pong>(start), Pong(10));
```

A failure says what it expected and what it got (`expected 1 `Pong` in frame 3, got 0: []`),
that a type is not watched or not registered, or that messages were dropped because the app was
stepped with a bare `update()`.

### 5. States

```rust
use bevy::prelude::*;
use bevy_headless_test::prelude::*;

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Screen {
    #[default]
    Menu,
    Game,
}

let mut app = TestApp::new();
app.init_state::<Screen>();
app.step();
app.assert_state(Screen::Menu);
app.set_state_and_step(Screen::Game); // queued, then applied by the frame's StateTransition
app.assert_state(Screen::Game);
assert_eq!(app.state::<Screen>(), Some(Screen::Game));
```

`set_state(s)` only queues (applied next frame); `run_until_state(s, max_frames)` steps until it
holds. A wrong state fails with both values; a state that was never initialised says so.

### 6. UI text

Feature `ui` (on by default). `ui_text()` collects every `Text`, `TextSpan` and `Text2d` string
of the world; `ui_text_under(root)` those of one entity and its descendants, in reading order (a
parent before its children, children in order). Then assert:

| call | asserts |
|---|---|
| `assert_all_ascii()` | printable ASCII only (a font with only the 95 ASCII glyphs draws it) |
| `assert_charset("...")` | only these characters (your font's coverage) |
| `assert_chars("rule name", \|c\| ..)` | any predicate |
| `assert_at_least(n)` | at least `n` strings: a page that drew nothing cannot pass |
| `assert_contains("...")` | some string contains it |

Line breaks and tabs are layout, never rejected. `entries()` gives each `TextEntry { entity,
name, kind, text }`; `find_rejected(pred)` returns the first bad character without panicking.

```rust
use bevy::prelude::*;
use bevy_headless_test::prelude::*;
use bevy_headless_test::text::{Text, TextSpan};

let mut app = TestApp::new();
let panel = app.world_mut().spawn(Name::new("audio panel")).id();
let row = app.world_mut().spawn((Text::new("Volume: "), ChildOf(panel))).id();
app.world_mut().spawn((TextSpan::new("80%"), ChildOf(row)));
app.step();

app.ui_text_under(panel).assert_all_ascii().assert_at_least(2).assert_contains("Volume");
```

A failure reads:

```text
'–' (U+2013) at byte 3 is not ASCII: TextSpan on 16v0 ("volume value"): "80 – 90"; checked 3 string(s)
```

For a tabbed or paged UI, collect once per tab after the frame that builds it: only the page on
screen exists.

### 7. Replicon apps and protocol hashes

Feature `replicon`. Replicon computes its protocol hash, and wires the local delivery of client
events when there is no connection, in `Plugin::finish`. A hand-made test app that is only
`update()`d never gets there: client events never arrive and there is no hash.

- `replicon::replicon_app(|app| { /* your registrations */ })`: a test app with
  `RepliconPlugins`, your registrations, then `finish()` + `cleanup()`.
- `replicon::replicon_builder()`: the same builder, unfinished, for more configuration.
- Replicon's own pairs, and its race with `bevy_time` in `PreUpdate`, are allowed
  (`REPLICON_THIRD_PARTY`); yours against replicon's are still checked.
- `replicon::protocol_hash(&app)` and `replicon::assert_same_protocol(&a, &b)`: the hash covers
  every replicated component, event and message in registration ORDER; a client and a server that
  register the same things in a different order cannot talk.

```rust
use bevy::prelude::*;
use bevy_headless_test::prelude::*;
use bevy_headless_test::replicon::bevy_replicon::prelude::*;
use bevy_headless_test::replicon::{assert_same_protocol, replicon_app};
use serde::{Deserialize, Serialize};

#[derive(Event, Serialize, Deserialize, Clone)]
struct Ping;

#[derive(Resource, Default)]
struct Pings(u32);

/// The shared protocol: one function both stacks call.
fn protocol(app: &mut App) {
    app.add_client_event::<Ping>(Channel::Ordered);
}

let mut client = replicon_app(|app| {
    protocol(app);
    app.init_resource::<Pings>().add_observer(|_: On<FromClient<Ping>>, mut n: ResMut<Pings>| n.0 += 1);
});
let server = replicon_app(protocol);
assert_same_protocol(&client, &server);

// No connection: the event is delivered locally, as from `ClientId::Server`.
client.world_mut().commands().client_trigger(Ping);
client.step_n(2);
assert_eq!(client.world().resource::<Pings>().0, 1);
```

Replicon's own in-memory connection, `bevy_replicon::test_app::ServerTestAppExt`
(`connect_client`, `exchange_with_client`), works on a `TestApp` too (it dereferences to `App`);
call `finish()` on both apps first.

### 8. Multiplayer tests over UDP loopback

A host and a client in one process over real UDP, through `bevy_net_session`, live in their own
crate: [`bevy_net_session_test`](https://crates.io/crates/bevy_net_session_test), built on this
kit. (Up to 0.1 they were this crate's `loopback` module, feature `net_session`.) This kit itself
knows nothing about any networking crate.

### 9. Several apps at once

`TestApp::step_together(&mut [&mut a, &mut b])` steps each app once, in order, then sleeps once
for the longest `real_pause` among them; `TestApp::run_together_until(apps, |apps| .., max)`
repeats it until the condition holds.

## How it works

- **The check** builds each checked schedule itself (`Schedule::initialize`, through
  `World::try_schedule_scope`) before the frame and reads Bevy's own conflict list
  (`ScheduleGraph::conflicting_systems`, the list that Bevy's `LogLevel::Error` ambiguity
  detection fails on). Bevy's `Error` mode cannot filter by system name, so the kit applies the
  allow-list and fails itself, with full system paths (the `debug` feature of `bevy_ecs` is
  enabled so names are real). A schedule is inspected again only when it was rebuilt. Bevy's
  `allow_ambiguous_resource` / `_component` and `ambiguous_with` are honoured as usual.
- **Time**: `TimeUpdateStrategy::ManualDuration(frame)`, plus one line before the first frame
  that tells `Time<Real>` its start, so frame 1 advances by a full frame like every other.
- **Finish**: the first frame (or `finish()`) waits for every plugin to be `ready()`, then runs
  `App::finish()` and `App::cleanup()`, exactly like `App::run`.
- **Messages**: each watched type keeps its own `MessageCursor` and drains `Messages<M>` after
  every frame, so every message is recorded once with its frame number, and a message the
  buffers dropped (a bare `update()` in between) is counted and reported.
- **UI text** reads the `Text` (bevy_ui), `TextSpan` (bevy_text) and `Text2d` (bevy_sprite)
  components directly; no layout, fonts or renderer are needed.

## API reference

### Crate root (`bevy_headless_test`, also in `prelude`)

| item | what |
|---|---|
| `TestApp` | the strict headless app (derefs to `App`) |
| `TestAppBuilder` | its configuration |
| `DEFAULT_FRAME` | 1/64 s |
| `default_strict_schedules()` | `First`, `PreUpdate`, `Update`, `FixedUpdate`, `PostUpdate`, `Last` |
| `AmbiguityPolicy { schedules, rules, enabled }` | what is checked and allowed |
| `AllowRule::{Pair, System, Among, Data}` | one allow-list entry |
| `Ambiguity { schedule, system_a, system_b, conflicts }` | one uncovered pair (`Display`) |
| `Mark(u64)` | a frame count, for "since" |
| `UiText`, `TextEntry { entity, name, kind, text }`, `TextKind::{Text, TextSpan, Text2d}` | UI text (feature `ui`) |

### `TestAppBuilder`

`frame_duration(Duration)`, `real_pause(Duration)`, `strict_schedules(labels)`,
`strict(label)`, `lenient(label)`, `no_ambiguity_check()`, `allow_pair(a, b)`,
`allow_pair_in(label, a, b)`, `allow_system(name)`, `allow_internal(prefix)`,
`allow_among(prefixes)`, `allow_data(name)`, `allow_resource::<R>()`,
`allow_component::<C>()`, `allow(AllowRule)`, `setup(FnOnce(&mut App))`, `build() -> TestApp`.

### `TestApp`

| group | methods |
|---|---|
| create | `new()`, `builder()`, `default()` |
| inner app | `app()`, `app_mut()`, `into_app()`, `Deref`/`DerefMut` to `App` |
| frames | `step()`, `step_n(n)`, `step_secs(secs) -> u32`, `run_until(pred, max) -> u32`, `finish()` |
| several apps | `step_together(apps)`, `run_together_until(apps, pred, max) -> u32` |
| clock | `frame_count()`, `elapsed()`, `frame_duration()`, `set_frame_duration(d)`, `real_pause()` |
| ambiguities | `ambiguities() -> Vec<Ambiguity>`, `check_ambiguities()`, `ambiguity_policy_mut()` |
| messages | `watch::<M>()`, `send(m)`, `send_and_step(m)`, `mark()`, `messages::<M>()`, `messages_since::<M>(mark)`, `all_messages::<M>()`, `assert_count::<M>(n)`, `assert_exactly_one::<M>()`, `assert_none::<M>()`, `assert_exactly_one_since::<M>(mark)`, `assert_none_since::<M>(mark)` |
| states | `state::<S>() -> Option<S>`, `set_state(s)`, `set_state_and_step(s)`, `assert_state(s)`, `run_until_state(s, max) -> u32` |
| UI text | `ui_text() -> UiText`, `ui_text_under(root) -> UiText` (feature `ui`) |

### `UiText` (feature `ui`)

`collect(&mut World)`, `collect_under(&World, root)`, `entries()`, `strings()`, `len()`,
`is_empty()`, `contains(needle)`, `find_rejected(pred)`, `assert_chars(what, pred)`,
`assert_all_ascii()`, `assert_charset(allowed)`, `assert_at_least(n)`,
`assert_contains(needle)`. Module `text` re-exports `Text`, `TextSpan`, `Text2d`.

### `replicon` (feature `replicon`)

`replicon_app(register) -> TestApp`, `replicon_builder() -> TestAppBuilder`,
`protocol_hash(&App) -> Option<ProtocolHash>`, `assert_same_protocol(&App, &App)`,
`REPLICON_THIRD_PARTY`, re-export `bevy_replicon`.

### What panics

The `assert_*`, `check_*`, `step*`, `run*_until*`, `send*`, `set_state*` functions panic with a message on failure: that is how a test fails. Everything else
(`state`, `messages*` for a watched type, `ambiguities` on a buildable app, `ui_text*`,
`protocol_hash`, the builder) returns a value.

## Cargo features

| feature | default | adds |
|---|---|---|
| `ui` | yes | UI text read-back (`bevy_ui`, `bevy_text`, `bevy_sprite`; no renderer) |
| `replicon` | no | the `replicon` module (`bevy_replicon` 0.45.2, client + server) |

Without default features the kit depends on `bevy_app`, `bevy_ecs`, `bevy_state`, `bevy_time`
and `bevy_diagnostic` only.

## Compatibility

| bevy_headless_test | Bevy | bevy_replicon (optional) | bevy_net_session (optional) |
|---|---|---|---|
| 0.2 | 0.20.0 | 0.45.2 | - (moved to `bevy_net_session_test`) |
| 0.1 | 0.19.0 | 0.44.2 | 0.1 |

Every Bevy crate is required as `=0.20.0` without default features, so your project keeps ONE
Bevy. The minimum Rust version is 1.97.1 (Bevy 0.20's).

## Examples

- `quick_start`: a typical system test (messages, a state transition, deterministic time).
  `cargo run --example quick_start`.

The `tests/` folder shows every helper, including the failures they produce.

## Limitations and FAQ

**Why not just set `ambiguity_detection: LogLevel::Error`?** That fails on every pair, including
those inside libraries you cannot order, and its message lists short names. The kit reads the
same list, lets you allow what is not yours, and prints full paths.

**My app has its own schedule.** `TestApp::builder().strict(MySchedule)`. A checked schedule the
app does not have is skipped.

**A library's pairs fail my test.** `allow_internal("that_crate")`, or `allow_among([...])` when
two libraries race each other. If the pair is between your system and a library's, order yours.

**Can I use `DefaultPlugins`?** Not in this app (it already has the minimal set, and a window or
renderer is not headless). Add the individual plugins your code needs.

**Is time real anywhere?** Only `real_pause`, and only where you ask for it (bevy_net_session_test's
loopback builder sets one). Everything else is simulated.

**`Startup` is not checked by default.** It runs once; add it with `.strict(Startup)`.

**Messages written by observers or in `Last` are recorded?** Yes: the watcher reads after the
whole frame.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

## Contributing

Issues and pull requests are welcome. Before sending a change, run:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
```

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
