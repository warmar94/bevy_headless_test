//! UI text read-back: collect every string the app draws and assert on it.

use std::collections::HashSet;
use std::fmt;

use bevy_ecs::entity::Entity;
use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::name::Name;
use bevy_ecs::world::World;

use crate::app::TestApp;

/// World-space 2D text (`bevy_sprite`).
pub use bevy_sprite::Text2d;
/// A child span of a `Text` / `Text2d` (`bevy_text`).
pub use bevy_text::TextSpan;
/// The UI node text component (`bevy_ui`).
pub use bevy_ui::widget::Text;

/// Which component a string came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextKind {
    /// `bevy_ui::widget::Text`.
    Text,
    /// `bevy_text::TextSpan`.
    TextSpan,
    /// `bevy_sprite::Text2d`.
    Text2d,
}

/// One string found in the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEntry {
    /// The entity holding it.
    pub entity: Entity,
    /// The entity's `Name`, if it has one.
    pub name: Option<String>,
    /// The component it came from.
    pub kind: TextKind,
    /// The string.
    pub text: String,
}

impl TextEntry {
    fn describe(&self) -> String {
        match &self.name {
            Some(name) => format!("{:?} on {} ({name:?})", self.kind, self.entity),
            None => format!("{:?} on {}", self.kind, self.entity),
        }
    }
}

impl fmt::Display for TextEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.describe(), self.text)
    }
}

/// Every `Text`, `TextSpan` and `Text2d` string in a world, or under one root entity, in
/// hierarchy order (a parent before its children, children in order).
///
/// Collect it after the frame that builds your UI, then assert: [`assert_all_ascii`],
/// [`assert_charset`], [`assert_chars`], [`assert_at_least`], [`assert_contains`]. Every failure
/// names the entity (and its `Name`), the component, the offending character and the string.
///
/// A check over zero strings passes, which is how an empty page reads as a clean pass: pair a
/// charset assertion with [`assert_at_least`].
///
/// [`assert_all_ascii`]: Self::assert_all_ascii
/// [`assert_charset`]: Self::assert_charset
/// [`assert_chars`]: Self::assert_chars
/// [`assert_at_least`]: Self::assert_at_least
/// [`assert_contains`]: Self::assert_contains
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiText {
    entries: Vec<TextEntry>,
}

fn entry_of(world: &World, entity: Entity) -> Vec<TextEntry> {
    let Ok(e) = world.get_entity(entity) else {
        return Vec::new();
    };
    let name = e.get::<Name>().map(|n| n.as_str().to_string());
    let mut found = Vec::new();
    if let Some(t) = e.get::<Text>() {
        found.push(TextEntry { entity, name: name.clone(), kind: TextKind::Text, text: t.0.clone() });
    }
    if let Some(t) = e.get::<TextSpan>() {
        found.push(TextEntry { entity, name: name.clone(), kind: TextKind::TextSpan, text: t.0.clone() });
    }
    if let Some(t) = e.get::<Text2d>() {
        found.push(TextEntry { entity, name, kind: TextKind::Text2d, text: t.0.clone() });
    }
    found
}

/// `root` and its descendants, depth first: a parent before its children, children in their
/// `Children` order (reading order for a UI tree).
fn walk(world: &World, root: Entity, seen: &mut HashSet<Entity>, out: &mut Vec<Entity>) {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if !seen.insert(entity) {
            continue;
        }
        out.push(entity);
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter().rev());
        }
    }
}

