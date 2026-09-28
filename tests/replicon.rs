//! Replicon helpers (feature `replicon`).

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy_headless_test::prelude::*;
use bevy_headless_test::replicon::bevy_replicon::prelude::*;
use bevy_headless_test::replicon::{assert_same_protocol, protocol_hash, replicon_app, replicon_builder};
use serde::{Deserialize, Serialize};

#[derive(Event, Serialize, Deserialize, Clone, Debug)]
struct Ping;

#[derive(Component, Serialize, Deserialize, Clone, Debug)]
struct Alpha;

#[derive(Component, Serialize, Deserialize, Clone, Debug)]
struct Beta;

#[derive(Resource, Default)]
struct Received(usize);

fn count_pings(ping: On<FromClient<Ping>>, mut received: ResMut<Received>) {
    assert_eq!(ping.client_id, ClientId::Server, "no connection: delivered locally");
    received.0 += 1;
}

fn register_ping(app: &mut App) {
    app.add_client_event::<Ping>(Channel::Ordered).init_resource::<Received>().add_observer(count_pings);
}

fn send_ping(mut commands: Commands) {
    commands.client_trigger(Ping);
}

#[test]
fn a_replicon_app_delivers_a_client_event_locally() {
    let mut app = replicon_app(register_ping);
    app.add_systems(Startup, send_ping);
    app.step_n(3);
    assert_eq!(app.world().resource::<Received>().0, 1);
}

#[test]
fn a_bare_app_that_is_never_finished_does_not() {
    // The trap this kit exists for: `update()` without `finish()` + `cleanup()`.
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, RepliconPlugins));
    register_ping(&mut app);
    app.add_systems(Startup, send_ping);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().resource::<Received>().0, 0);
    assert!(protocol_hash(&app).is_none(), "no finish(), no protocol hash");
}

#[test]
fn a_replicon_app_passes_the_strict_check() {
    let mut app = replicon_app(register_ping);
    let found = app.ambiguities();
    assert!(found.is_empty(), "{found:#?}");
    app.step_n(5);
}

fn stack(order_ab: bool) -> TestApp {
    replicon_app(|app| {
        if order_ab {
            app.replicate::<Alpha>().replicate::<Beta>();
        } else {
            app.replicate::<Beta>().replicate::<Alpha>();
        }
    })
}

#[test]
fn the_same_registrations_give_the_same_hash() {
    let (a, b) = (stack(true), stack(true));
    assert!(protocol_hash(&a).is_some());
    assert_same_protocol(&a, &b);
}

#[test]
fn a_different_registration_order_gives_a_different_hash() {
    assert_ne!(protocol_hash(&stack(true)), protocol_hash(&stack(false)));
}

#[test]
#[should_panic(expected = "replicon protocol hashes differ")]
fn assert_same_protocol_fails_on_a_different_order() {
    assert_same_protocol(&stack(true), &stack(false));
}

#[test]
#[should_panic(expected = "app B has no replicon protocol hash")]
fn assert_same_protocol_names_an_unfinished_app() {
    let unfinished = replicon_builder().build();
    assert_same_protocol(&stack(true), &unfinished);
}
