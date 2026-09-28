//! [`TestApp`]: the strict headless app and its frame stepping.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::time::Duration;

use bevy_app::{App, First, FixedUpdate, Last, PluginsState, PostUpdate, PreUpdate, TaskPoolPlugin, Update};
use bevy_diagnostic::FrameCountPlugin;
use bevy_ecs::component::Component;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel};
use bevy_ecs::world::World;
use bevy_state::app::StatesPlugin;
use bevy_time::{Real, Time, TimePlugin, TimeUpdateStrategy};

use crate::ambiguity::{self, AllowRule, Ambiguity, AmbiguityPolicy};
use crate::messages::AnyWatcher;

/// The default simulated frame: 1/64 s (15.625 ms), exactly Bevy's default fixed timestep, so
/// `FixedUpdate` runs exactly once per frame.
pub const DEFAULT_FRAME: Duration = Duration::from_micros(15_625);

/// How many times a plugin that is not `ready()` is polled before the first frame gives up.
const READY_POLLS: u32 = 1_000;

/// The schedules checked for ambiguities by default: `First`, `PreUpdate`, `Update`,
/// `FixedUpdate`, `PostUpdate`, `Last`.
pub fn default_strict_schedules() -> Vec<InternedScheduleLabel> {
    vec![First.intern(), PreUpdate.intern(), Update.intern(), FixedUpdate.intern(), PostUpdate.intern(), Last.intern()]
}

type Setup = Box<dyn FnOnce(&mut App)>;

/// Configures a [`TestApp`]. Start with [`TestApp::builder`].
pub struct TestAppBuilder {
    policy: AmbiguityPolicy,
    frame: Duration,
    real_pause: Duration,
    setup: Vec<Setup>,
}

impl fmt::Debug for TestAppBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestAppBuilder")
            .field("policy", &self.policy)
            .field("frame", &self.frame)
            .field("real_pause", &self.real_pause)
            .field("setup", &self.setup.len())
            .finish()
    }
}

impl Default for TestAppBuilder {
    fn default() -> Self {
        Self {
            policy: AmbiguityPolicy { schedules: default_strict_schedules(), rules: Vec::new(), enabled: true },
            frame: DEFAULT_FRAME,
            real_pause: Duration::ZERO,
            setup: Vec::new(),
        }
    }
}

impl TestAppBuilder {
    /// The simulated time every frame advances by (default [`DEFAULT_FRAME`]). Keep it at or
    /// below 250 ms: Bevy's virtual clock clamps a longer frame (`Time<Virtual>::max_delta`).
    pub fn frame_duration(mut self, frame: Duration) -> Self {
        self.frame = frame;
        self
    }

    /// A REAL sleep after every frame (default none). Only for tests that talk to the OS (real
    /// sockets), where packets need wall-clock time to arrive.
    pub fn real_pause(mut self, pause: Duration) -> Self {
        self.real_pause = pause;
        self
    }

    /// Replace the checked schedules (default [`default_strict_schedules`]).
    pub fn strict_schedules(mut self, labels: impl IntoIterator<Item = InternedScheduleLabel>) -> Self {
        self.policy.schedules = labels.into_iter().collect();
        self
    }

    /// Also check this schedule.
    pub fn strict(mut self, label: impl ScheduleLabel) -> Self {
        let label = label.intern();
        if !self.policy.schedules.contains(&label) {
            self.policy.schedules.push(label);
        }
        self
    }

    /// Do not check this schedule.
    pub fn lenient(mut self, label: impl ScheduleLabel) -> Self {
        let label = label.intern();
        self.policy.schedules.retain(|l| *l != label);
        self
    }

    /// Turn the ambiguity check off entirely.
    pub fn no_ambiguity_check(mut self) -> Self {
        self.policy.enabled = false;
        self
    }

    /// Allow one pair of systems (name patterns, either order) in every checked schedule. See
    /// [`AllowRule`] for how a pattern matches.
    pub fn allow_pair(self, a: impl Into<String>, b: impl Into<String>) -> Self {
        self.allow(AllowRule::Pair { schedule: None, a: a.into(), b: b.into() })
    }

