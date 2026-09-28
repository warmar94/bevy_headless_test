//! The strict ambiguity check: every unordered pair of systems that conflicts on the same data
//! is a failure, unless an [`AllowRule`] covers it.

use std::collections::{HashMap, HashSet};
use std::fmt;

use bevy_ecs::schedule::{InternedScheduleLabel, Schedule};
use bevy_ecs::world::World;

/// One entry of the ambiguity allow-list.
///
/// A system NAME PATTERN matches a system whose full path (`my_game::combat::apply_damage`)
///
/// - equals the pattern,
/// - ends with `::pattern` (so `apply_damage` or `combat::apply_damage` match), or
/// - starts with `pattern::` (so `my_game::combat` matches every system in that module).
///
/// Generic arguments are ignored when matching (`update<Foo>` matches `update`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllowRule {
    /// A pair of systems, in either order, optionally only in one schedule.
    Pair {
        /// `None` = every checked schedule.
        schedule: Option<InternedScheduleLabel>,
        /// A name pattern for one system of the pair.
        a: String,
        /// A name pattern for the other system of the pair.
        b: String,
    },
    /// Every pair that involves this system, optionally only in one schedule.
    System {
        /// `None` = every checked schedule.
        schedule: Option<InternedScheduleLabel>,
        /// A name pattern for the system.
        name: String,
    },
    /// Every pair whose BOTH systems belong to one of these crates or modules (for example
    /// `["bevy_replicon"]`, or `["bevy_replicon", "bevy_time"]`): libraries' own ambiguities,
    /// which your game cannot order. Pairs between your systems and theirs are still checked.
    ///
    /// A system belongs to `prefix` when its path matches it (see above) or, for a system built
    /// from parameter builders (such systems are named after their parameter list, `(ResMut<..>,
    /// ..)`), when that list names a type of `prefix`.
    Among {
        /// Crate or module paths.
        prefixes: Vec<String>,
    },
    /// Conflicts on data whose type name matches this pattern (same matching rules, applied to
    /// component / resource type names such as `my_game::Score` or `Messages<my_game::Hit>`).
    /// A pair is allowed when every piece of data it conflicts on is allowed; a pair that
    /// conflicts on the whole `World` (an exclusive system) is never allowed by this rule.
    Data {
        /// A type name pattern.
        name: String,
    },
}

/// Which schedules are checked, and what is allowed.
#[derive(Clone, Debug)]
pub struct AmbiguityPolicy {
    /// The schedules checked (missing ones are skipped).
    pub schedules: Vec<InternedScheduleLabel>,
    /// The allow-list.
    pub rules: Vec<AllowRule>,
    /// `false` disables the check entirely.
    pub enabled: bool,
}

/// One unordered pair of conflicting systems that no rule allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ambiguity {
    /// The schedule (`Update`, `FixedUpdate`, ...).
    pub schedule: String,
    /// The first system's full path.
    pub system_a: String,
    /// The second system's full path.
    pub system_b: String,
    /// The type names of the data both touch, at least one of them mutably. Empty means the
    /// whole `World` (one of them is exclusive, or both take `EntityMut`-like access).
    pub conflicts: Vec<String>,
}

impl fmt::Display for Ambiguity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let on = if self.conflicts.is_empty() { "the whole World".to_string() } else { self.conflicts.join(", ") };
        write!(f, "{}: `{}` and `{}` access {} (at least one of them mutably) with no order between them", self.schedule, self.system_a, self.system_b, on)
    }
}

/// Does a full type path match a pattern (see [`AllowRule`])?
pub(crate) fn name_matches(full: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    let candidates = [full, full.split('<').next().unwrap_or(full)];
    candidates.iter().any(|name| {
        *name == pattern
            || (name.len() > pattern.len() + 2 && name.ends_with(pattern) && name[..name.len() - pattern.len()].ends_with("::"))
            || (name.len() > pattern.len() + 2 && name.starts_with(pattern) && name[pattern.len()..].starts_with("::"))
    })
}

/// Does a system belong to a crate / module (see [`AllowRule::Among`])?
pub(crate) fn belongs_to(system: &str, prefix: &str) -> bool {
    if name_matches(system, prefix) {
        return true;
    }
    let prefix = prefix.trim();
    if !system.starts_with('(') || prefix.is_empty() {
        return false;
    }
    let needle = format!("{prefix}::");
    system.match_indices(&needle).any(|(i, _)| system[..i].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == ':')))
}

impl AmbiguityPolicy {
    fn rule_allows_pair(rule: &AllowRule, schedule: InternedScheduleLabel, a: &str, b: &str) -> bool {
        let in_schedule = |s: &Option<InternedScheduleLabel>| s.is_none_or(|s| s == schedule);
        match rule {
            AllowRule::Pair { schedule: s, a: pa, b: pb } => {
                in_schedule(s) && ((name_matches(a, pa) && name_matches(b, pb)) || (name_matches(a, pb) && name_matches(b, pa)))
            }
            AllowRule::System { schedule: s, name } => in_schedule(s) && (name_matches(a, name) || name_matches(b, name)),
            AllowRule::Among { prefixes } => {
                let belongs = |name: &str| prefixes.iter().any(|p| belongs_to(name, p));
                belongs(a) && belongs(b)
            }
            AllowRule::Data { .. } => false,
        }
    }

