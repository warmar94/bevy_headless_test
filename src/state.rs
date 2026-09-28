//! State helpers: set, read and assert Bevy `States`.

use std::any::type_name;
use std::fmt::Debug;

use bevy_state::state::{FreelyMutableState, NextState, State, States};

use crate::app::TestApp;
use crate::messages::short;

impl TestApp {
    /// The current value of state `S`, or `None` when `S` is not initialised.
    pub fn state<S: States>(&self) -> Option<S> {
        self.app.world().get_resource::<State<S>>().map(|s| s.get().clone())
    }

    /// Queue a transition to `next` (applied in the next frame's `StateTransition`).
    ///
    /// # Panics
    ///
    /// If `S` is not initialised (`app.init_state::<S>()` / `insert_state`).
    pub fn set_state<S: FreelyMutableState>(&mut self, next: S) -> &mut Self {
        match self.app.world_mut().get_resource_mut::<NextState<S>>() {
            Some(mut queued) => queued.set(next),
            None => panic!("cannot set `{}`: the state is not initialised (call `app.init_state::<{0}>()` or `insert_state`)", short(type_name::<S>())),
        }
        self
    }

    /// Queue a transition to `next` and step one frame, so it is applied.
    pub fn set_state_and_step<S: FreelyMutableState>(&mut self, next: S) -> &mut Self {
        self.set_state(next);
        self.step()
    }

    /// Assert the current value of state `S`.
    ///
    /// # Panics
    ///
    /// If the state differs or is not initialised.
    pub fn assert_state<S: States + Debug>(&self, expected: S) {
        match self.state::<S>() {
            Some(actual) => assert!(actual == expected, "state `{}` is {actual:?}, expected {expected:?} (frame {})", short(type_name::<S>()), self.frames),
            None => panic!("state `{}` is not initialised, expected {expected:?}", short(type_name::<S>())),
        }
    }

    /// Step until state `S` equals `expected`; returns the frames run.
    ///
    /// # Panics
    ///
    /// If it does not within `max_frames`.
    pub fn run_until_state<S: States + Debug>(&mut self, expected: S, max_frames: u32) -> u32 {
        for frame in 0..max_frames {
            if self.state::<S>().as_ref() == Some(&expected) {
                return frame;
            }
            self.step();
        }
        self.assert_state(expected);
        max_frames
    }
}