    /// Allow one pair of systems in one schedule only.
    pub fn allow_pair_in(self, schedule: impl ScheduleLabel, a: impl Into<String>, b: impl Into<String>) -> Self {
        self.allow(AllowRule::Pair { schedule: Some(schedule.intern()), a: a.into(), b: b.into() })
    }

    /// Allow every pair that involves this system.
    pub fn allow_system(self, name: impl Into<String>) -> Self {
        self.allow(AllowRule::System { schedule: None, name: name.into() })
    }

    /// Allow every pair whose both systems belong to `prefix` (a crate or module path): a
    /// library's own internal pairs. See [`AllowRule::Among`].
    pub fn allow_internal(self, prefix: impl Into<String>) -> Self {
        self.allow(AllowRule::Among { prefixes: vec![prefix.into()] })
    }

    /// Allow every pair whose both systems belong to any of `prefixes` (crates or modules you do
    /// not own, such as a networking library and the Bevy crate it races with). See
    /// [`AllowRule::Among`].
    pub fn allow_among<S: Into<String>>(self, prefixes: impl IntoIterator<Item = S>) -> Self {
        self.allow(AllowRule::Among { prefixes: prefixes.into_iter().map(Into::into).collect() })
    }

    /// Allow conflicts on data whose type name matches `name` (by pattern, see [`AllowRule`]).
    pub fn allow_data(self, name: impl Into<String>) -> Self {
        self.allow(AllowRule::Data { name: name.into() })
    }

    /// Allow unordered access to one resource type, through Bevy's own mechanism
    /// (`App::allow_ambiguous_resource`), which also applies to schedules this kit does not check.
    pub fn allow_resource<R: Resource>(mut self) -> Self {
        self.setup.push(Box::new(|app: &mut App| {
            app.allow_ambiguous_resource::<R>();
        }));
        self
    }

    /// Allow unordered access to one component type, through Bevy's own mechanism
    /// (`App::allow_ambiguous_component`).
    pub fn allow_component<C: Component>(mut self) -> Self {
        self.setup.push(Box::new(|app: &mut App| {
            app.allow_ambiguous_component::<C>();
        }));
        self
    }

    /// Add any [`AllowRule`].
    pub fn allow(mut self, rule: AllowRule) -> Self {
        self.policy.rules.push(rule);
        self
    }

    /// Run `f` on the app while it is built (after the base plugins): add plugins, resources,
    /// systems.
    pub fn setup(mut self, f: impl FnOnce(&mut App) + 'static) -> Self {
        self.setup.push(Box::new(f));
        self
    }

    /// Build the app: Bevy's minimal plugins (task pools, frame count, time), `StatesPlugin`,
    /// `TimeUpdateStrategy::ManualDuration(frame)`, then every `setup` step in order.
    pub fn build(self) -> TestApp {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), FrameCountPlugin, TimePlugin, StatesPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(self.frame));
        for setup in self.setup {
            setup(&mut app);
        }
        TestApp { app, policy: self.policy, frame: self.frame, real_pause: self.real_pause, frames: 0, checked: HashSet::new(), watchers: HashMap::new() }
    }
}

/// A strict, headless, deterministic Bevy [`App`] for tests.
///
/// - Bevy's minimal plugins (task pools, frame count, time) + `StatesPlugin`; no window, no
///   renderer, no runner. Do not add `MinimalPlugins` again.
/// - Time advances by exactly [`TestAppBuilder::frame_duration`] every frame, from the first
///   frame on (Bevy's own first frame has a zero delta; this app's does not).
/// - Before every frame, the checked schedules are built and every unordered pair of conflicting
///   systems that no [`AllowRule`] covers fails the test, naming the schedule, the two systems
///   and the data.
/// - The first frame runs `App::finish` + `App::cleanup` (what `App::run` does and a bare
///   `app.update()` does not; without it, plugins that finish in `finish()` stay half-built).
///
/// It dereferences to [`App`], so `add_plugins`, `add_systems`, `world_mut()` and friends work
/// as usual. Step it through [`step`](Self::step) and friends; a bare `app.update()` skips the
/// checks and the message watchers.
pub struct TestApp {
    pub(crate) app: App,
    policy: AmbiguityPolicy,
    frame: Duration,
    real_pause: Duration,
    pub(crate) frames: u64,
    checked: HashSet<InternedScheduleLabel>,
    pub(crate) watchers: HashMap<TypeId, Box<dyn AnyWatcher>>,
}