    /// Whether the rules allow this pair.
    pub(crate) fn allows(&self, schedule: InternedScheduleLabel, a: &str, b: &str, conflicts: &[String]) -> bool {
        if self.rules.iter().any(|rule| Self::rule_allows_pair(rule, schedule, a, b)) {
            return true;
        }
        let data_rules: Vec<&str> = self
            .rules
            .iter()
            .filter_map(|rule| match rule {
                AllowRule::Data { name } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        !conflicts.is_empty() && !data_rules.is_empty() && conflicts.iter().all(|c| data_rules.iter().any(|p| name_matches(c, p)))
    }
}

/// Build (if needed) every checked schedule and list the pairs no rule allows.
///
/// A schedule is inspected when it was (re)built now, when `force` is set, or when it is not in
/// `checked` yet (built somewhere else); inspected schedules are added to `checked`.
///
/// `Err` carries a schedule that failed to build (a cycle, a conflicting set configuration, ...)
/// with Bevy's own explanation.
pub(crate) fn find(world: &mut World, policy: &AmbiguityPolicy, checked: &mut HashSet<InternedScheduleLabel>, force: bool) -> Result<Vec<Ambiguity>, String> {
    let mut found = Vec::new();
    if !policy.enabled {
        return Ok(found);
    }
    for &label in &policy.schedules {
        let seen = checked.contains(&label);
        let result = world.try_schedule_scope(label, |world, schedule| {
            let built = match schedule.initialize(world) {
                Ok(built) => built.is_some(),
                Err(e) => return Err(format!("the {label:?} schedule failed to build: {}", e.to_string(schedule.graph(), world))),
            };
            if built || force || !seen {
                Ok(Some(check_schedule(world, schedule, label, policy)))
            } else {
                Ok(None)
            }
        });
        match result {
            Ok(Ok(Some(mut pairs))) => {
                checked.insert(label);
                found.append(&mut pairs);
            }
            Ok(Ok(None)) => {}
            Ok(Err(e)) => return Err(e),
            // The app has no such schedule: nothing to check.
            Err(_) => {}
        }
    }
    Ok(found)
}

fn check_schedule(world: &World, schedule: &Schedule, label: InternedScheduleLabel, policy: &AmbiguityPolicy) -> Vec<Ambiguity> {
    let conflicting = schedule.graph().conflicting_systems();
    if conflicting.is_empty() {
        return Vec::new();
    }
    let names: HashMap<_, String> = match schedule.systems() {
        Ok(systems) => systems.map(|(key, system)| (key, system.name().to_string())).collect(),
        Err(_) => HashMap::new(),
    };
    let components = world.components();
    let mut found = Vec::new();
    for (a, b, on) in conflicting.iter() {
        let name_a = names.get(a).cloned().unwrap_or_else(|| format!("{a:?}"));
        let name_b = names.get(b).cloned().unwrap_or_else(|| format!("{b:?}"));
        let conflicts: Vec<String> = on.iter().map(|id| components.get_name(*id).map(|n| n.to_string()).unwrap_or_else(|| format!("{id:?}"))).collect();
        if !policy.allows(label, &name_a, &name_b, &conflicts) {
            found.push(Ambiguity { schedule: format!("{label:?}"), system_a: name_a, system_b: name_b, conflicts });
        }
    }
    found
}

/// Format a failure listing every pair, with the fixes.
pub(crate) fn failure_message(found: &[Ambiguity]) -> String {
    let mut msg = format!("{} unordered system pair(s) conflict on the same data:\n", found.len());
    for pair in found {
        msg.push_str("  - ");
        msg.push_str(&pair.to_string());
        msg.push('\n');
    }
    msg.push_str(
        "Order them (`.before()` / `.after()` / `.chain()` / a `SystemSet`), or, for a pair you cannot order (a \
         third-party crate's), allow it on the TestApp builder (`allow_pair`, `allow_system`, `allow_internal`, \
         `allow_among`, `allow_data`).",
    );
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_patterns() {
        assert!(name_matches("game::combat::apply", "apply"));
        assert!(name_matches("game::combat::apply", "combat::apply"));
        assert!(name_matches("game::combat::apply", "game::combat::apply"));
        assert!(name_matches("game::combat::apply", "game::combat"));
        assert!(name_matches("game::combat::apply", "game"));
        assert!(name_matches("bevy_ecs::message::update<game::Hit>", "update"));
        assert!(!name_matches("game::combat::apply_damage", "apply"));
        assert!(!name_matches("game::combat::reapply", "apply"));
        assert!(!name_matches("gamex::combat::apply", "game"));
        assert!(!name_matches("game::combat::apply", ""));
    }

    #[test]
    fn builder_systems_belong_to_the_crates_they_name() {
        let builder = "(bevy_ecs::world::FilteredResourcesMut<'_, '_>, bevy_ecs::ResMut<'_, lib_a::net::Messages>)";
        assert!(belongs_to(builder, "lib_a"));
        assert!(belongs_to(builder, "lib_a::net"));
        assert!(!belongs_to(builder, "lib"));
        assert!(!belongs_to(builder, "a::net"));
        assert!(belongs_to("lib_a::net::send", "lib_a"));
        assert!(!belongs_to("game::uses_lib_a::net", "lib_a"));
    }
}
