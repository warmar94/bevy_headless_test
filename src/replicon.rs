//! Replicon helpers (feature `replicon`): a test app with replicon's plugins that is finished
//! before the first frame, and protocol-hash comparison.
//!
//! Replicon computes its protocol hash and wires local message delivery in `Plugin::finish`. A
//! hand-built test `App` that is only ever `update()`d never runs `finish()`, so client events
//! written with no connection are never delivered locally and the protocol hash never exists.
//! [`TestApp`] runs `finish()` + `cleanup()` itself (on [`TestApp::finish`] or the first frame);
//! [`replicon_app`] does it right after your registrations.

use bevy_app::App;
use bevy_replicon::prelude::RepliconPlugins;
use bevy_replicon::shared::protocol::ProtocolHash;

use crate::app::{TestApp, TestAppBuilder};

/// The bevy_replicon this module is built on (use it to be sure your versions match).
pub use bevy_replicon;

/// The crates whose systems race each other inside a replicon app, none of them yours:
/// replicon itself and `bevy_time` (see [`replicon_builder`]).
pub const REPLICON_THIRD_PARTY: [&str; 2] = ["bevy_replicon", "bevy_time"];

/// A [`TestAppBuilder`] with `RepliconPlugins` (no transport: with no connection, client events
/// and messages are delivered locally as from `ClientId::Server`, and server ones to the local
/// client). Pairs among replicon's own systems, and between them and `bevy_time`'s (replicon's
/// exclusive receive system and Bevy's delayed-command check in `PreUpdate`), are allowed through
/// [`REPLICON_THIRD_PARTY`]; pairs between your systems and theirs are still checked.
///
/// The app is NOT finished: register your protocol, then [`TestApp::finish`] or step.
pub fn replicon_builder() -> TestAppBuilder {
    TestApp::builder().allow_among(REPLICON_THIRD_PARTY).setup(|app| {
        app.add_plugins(RepliconPlugins);
    })
}

/// A finished replicon test app: [`replicon_builder`], then `register` (your protocol:
/// `add_client_event`, `replicate`, ...), then `finish()` + `cleanup()`.
///
/// ```
/// use bevy::prelude::*;
/// use bevy_headless_test::replicon::{bevy_replicon::prelude::*, protocol_hash, replicon_app};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Event, Serialize, Deserialize, Clone)]
/// struct Ping;
///
/// let app = replicon_app(|app| {
///     app.add_client_event::<Ping>(Channel::Ordered);
/// });
/// assert!(protocol_hash(&app).is_some());
/// ```
pub fn replicon_app(register: impl FnOnce(&mut App)) -> TestApp {
    let mut app = replicon_builder().build();
    register(&mut app);
    app.finish();
    app
}

/// Replicon's protocol hash of `app`: every replicated component, event and message it
/// registered, in order, plus custom data. `None` until the app was finished (the hash is
/// computed in `Plugin::finish`) or when it has no replicon.
pub fn protocol_hash(app: &App) -> Option<ProtocolHash> {
    app.world().get_resource::<ProtocolHash>().copied()
}

/// Assert two apps (a client and a server stack, say) registered the same protocol, in the same
/// order.
///
/// # Panics
///
/// If either app has no hash yet, or the hashes differ.
pub fn assert_same_protocol(a: &App, b: &App) {
    let hash_a = protocol_hash(a);
    let hash_b = protocol_hash(b);
    let missing = |which: &str| {
        format!("app {which} has no replicon protocol hash: finish it first (`TestApp::finish()` or one step), and make sure it has RepliconPlugins")
    };
    let Some(hash_a) = hash_a else { panic!("{}", missing("A")) };
    let Some(hash_b) = hash_b else { panic!("{}", missing("B")) };
    assert!(
        hash_a == hash_b,
        "replicon protocol hashes differ ({hash_a:?} vs {hash_b:?}): the two apps did not register the same replicated components, \
         events and messages in the same order. Register them in ONE shared function or plugin that both stacks call, at the \
         same point of the plugin order."
    );
}