impl fmt::Debug for TestApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestApp")
            .field("policy", &self.policy)
            .field("frame", &self.frame)
            .field("real_pause", &self.real_pause)
            .field("frames", &self.frames)
            .field("watched_messages", &self.watchers.len())
            .finish_non_exhaustive()
    }
}

impl Default for TestApp {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for TestApp {
    type Target = App;
    fn deref(&self) -> &App {
        &self.app
    }
}

impl DerefMut for TestApp {
    fn deref_mut(&mut self) -> &mut App {
        &mut self.app
    }
}

impl TestApp {
    /// A test app with the defaults (see [`TestAppBuilder`]).
    pub fn new() -> Self {
        TestAppBuilder::default().build()
    }

    /// Configure a test app.
    pub fn builder() -> TestAppBuilder {
        TestAppBuilder::default()
    }

    /// The inner [`App`].
    pub fn app(&self) -> &App {
        &self.app
    }

    /// The inner [`App`], mutably.
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Take the inner [`App`] out (the checks and watchers stay behind).
    pub fn into_app(self) -> App {
        self.app
    }

    /// The ambiguity policy in effect (schedules + allow-list); change it at any time.
    pub fn ambiguity_policy_mut(&mut self) -> &mut AmbiguityPolicy {
        &mut self.policy
    }

    /// The simulated time per frame.
    pub fn frame_duration(&self) -> Duration {
        self.frame
    }

    /// Change the simulated time per frame from the next frame on.
    pub fn set_frame_duration(&mut self, frame: Duration) -> &mut Self {
        self.frame = frame;
        self.app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        self
    }

    /// The real sleep after every frame (see [`TestAppBuilder::real_pause`]).
    pub fn real_pause(&self) -> Duration {
        self.real_pause
    }

    /// Frames stepped by this test app so far.
    pub fn frame_count(&self) -> u64 {
        self.frames
    }

    /// `Time::elapsed()` (the default clock; zero before the first frame).
    pub fn elapsed(&self) -> Duration {
        self.app.world().get_resource::<Time>().map(|t| t.elapsed()).unwrap_or_default()
    }

    /// Run `App::finish` + `App::cleanup` now, if they have not run yet (the first frame does
    /// it otherwise). Needed before reading anything plugins compute in `finish()`, such as
    /// replicon's protocol hash.
    ///
    /// # Panics
    ///
    /// If a plugin never reports `ready()`.
    pub fn finish(&mut self) -> &mut Self {
        let mut polls = 0;
        loop {
            match self.app.plugins_state() {
                PluginsState::Adding => {
                    polls += 1;
                    assert!(polls < READY_POLLS, "a plugin never became ready (`Plugin::ready` stayed false for {READY_POLLS} polls)");
                    std::thread::yield_now();
                }
                PluginsState::Ready => {
                    self.app.finish();
                    self.app.cleanup();
                    return self;
                }
                PluginsState::Finished => {
                    self.app.cleanup();
                    return self;
                }
                PluginsState::Cleaned => return self,
            }
        }
    }

    /// Every unordered conflicting pair in the checked schedules that the allow-list does not
    /// cover (builds the schedules first; does not panic on pairs).
    ///
    /// # Panics
    ///
    /// If a checked schedule cannot be built at all (a cycle, ...), with Bevy's explanation.
    pub fn ambiguities(&mut self) -> Vec<Ambiguity> {
        self.finish();
        let mut fresh = HashSet::new();
        let policy = self.policy.clone();
        match ambiguity::find(self.app.world_mut(), &policy, &mut fresh, true) {
            Ok(found) => found,
            Err(e) => panic!("{e}"),
        }
    }

