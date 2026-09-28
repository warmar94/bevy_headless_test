//! The strict headless app: the ambiguity check, deterministic time, frame stepping, message and
//! state helpers.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

use bevy::prelude::*;
use bevy_headless_test::prelude::*;

#[derive(Resource, Default)]
struct Score(u32);

#[derive(Resource, Default)]
struct Other(u32);

fn add_one(mut score: ResMut<Score>) {
    score.0 += 1;
}

fn double(mut score: ResMut<Score>) {
    score.0 *= 2;
}

fn bump_other(mut other: ResMut<Other>) {
    other.0 += 1;
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

fn ambiguous_app(builder: TestAppBuilder) -> TestApp {
    let mut app = builder.build();
    app.init_resource::<Score>().add_systems(Update, (add_one, double));
    app
}

#[test]
#[should_panic(expected = "with no order between them")]
fn an_unordered_conflicting_pair_fails_the_first_frame() {
    ambiguous_app(TestApp::builder()).step();
}

#[test]
fn the_failure_names_the_schedule_both_systems_and_the_data() {
    let err = catch_unwind(AssertUnwindSafe(|| {
        ambiguous_app(TestApp::builder()).step();
    }))
    .expect_err("the ambiguous pair must fail");
    let msg = panic_message(err);
    println!(">>> {msg}");
    assert!(msg.contains("Update"), "{msg}");
    assert!(msg.contains("add_one") && msg.contains("double"), "{msg}");
    assert!(msg.contains("Score"), "{msg}");
}

#[test]
fn ambiguities_lists_the_pair_without_panicking() {
    let mut app = ambiguous_app(TestApp::builder());
    let found = app.ambiguities();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].schedule, "Update");
    assert!(found[0].conflicts.iter().any(|c| c.ends_with("Score")));
}

#[test]
fn an_ordered_pair_passes() {
    let mut app = TestApp::new();
    app.init_resource::<Score>().add_systems(Update, (add_one, double).chain());
    app.step_n(3);
    // 0 -> 1 -> 2, 2 -> 3 -> 6, 6 -> 7 -> 14
    assert_eq!(app.world().resource::<Score>().0, 14);
}

#[test]
fn an_allow_listed_pair_is_not_a_failure() {
    let mut app = ambiguous_app(TestApp::builder().allow_pair("double", "add_one"));
    app.step_n(2);
    assert!(app.ambiguities().is_empty());
}

#[test]
fn every_allow_rule_kind_covers_the_pair() {
    let rules = [
        TestApp::builder().allow_pair_in(Update, "add_one", "double"),
        TestApp::builder().allow_system("double"),
        TestApp::builder().allow_internal("strict"),
        TestApp::builder().allow_data("Score"),
        TestApp::builder().allow_resource::<Score>(),
        TestApp::builder().lenient(Update),
        TestApp::builder().no_ambiguity_check(),
    ];
    for builder in rules {
        let label = format!("{builder:?}");
        let mut app = ambiguous_app(builder);
        app.step();
        assert!(app.ambiguities().is_empty(), "{label}");
    }
}

#[test]
fn a_rule_for_another_pair_or_schedule_does_not_cover_it() {
    for builder in [
        TestApp::builder().allow_pair("add_one", "bump_other"),
        TestApp::builder().allow_pair_in(PostUpdate, "add_one", "double"),
        TestApp::builder().allow_data("Other"),
    ] {
        let mut app = ambiguous_app(builder);
        app.init_resource::<Other>().add_systems(Update, bump_other);
        assert_eq!(app.ambiguities().len(), 1);
    }
}

#[test]
fn fixed_update_is_checked_too() {
    let mut app = TestApp::new();
    app.init_resource::<Score>().add_systems(FixedUpdate, (add_one, double));
    let found = app.ambiguities();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].schedule, "FixedUpdate");
}

#[test]
fn a_schedule_added_later_is_checked_when_strict() {
    #[derive(bevy::ecs::schedule::ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
    struct Custom;
    let mut app = TestApp::builder().strict(Custom).build();
    app.init_resource::<Score>().add_systems(Custom, (add_one, double));
    assert_eq!(app.ambiguities().len(), 1);
}

#[test]
fn time_advances_by_exactly_one_frame_per_step_from_the_first() {
    let mut app = TestApp::new();
    app.step();
    assert_eq!(app.elapsed(), DEFAULT_FRAME, "frame 1 already advances time");
    app.step_n(9);
    assert_eq!(app.elapsed(), DEFAULT_FRAME * 10);
    assert_eq!(app.frame_count(), 10);
}

#[test]
fn time_stepping_is_deterministic() {
    #[derive(Resource, Default)]
    struct Trace(Vec<Duration>);
    let run = || {
        let mut app = TestApp::builder().frame_duration(Duration::from_millis(20)).build();
        app.init_resource::<Trace>().add_systems(Update, |time: Res<Time>, mut trace: ResMut<Trace>| trace.0.push(time.elapsed()));
        app.step_secs(1.0);
        std::mem::take(&mut app.world_mut().resource_mut::<Trace>().0)
    };
    let (a, b) = (run(), run());
    assert_eq!(a.len(), 50, "1 s at 20 ms is 50 frames");
    assert_eq!(a, b);
    assert_eq!(a.last().copied(), Some(Duration::from_secs(1)));
}

