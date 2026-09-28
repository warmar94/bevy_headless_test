//! A test kit for Bevy apps: strict, headless, deterministic.
//!
//! [`TestApp`] is a windowless [`App`](bevy_app::App) with Bevy's minimal plugins, states,
//! deterministic time and a strict ambiguity check: every unordered pair of systems that
//! conflicts on the same data fails the test, naming the schedule, both systems and the data
//! they fight over. Known third-party pairs go on an allow-list.
//!
//! On top of it:
//!
//! - frame stepping: [`TestApp::step`], [`TestApp::step_n`], [`TestApp::step_secs`],
//!   [`TestApp::run_until`];
//! - message helpers: [`TestApp::watch`], [`TestApp::send`], [`TestApp::messages`],
//!   [`TestApp::assert_exactly_one`], [`TestApp::assert_none`], marks for "since";
//! - state helpers: [`TestApp::set_state`], [`TestApp::state`], [`TestApp::assert_state`];
//! - UI text read-back (feature `ui`): [`UiText`], [`TestApp::ui_text`];
//! - replicon (feature `replicon`): [`replicon::replicon_app`], [`replicon::protocol_hash`],
//!   [`replicon::assert_same_protocol`];
//! - UDP loopback (feature `net_session`): [`loopback::host`], [`loopback::join`],
//!   [`loopback::connect`].
//!
//! It is a test crate: the `assert_*` / `check_*` / `step*` functions panic with a message that
//! says what went wrong and where. Everything else returns `Option` / `Result` and never panics.
#![warn(missing_docs)]

mod ambiguity;
mod app;
mod messages;
mod state;
#[cfg(feature = "ui")]
pub mod text;

#[cfg(feature = "net_session")]
pub mod loopback;
#[cfg(feature = "replicon")]
pub mod replicon;

/// Every Rust example in the README compiles (checked by `cargo test --all-features`).
#[cfg(all(doctest, feature = "ui", feature = "replicon", feature = "net_session"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use ambiguity::{AllowRule, Ambiguity, AmbiguityPolicy};
pub use app::{default_strict_schedules, TestApp, TestAppBuilder, DEFAULT_FRAME};
pub use messages::Mark;
#[cfg(feature = "ui")]
pub use text::{TextEntry, TextKind, UiText};

/// Everything a test usually needs: `use bevy_headless_test::prelude::*;`.
pub mod prelude {
    pub use crate::{AllowRule, Ambiguity, AmbiguityPolicy, Mark, TestApp, TestAppBuilder, DEFAULT_FRAME};
    #[cfg(feature = "ui")]
    pub use crate::{TextEntry, TextKind, UiText};
}
