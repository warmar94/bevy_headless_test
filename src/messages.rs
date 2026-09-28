//! Message helpers: write a message, step, and read what was written, per frame or since a mark.

use std::any::{type_name, Any, TypeId};
use std::fmt::Debug;

use bevy_ecs::message::{Message, MessageCursor, Messages};
use bevy_ecs::world::World;

use crate::app::TestApp;

/// A point in a test app's frame count, from [`TestApp::mark`]. "Since a mark" means every frame
/// stepped after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mark(pub u64);

/// A type-erased message watcher.
pub(crate) trait AnyWatcher: Send + Sync {
    fn drain(&mut self, world: &World, frame: u64);
    fn as_any(&self) -> &dyn Any;
}

/// Records every `M` written, with the frame it was seen in.
struct Watcher<M: Message + Clone> {
    cursor: MessageCursor<M>,
    seen: Vec<(u64, M)>,
    /// Messages the buffers dropped before this watcher could read them.
    missed: usize,
    /// Whether `Messages<M>` existed at the last drain.
    registered: bool,
}

impl<M: Message + Clone> AnyWatcher for Watcher<M> {
    fn drain(&mut self, world: &World, frame: u64) {
        let Some(messages) = world.get_resource::<Messages<M>>() else {
            self.registered = false;
            return;
        };
        self.registered = true;
        self.missed += self.cursor.missed_messages(messages);
        for message in self.cursor.read(messages) {
            self.seen.push((frame, message.clone()));
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl TestApp {
    fn watcher<M: Message + Clone>(&self) -> &Watcher<M> {
        let watcher = self.watchers.get(&TypeId::of::<M>()).and_then(|w| w.as_any().downcast_ref::<Watcher<M>>());
        match watcher {
            Some(w) => w,
            None => panic!(
                "messages of type `{}` are not watched: call `watch::<{}>()` before the frames you want to observe",
                type_name::<M>(),
                short(type_name::<M>())
            ),
        }
    }

    /// Start recording every `M` written from now on (plus any still in its buffers). Every
    /// `step` then reads them, so none is missed. Watching twice is harmless.
    pub fn watch<M: Message + Clone>(&mut self) -> &mut Self {
        // A fresh cursor starts at what the buffers still hold.
        let registered = self.app.world().contains_resource::<Messages<M>>();
        let watcher = Watcher::<M> { cursor: MessageCursor::default(), seen: Vec::new(), missed: 0, registered };
        self.watchers.entry(TypeId::of::<M>()).or_insert_with(|| Box::new(watcher));
        self
    }

    /// Write a message into the world (it is seen by systems in the next frame).
    ///
    /// # Panics
    ///
    /// If `M` is not registered (`app.add_message::<M>()`).
    pub fn send<M: Message>(&mut self, message: M) -> &mut Self {
        if self.app.world_mut().write_message(message).is_none() {
            panic!(
                "cannot send `{}`: the message type is not registered (call `app.add_message::<{}>()` or add the plugin that does)",
                type_name::<M>(),
                short(type_name::<M>())
            );
        }
        self
    }

    /// Write a message and step one frame.
    pub fn send_and_step<M: Message>(&mut self, message: M) -> &mut Self {
        self.send(message);
        self.step()
    }

    /// The current frame count, to read messages "since" later.
    pub fn mark(&self) -> Mark {
        Mark(self.frames)
    }

    /// Every watched `M` seen in the LAST frame stepped.
    pub fn messages<M: Message + Clone>(&self) -> Vec<M> {
        let frame = self.frames;
        self.watcher::<M>().seen.iter().filter(|(f, _)| *f == frame).map(|(_, m)| m.clone()).collect()
    }

    /// Every watched `M` seen in the frames stepped after `mark`.
    pub fn messages_since<M: Message + Clone>(&self, mark: Mark) -> Vec<M> {
        self.watcher::<M>().seen.iter().filter(|(f, _)| *f > mark.0).map(|(_, m)| m.clone()).collect()
    }

    /// Every watched `M` seen since [`watch`](Self::watch).
    pub fn all_messages<M: Message + Clone>(&self) -> Vec<M> {
        self.watcher::<M>().seen.iter().map(|(_, m)| m.clone()).collect()
    }

    fn assert_count_in<M: Message + Clone + Debug>(&self, n: usize, found: Vec<M>, window: &str) -> Vec<M> {
        let watcher = self.watcher::<M>();
        let hint = if !watcher.registered {
            " (no `Messages` resource for it: is the message registered?)".to_string()
        } else if watcher.missed > 0 {
            format!(" ({} were dropped before they could be read: step with `TestApp::step`, not `app.update()`)", watcher.missed)
        } else {
            String::new()
        };
        assert!(found.len() == n, "expected {n} `{}` {window}, got {}{hint}: {found:?}", short(type_name::<M>()), found.len());
        found
    }

    /// Assert exactly `n` watched `M` in the last frame; returns them.
    pub fn assert_count<M: Message + Clone + Debug>(&self, n: usize) -> Vec<M> {
        self.assert_count_in(n, self.messages::<M>(), &format!("in frame {}", self.frames))
    }

    /// Assert exactly one watched `M` in the last frame; returns it.
    pub fn assert_exactly_one<M: Message + Clone + Debug>(&self) -> M {
        let mut found = self.assert_count::<M>(1);
        found.remove(0)
    }

    /// Assert no watched `M` in the last frame.
    pub fn assert_none<M: Message + Clone + Debug>(&self) {
        self.assert_count::<M>(0);
    }

    /// Assert exactly one watched `M` since `mark`; returns it.
    pub fn assert_exactly_one_since<M: Message + Clone + Debug>(&self, mark: Mark) -> M {
        let mut found = self.assert_count_in(1, self.messages_since::<M>(mark), &format!("since frame {}", mark.0));
        found.remove(0)
    }

    /// Assert no watched `M` since `mark`.
    pub fn assert_none_since<M: Message + Clone + Debug>(&self, mark: Mark) {
        self.assert_count_in(0, self.messages_since::<M>(mark), &format!("since frame {}", mark.0));
    }
}

/// `a::b::C<d::E>` -> `C<d::E>` (for readable messages).
pub(crate) fn short(name: &str) -> &str {
    let base = name.split('<').next().unwrap_or(name);
    match base.rfind("::") {
        Some(i) => &name[i + 2..],
        None => name,
    }
}
