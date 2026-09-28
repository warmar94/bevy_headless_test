//! A host and a client in one process over REAL UDP loopback (feature `net_session`), through
//! [`bevy_net_session`]'s `ip` transport.
//!
//! Both apps are ordinary [`TestApp`]s with `NetSessionPlugin` added (same protocol version, same
//! registrations). [`host`] starts hosting on a free port; [`join`] joins it and steps both apps in
//! lock-step (with a short real sleep per frame, so packets arrive) until the host sees the
//! client and the client is joined; [`connect`] does both.
//!
//! ```no_run
//! use bevy_headless_test::loopback::{self, bevy_net_session::NetSessionPlugin};
//!
//! // Your shared protocol registration goes in `setup`, identical on both sides.
//! let stack = || loopback::net_builder(NetSessionPlugin { protocol_version: 1, ..Default::default() }).build();
//! let (mut host, mut client) = (stack(), stack());
//! loopback::connect(&mut host, &mut client);
//! ```

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use bevy_ecs::message::{MessageCursor, Messages};
use bevy_net_session::{HostFailed, HostSession, JoinFailed, JoinSession, NetSession, NetSessionPlugin, SessionPeer};

use crate::app::{TestApp, TestAppBuilder};
use crate::replicon::{replicon_builder, REPLICON_THIRD_PARTY};

/// The bevy_net_session this module is built on.
pub use bevy_net_session;

/// The crates whose systems race each other inside a networked app, none of them yours: those of
/// [`REPLICON_THIRD_PARTY`] plus replicon's renet backend (its state setters in `PreUpdate`). The
/// session layer's own systems stay checked.
pub const NET_THIRD_PARTY: [&str; 3] = [REPLICON_THIRD_PARTY[0], REPLICON_THIRD_PARTY[1], "bevy_replicon_renet"];

/// A [`TestAppBuilder`] for one peer: [`replicon_builder`] + `plugin` + a [`LOOPBACK_PAUSE`]
/// real pause per frame, with [`NET_THIRD_PARTY`]'s pairs allowed. Add your shared protocol with
/// `.setup(..)`, identically on every peer.
pub fn net_builder(plugin: NetSessionPlugin) -> TestAppBuilder {
    replicon_builder().allow_among(NET_THIRD_PARTY).real_pause(LOOPBACK_PAUSE).setup(move |app| {
        app.add_plugins(plugin);
    })
}

/// The real sleep between loopback frames (at least; a longer `real_pause` of either app wins).
pub const LOOPBACK_PAUSE: Duration = Duration::from_millis(1);

/// How many frames [`connect`] waits for a join (a loopback join takes a handful).
pub const DEFAULT_JOIN_FRAMES: u32 = 600;

fn session(app: &TestApp, which: &str) -> NetSession {
    match app.world().get_resource::<NetSession>() {
        Some(session) => session.clone(),
        None => panic!("the {which} app has no NetSession: add `NetSessionPlugin` to it"),
    }
}

/// Start hosting on a free UDP port (one frame); returns the loopback address to join.
///
/// # Panics
///
/// If the app has no `NetSessionPlugin` or hosting failed (the `HostFailed` reason is shown).
pub fn host(host: &mut TestApp, max_clients: usize) -> SocketAddr {
    let mut failures = host.world().get_resource::<Messages<HostFailed>>().map(|m| m.get_cursor()).unwrap_or_default();
    host.send(HostSession::ip(0, max_clients));
    host.step();
    let state = session(host, "host");
    match state.local_addr() {
        Some(addr) if state.is_host() => SocketAddr::from((Ipv4Addr::LOCALHOST, addr.port())),
        _ => {
            let reason = host
                .world()
                .get_resource::<Messages<HostFailed>>()
                .and_then(|m| failures.read(m).last().map(|f| format!("{:?}: {}", f.reason, f.message)))
                .unwrap_or_else(|| "no SessionStarted and no HostFailed".to_string());
            panic!("hosting on UDP loopback failed: {reason}");
        }
    }
}

/// Join `addr` from `client` with `payload` and step both apps until the client is joined and the
/// host has its peer; returns the frames it took.
///
/// # Panics
///
/// If the join fails (the `JoinFailed` reason is shown) or does not finish in `max_frames`.
pub fn join(host: &mut TestApp, client: &mut TestApp, addr: SocketAddr, payload: impl Into<Vec<u8>>, max_frames: u32) -> u32 {
    let mut failures: MessageCursor<JoinFailed> = client.world().get_resource::<Messages<JoinFailed>>().map(|m| m.get_cursor()).unwrap_or_default();
    client.send(JoinSession::ip(addr).with_payload(payload));
    for frame in 0..max_frames {
        if let Some(failed) = client.world().get_resource::<Messages<JoinFailed>>().and_then(|m| failures.read(m).last().cloned()) {
            panic!("joining {addr} over UDP loopback failed after {frame} frames: {:?}: {}", failed.reason, failed.message);
        }
        let joined = session(client, "client");
        if joined.is_joined() {
            if let Some(id) = joined.local_id() {
                let world = host.world_mut();
                let mut peers = world.query::<&SessionPeer>();
                if peers.iter(world).any(|p| p.id == id) {
                    return frame;
                }
            }
        }
        for app in [&mut *host, &mut *client] {
            app.step_once();
        }
        let pause = LOOPBACK_PAUSE.max(host.real_pause()).max(client.real_pause());
        std::thread::sleep(pause);
    }
    panic!(
        "joining {addr} over UDP loopback did not finish in {max_frames} frames (client state {:?}, host state {:?})",
        session(client, "client").state(),
        session(host, "host").state()
    );
}

/// [`host`] (up to 4 clients) + [`join`] with an empty payload; returns the host's address.
pub fn connect(host_app: &mut TestApp, client: &mut TestApp) -> SocketAddr {
    let addr = host(host_app, 4);
    join(host_app, client, addr, Vec::new(), DEFAULT_JOIN_FRAMES);
    addr
}
