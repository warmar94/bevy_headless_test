# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (before 1.0, a breaking
change or a Bevy / bevy_replicon bump raises the minor version).

## [0.1.0] - Unreleased

First release, for Bevy 0.19.0 and, optionally, bevy_replicon 0.44.2 and bevy_net_session 0.1.

### Added

- `TestApp`: a headless app with Bevy's minimal plugins (task pools, frame count, time) and
  `StatesPlugin`; dereferences to `App`. `TestAppBuilder` for the configuration.
- The strict ambiguity check: before every frame, the checked schedules (default `First`,
  `PreUpdate`, `Update`, `FixedUpdate`, `PostUpdate`, `Last`) are built and every unordered pair
  of conflicting systems fails the test with the schedule, both full system paths and the data.
  Allow-list: `allow_pair`, `allow_pair_in`, `allow_system`, `allow_internal`, `allow_among`,
  `allow_data`, `allow_resource`, `allow_component`, `allow(AllowRule)`; `strict`, `lenient`,
  `strict_schedules`, `no_ambiguity_check`. `ambiguities()` lists uncovered pairs without
  panicking; `check_ambiguities()`.
- Deterministic time (`TimeUpdateStrategy::ManualDuration`, default 1/64 s, a full frame from
  the first frame on) and stepping: `step`, `step_n`, `step_secs`, `run_until`, `step_together`,
  `run_together_until`; `real_pause` for tests on real sockets.
- `App::finish()` + `App::cleanup()` before the first frame (or on `finish()`).
- Message helpers: `watch`, `send`, `send_and_step`, `mark`, `messages`, `messages_since`,
  `all_messages`, `assert_count`, `assert_exactly_one`, `assert_none`,
  `assert_exactly_one_since`, `assert_none_since`.
- State helpers: `state`, `set_state`, `set_state_and_step`, `assert_state`, `run_until_state`.
- UI text read-back (feature `ui`, default): `UiText` over `Text`, `TextSpan` and `Text2d`,
  whole world or under a root, with `assert_all_ascii`, `assert_charset`, `assert_chars`,
  `assert_at_least`, `assert_contains`; failures name the entity, its `Name`, the character and
  the string.
- Feature `replicon`: `replicon_app`, `replicon_builder`, `protocol_hash`,
  `assert_same_protocol`, `REPLICON_THIRD_PARTY`.
- Feature `net_session`: `loopback::net_builder`, `host`, `join`, `connect` (a host and a client
  in one process over real UDP loopback), `NET_THIRD_PARTY`.
- Example `quick_start`.
