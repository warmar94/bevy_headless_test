//! A host and a client in one process over real UDP loopback (feature `net_session`).

use bevy::prelude::*;
use bevy_headless_test::loopback::bevy_net_session::{NetSession, NetSessionPlugin, SessionState};
use bevy_headless_test::loopback::{self, net_builder, DEFAULT_JOIN_FRAMES};
use bevy_headless_test::prelude::*;
use bevy_headless_test::replicon::assert_same_protocol;
use bevy_headless_test::replicon::bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Event, Serialize, Deserialize, Clone, Debug)]
struct Ping;

/// Pings the host received, and from whom.
#[derive(Resource, Default)]
struct Received(Vec<ClientId>);

fn peer(version: u64) -> TestApp {
    net_builder(NetSessionPlugin { protocol_version: version, ..Default::default() })
        .setup(|app| {
            // The shared protocol: identical on both peers.
            app.add_client_event::<Ping>(Channel::Ordered)
                .init_resource::<Received>()
                .add_observer(|ping: On<FromClient<Ping>>, mut received: ResMut<Received>| received.0.push(ping.client_id));
        })
        .build()
}

fn state(app: &TestApp) -> SessionState {
    app.world().resource::<NetSession>().state()
}

#[test]
fn a_client_joins_a_host_over_udp_loopback() {
    let (mut host, mut client) = (peer(1), peer(1));
    let addr = loopback::connect(&mut host, &mut client);
    println!(">>> joined {addr} after {} client frames", client.frame_count());
    assert!(addr.ip().is_loopback() && addr.port() != 0);
    assert_eq!(state(&client), SessionState::Joined);
    assert_eq!(state(&host), SessionState::Listening);
    assert_same_protocol(&host, &client);
}

#[test]
fn a_client_event_crosses_the_wire() {
    let (mut host, mut client) = (peer(1), peer(1));
    loopback::connect(&mut host, &mut client);
    client.world_mut().commands().client_trigger(Ping);
    let frames = TestApp::run_together_until(&mut [&mut host, &mut client], |apps| !apps[0].world().resource::<Received>().0.is_empty(), 200);
    let from = &host.world().resource::<Received>().0;
    println!(">>> the host received {from:?} after {frames} frames");
    assert_eq!(from.len(), 1);
    assert_ne!(from[0], ClientId::Server, "from the remote client, not a local delivery");
    assert!(client.world().resource::<Received>().0.is_empty(), "a connected client does not deliver to itself");
}

#[test]
#[should_panic(expected = "VersionMismatch")]
fn a_client_on_another_protocol_version_fails_with_the_reason() {
    let (mut host, mut client) = (peer(1), peer(2));
    let addr = loopback::host(&mut host, 4);
    loopback::join(&mut host, &mut client, addr, Vec::new(), DEFAULT_JOIN_FRAMES);
}
