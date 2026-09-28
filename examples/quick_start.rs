//! A typical system test, written as an example so it runs with `cargo run --example quick_start`.
//! In a real project the body of `main` is a `#[test]` function in your crate.
//!
//! The "game": a `Hit` message lowers `Health`; at zero, a `Died` message is written and the
//! game moves to `Phase::GameOver`.

use bevy::prelude::*;
use bevy_headless_test::prelude::*;

#[derive(Message, Clone, Debug)]
struct Hit(u32);

#[derive(Message, Clone, Debug, PartialEq)]
struct Died;

#[derive(Resource)]
struct Health(u32);

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Phase {
    #[default]
    Playing,
    GameOver,
}

/// Both systems touch `Health`; `.chain()` orders them. Remove it and the test app fails with
/// the schedule, both system names and `Health`.
fn apply_hits(mut hits: MessageReader<Hit>, mut health: ResMut<Health>) {
    for hit in hits.read() {
        health.0 = health.0.saturating_sub(hit.0);
    }
}

fn check_death(health: Res<Health>, mut died: MessageWriter<Died>, mut next: ResMut<NextState<Phase>>) {
    if health.is_changed() && health.0 == 0 {
        died.write(Died);
        next.set(Phase::GameOver);
    }
}

struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Phase>().add_message::<Hit>().add_message::<Died>().insert_resource(Health(10)).add_systems(Update, (apply_hits, check_death).chain());
    }
}

fn main() {
    // Strict (ambiguity check on First..Last), headless, 1/64 s per frame.
    let mut app = TestApp::new();
    app.add_plugins(GamePlugin);
    app.watch::<Died>();

    app.send(Hit(4)).step();
    app.assert_none::<Died>();
    assert_eq!(app.world().resource::<Health>().0, 6);

    let before = app.mark();
    app.send(Hit(6)).step();
    app.assert_exactly_one::<Died>();
    // The transition queued this frame is applied in the next frame's StateTransition.
    app.step();
    app.assert_state(Phase::GameOver);
    app.assert_exactly_one_since::<Died>(before);

    // Deterministic time: 3 frames so far, each exactly 1/64 s.
    assert_eq!(app.elapsed(), DEFAULT_FRAME * 3);
    println!(">>> quick_start: {} frames, {:?} simulated, one Died, now {:?}", app.frame_count(), app.elapsed(), app.state::<Phase>());
}
