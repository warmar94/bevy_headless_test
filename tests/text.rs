//! UI text read-back (feature `ui`).

use std::panic::{catch_unwind, AssertUnwindSafe};

use bevy::prelude::*;
use bevy_headless_test::prelude::*;
use bevy_headless_test::text::{Text, Text2d, TextSpan};

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

/// A small "menu": a root with a title, a label with a span, and one world-space label outside.
fn menu(app: &mut TestApp, label: &str) -> Entity {
    let world = app.world_mut();
    let root = world.spawn(Name::new("menu")).id();
    world.spawn((Text::new("Settings"), ChildOf(root)));
    let row = world.spawn((Name::new("volume row"), Text::new("Volume: "), ChildOf(root))).id();
    world.spawn((TextSpan::new(label), ChildOf(row)));
    world.spawn(Text2d::new("outside the menu"));
    root
}

#[test]
fn every_string_is_collected_in_hierarchy_order() {
    let mut app = TestApp::new();
    let root = menu(&mut app, "80%");
    app.step();
    let under = app.ui_text_under(root);
    assert_eq!(under.strings(), vec!["Settings", "Volume: ", "80%"]);
    let kinds: Vec<_> = under.entries().iter().map(|e| e.kind).collect();
    assert_eq!(kinds, vec![TextKind::Text, TextKind::Text, TextKind::TextSpan]);
    let all = app.ui_text();
    assert_eq!(all.len(), 4);
    let pos = |s: &str| all.strings().iter().position(|x| *x == s).expect(s);
    assert!(pos("Settings") < pos("Volume: ") && pos("Volume: ") < pos("80%"), "a hierarchy stays in reading order");
    assert_eq!(all.entries()[pos("outside the menu")].kind, TextKind::Text2d);
    all.assert_all_ascii().assert_at_least(4).assert_contains("Volume");
}

#[test]
fn collect_under_a_root_skips_what_is_outside() {
    let mut app = TestApp::new();
    let root = menu(&mut app, "80%");
    let text = app.ui_text_under(root);
    assert_eq!(text.len(), 3);
    assert!(!text.contains("outside"));
    assert!(app.ui_text_under(Entity::PLACEHOLDER).is_empty());
}

#[test]
fn a_non_ascii_character_names_the_entity_and_the_string() {
    let mut app = TestApp::new();
    let root = menu(&mut app, "80 \u{2013} 90");
    let text = app.ui_text_under(root);
    let (entry, c, at) = text.find_rejected(|c| c.is_ascii()).expect("the en dash");
    assert_eq!((c, at), ('\u{2013}', 3));
    assert_eq!(entry.kind, TextKind::TextSpan);
    let err = catch_unwind(AssertUnwindSafe(|| {
        text.assert_all_ascii();
    }))
    .expect_err("the en dash is not ASCII");
    let msg = panic_message(err);
    println!(">>> {msg}");
    assert!(msg.contains("U+2013"), "{msg}");
    assert!(msg.contains(&format!("{}", entry.entity)), "{msg}");
    assert!(msg.contains("80 \u{2013} 90"), "{msg}");
}

#[test]
fn the_entity_name_is_reported_when_it_has_one() {
    let mut app = TestApp::new();
    app.world_mut().spawn((Name::new("volume row"), Text::new("\u{d7}2")));
    let err = catch_unwind(AssertUnwindSafe(|| {
        app.ui_text().assert_all_ascii();
    }))
    .expect_err("the times sign is not ASCII");
    let msg = panic_message(err);
    assert!(msg.contains("\"volume row\""), "{msg}");
}

#[test]
fn a_charset_is_checked_character_by_character() {
    let mut app = TestApp::new();
    app.world_mut().spawn(Text::new("ABBA\nBAAB"));
    let text = app.ui_text();
    text.assert_charset("AB");
    let err = catch_unwind(AssertUnwindSafe(|| {
        text.assert_charset("A");
    }))
    .expect_err("B is not in the set");
    assert!(panic_message(err).contains("'B' (U+0042) at byte 1"));
}

#[test]
#[should_panic(expected = "expected at least 1 UI string(s), found 0")]
fn an_empty_page_is_caught_by_assert_at_least() {
    TestApp::new().ui_text().assert_all_ascii().assert_at_least(1);
}