    /// Build the checked schedules and fail on any pair the allow-list does not cover. Runs
    /// before every frame anyway; call it to check an app you never step.
    ///
    /// # Panics
    ///
    /// On the first failing schedule, listing every pair.
    pub fn check_ambiguities(&mut self) -> &mut Self {
        self.finish();
        let policy = self.policy.clone();
        let found = match ambiguity::find(self.app.world_mut(), &policy, &mut self.checked, false) {
            Ok(found) => found,
            Err(e) => panic!("{e}"),
        };
        if !found.is_empty() {
            panic!("{}", ambiguity::failure_message(&found));
        }
        self
    }

    fn prime_time(world: &mut World) {
        // Bevy's first frame has a zero delta (the clock only learns its start). Teaching it the
        // start now makes frame 1 advance by exactly one frame, like every later frame.
        if let Some(mut real) = world.get_resource_mut::<Time<Real>>() {
            if real.last_update().is_none() {
                let start = real.startup();
                real.update_with_instant(start);
            }
        }
    }

    /// One frame without the real pause.
    pub(crate) fn step_once(&mut self) {
        self.check_ambiguities();
        Self::prime_time(self.app.world_mut());
        self.app.update();
        self.frames += 1;
        let frame = self.frames;
        let world = self.app.world();
        for watcher in self.watchers.values_mut() {
            watcher.drain(world, frame);
        }
    }

    pub(crate) fn pause(&self) {
        if !self.real_pause.is_zero() {
            std::thread::sleep(self.real_pause);
        }
    }

    /// Run one frame.
    ///
    /// # Panics
    ///
    /// On an ambiguity the allow-list does not cover, or a schedule that cannot be built.
    pub fn step(&mut self) -> &mut Self {
        self.step_once();
        self.pause();
        self
    }

    /// Run `n` frames.
    pub fn step_n(&mut self, n: u32) -> &mut Self {
        for _ in 0..n {
            self.step();
        }
        self
    }

    /// Run frames until at least `secs` of simulated time passed; returns the frames run
    /// (`ceil(secs / frame)`).
    ///
    /// # Panics
    ///
    /// If `secs` is negative, NaN or infinite.
    pub fn step_secs(&mut self, secs: f32) -> u32 {
        assert!(secs.is_finite() && secs >= 0.0, "step_secs({secs}): the duration must be finite and not negative");
        let frame = self.frame.as_secs_f64();
        if frame <= 0.0 {
            // A zero frame never advances time: run one frame and stop.
            self.step();
            return 1;
        }
        // The small epsilon keeps 0.5 s at 1/64 s from becoming 33 frames through rounding.
        let n = ((f64::from(secs) / frame) - 1e-9).ceil().max(0.0) as u32;
        self.step_n(n);
        n
    }

    /// Step until `done(world)` is true, checking it before every frame; returns the frames run.
    ///
    /// # Panics
    ///
    /// If `done` is still false after `max_frames` frames.
    pub fn run_until(&mut self, mut done: impl FnMut(&mut World) -> bool, max_frames: u32) -> u32 {
        for frame in 0..max_frames {
            if done(self.app.world_mut()) {
                return frame;
            }
            self.step();
        }
        assert!(done(self.app.world_mut()), "run_until: the condition was still false after {max_frames} frames ({:?} of simulated time)", self.elapsed());
        max_frames
    }

    /// Step several apps in lock-step for one frame (in slice order), then sleep once for the
    /// longest real pause among them.
    pub fn step_together(apps: &mut [&mut TestApp]) {
        for app in apps.iter_mut() {
            app.step_once();
        }
        let pause = apps.iter().map(|a| a.real_pause).max().unwrap_or_default();
        if !pause.is_zero() {
            std::thread::sleep(pause);
        }
    }

    /// Step several apps in lock-step until `done(apps)` is true (checked before every frame);
    /// returns the frames run.
    ///
    /// # Panics
    ///
    /// If `done` is still false after `max_frames` frames.
    pub fn run_together_until(apps: &mut [&mut TestApp], mut done: impl FnMut(&mut [&mut TestApp]) -> bool, max_frames: u32) -> u32 {
        for frame in 0..max_frames {
            if done(apps) {
                return frame;
            }
            Self::step_together(apps);
        }
        assert!(done(apps), "run_together_until: the condition was still false after {max_frames} frames");
        max_frames
    }
}