impl UiText {
    /// Every string in the world: each hierarchy that holds text, depth first (see
    /// [`collect_under`](Self::collect_under)); the hierarchies in `Entity` order.
    pub fn collect(world: &mut World) -> Self {
        let mut holders: Vec<Entity> = Vec::new();
        let mut q = world.query::<(Entity, Option<&Text>, Option<&TextSpan>, Option<&Text2d>)>();
        for (entity, a, b, c) in q.iter(world) {
            if a.is_some() || b.is_some() || c.is_some() {
                holders.push(entity);
            }
        }
        let world: &World = world;
        let mut roots: Vec<Entity> = holders
            .iter()
            .map(|&e| {
                let mut top = e;
                let mut hops = 0usize;
                while let Some(parent) = world.get::<ChildOf>(top).map(|c| c.parent()) {
                    // A hierarchy cannot loop, but a test kit never spins forever.
                    hops += 1;
                    if hops > 10_000 || world.get_entity(parent).is_err() {
                        break;
                    }
                    top = parent;
                }
                top
            })
            .collect();
        roots.sort();
        roots.dedup();
        let mut seen = HashSet::new();
        let mut order = Vec::new();
        for root in roots {
            walk(world, root, &mut seen, &mut order);
        }
        let entries = order.into_iter().flat_map(|e| entry_of(world, e)).collect();
        Self { entries }
    }

    /// Every string on `root` and its descendants, depth first: a parent before its children,
    /// children in their `Children` order. An entity that does not exist gives an empty result.
    pub fn collect_under(world: &World, root: Entity) -> Self {
        let mut order = Vec::new();
        walk(world, root, &mut HashSet::new(), &mut order);
        let entries = order.into_iter().flat_map(|e| entry_of(world, e)).collect();
        Self { entries }
    }

    /// The strings found, with where they came from.
    pub fn entries(&self) -> &[TextEntry] {
        &self.entries
    }

    /// Just the strings.
    pub fn strings(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.text.as_str()).collect()
    }

    /// How many strings were found.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing was found.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether any string contains `needle`.
    pub fn contains(&self, needle: &str) -> bool {
        self.entries.iter().any(|e| e.text.contains(needle))
    }

    /// The first character that `allowed` rejects: the entry, the character and its byte offset.
    /// Line breaks and tabs (`\n`, `\r`, `\t`) are never rejected: they are layout, not glyphs.
    pub fn find_rejected(&self, allowed: impl Fn(char) -> bool) -> Option<(&TextEntry, char, usize)> {
        self.entries
            .iter()
            .find_map(|entry| entry.text.char_indices().find(|&(_, c)| !matches!(c, '\n' | '\r' | '\t') && !allowed(c)).map(|(i, c)| (entry, c, i)))
    }

    /// Assert every character passes `allowed`; `what` names the rule in the failure message.
    ///
    /// # Panics
    ///
    /// On the first rejected character, naming the entity, the character and the string.
    pub fn assert_chars(&self, what: &str, allowed: impl Fn(char) -> bool) -> &Self {
        if let Some((entry, c, at)) = self.find_rejected(allowed) {
            panic!("{c:?} (U+{:04X}) at byte {at} is not {what}: {}; checked {} string(s)", c as u32, entry, self.entries.len());
        }
        self
    }

    /// Assert every string is ASCII (a font with only the 95 printable ASCII glyphs draws it).
    pub fn assert_all_ascii(&self) -> &Self {
        self.assert_chars("ASCII", |c| c.is_ascii() && !c.is_ascii_control())
    }

    /// Assert every character is one of `allowed` (for example, the glyphs your font covers).
    pub fn assert_charset(&self, allowed: &str) -> &Self {
        let set: HashSet<char> = allowed.chars().collect();
        self.assert_chars("in the allowed character set", |c| set.contains(&c))
    }

    /// Assert at least `n` strings were found (so an empty page cannot pass a charset check).
    pub fn assert_at_least(&self, n: usize) -> &Self {
        assert!(self.entries.len() >= n, "expected at least {n} UI string(s), found {}: {:?}", self.entries.len(), self.strings());
        self
    }

    /// Assert some string contains `needle`.
    pub fn assert_contains(&self, needle: &str) -> &Self {
        assert!(self.contains(needle), "no UI string contains {needle:?}; found {}: {:?}", self.entries.len(), self.strings());
        self
    }
}

impl TestApp {
    /// Every UI string in the world (see [`UiText`]).
    pub fn ui_text(&mut self) -> UiText {
        UiText::collect(self.app.world_mut())
    }

    /// Every UI string on `root` and its descendants.
    pub fn ui_text_under(&self, root: Entity) -> UiText {
        UiText::collect_under(self.app.world(), root)
    }
}