#[test]
fn fixed_update_runs_once_per_default_frame() {
    let mut app = TestApp::new();
    app.init_resource::<Score>().add_systems(FixedUpdate, add_one);
    app.step_n(8);
    assert_eq!(app.world().resource::<Score>().0, 8);
}

#[test]
fn step_secs_rounds_up_to_whole_frames() {
    let mut app = TestApp::new();
    assert_eq!(app.step_secs(0.5), 32, "0.5 s at 1/64 s");
    assert_eq!(app.step_secs(0.01), 1);
    assert_eq!(app.step_secs(0.0), 0);
}

#[test]
#[should_panic(expected = "must be finite")]
fn step_secs_refuses_nan() {
    TestApp::new().step_secs(f32::NAN);
}

#[test]
fn run_until_returns_the_frames_it_took() {
    let mut app = TestApp::new();
    app.init_resource::<Score>().add_systems(Update, add_one);
    let frames = app.run_until(|world| world.resource::<Score>().0 >= 5, 100);
    assert_eq!(frames, 5);
}

#[test]
#[should_panic(expected = "still false after 10 frames")]
fn run_until_fails_when_the_condition_never_holds() {
    TestApp::new().run_until(|_| false, 10);
}

#[test]
fn finish_runs_plugin_finish_before_the_first_frame() {
    #[derive(Resource)]
    struct Finished;
    struct Late;
    impl Plugin for Late {
        fn build(&self, _: &mut App) {}
        fn finish(&self, app: &mut App) {
            app.insert_resource(Finished);
        }
    }
    let mut app = TestApp::new();
    app.add_plugins(Late);
    assert!(!app.world().contains_resource::<Finished>());
    app.step();
    assert!(app.world().contains_resource::<Finished>());
}

// ---------------------------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------------------------

#[derive(Message, Clone, Debug, PartialEq)]
struct Ping(u32);

#[derive(Message, Clone, Debug, PartialEq)]
struct Pong(u32);

fn answer(mut pings: MessageReader<Ping>, mut pongs: MessageWriter<Pong>) {
    for ping in pings.read() {
        pongs.write(Pong(ping.0 * 10));
    }
}

fn ping_pong() -> TestApp {
    let mut app = TestApp::new();
    app.add_message::<Ping>().add_message::<Pong>().add_systems(Update, answer);
    app.watch::<Pong>();
    app
}

#[test]
fn messages_are_counted_per_frame() {
    let mut app = ping_pong();
    app.send(Ping(1)).step();
    assert_eq!(app.assert_exactly_one::<Pong>(), Pong(10));
    app.step();
    app.assert_none::<Pong>();
    app.send(Ping(2)).send(Ping(3)).step();
    assert_eq!(app.assert_count::<Pong>(2), vec![Pong(20), Pong(30)]);
    assert_eq!(app.all_messages::<Pong>().len(), 3);
}

#[test]
fn messages_since_a_mark_span_frames() {
    let mut app = ping_pong();
    app.send_and_step(Ping(1));
    let mark = app.mark();
    app.assert_none_since::<Pong>(mark);
    app.step().send(Ping(2)).step().step();
    assert_eq!(app.assert_exactly_one_since::<Pong>(mark), Pong(20));
    assert_eq!(app.messages_since::<Pong>(Mark(0)), vec![Pong(10), Pong(20)]);
}

#[test]
fn a_message_is_seen_once_even_though_it_lives_two_frames() {
    let mut app = ping_pong();
    app.send(Ping(7)).step_n(4);
    assert_eq!(app.all_messages::<Pong>(), vec![Pong(70)]);
}

#[test]
#[should_panic(expected = "expected 1 `Pong` in frame 1, got 0")]
fn assert_exactly_one_names_the_type_and_the_count() {
    let mut app = ping_pong();
    app.step();
    app.assert_exactly_one::<Pong>();
}

#[test]
#[should_panic(expected = "not watched")]
fn reading_an_unwatched_message_says_how_to_fix_it() {
    let mut app = ping_pong();
    app.step();
    app.assert_none::<Ping>();
}

#[test]
#[should_panic(expected = "not registered")]
fn sending_an_unregistered_message_says_so() {
    TestApp::new().send(Ping(1));
}

// ---------------------------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------------------------

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Screen {
    #[default]
    Menu,
    Game,
}

#[test]
fn states_are_set_and_asserted() {
    let mut app = TestApp::new();
    app.init_state::<Screen>();
    app.step();
    app.assert_state(Screen::Menu);
    app.set_state_and_step(Screen::Game);
    app.assert_state(Screen::Game);
    assert_eq!(app.state::<Screen>(), Some(Screen::Game));
}

#[test]
fn run_until_state_waits_for_the_transition() {
    let mut app = TestApp::new();
    app.init_state::<Screen>();
    app.set_state(Screen::Game);
    assert_eq!(app.run_until_state(Screen::Game, 5), 1);
}

#[test]
#[should_panic(expected = "state `Screen` is Menu, expected Game")]
fn a_wrong_state_names_both_values() {
    let mut app = TestApp::new();
    app.init_state::<Screen>();
    app.step();
    app.assert_state(Screen::Game);
}

#[test]
#[should_panic(expected = "not initialised")]
fn a_missing_state_says_so() {
    TestApp::new().set_state(Screen::Game);
}
