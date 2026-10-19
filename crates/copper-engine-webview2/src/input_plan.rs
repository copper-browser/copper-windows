//! `PageView::input` as CDP `Input.*` calls.
//!
//! CDP input enters Chromium's renderer through the same path as real input
//! (the response comes back once the renderer acknowledged the event), so
//! every event it makes is trusted — pointer, mouse, wheel, keys, the
//! editor's `beforeinput`/`input`, `submit` — and every action reports
//! [`Trust::Trusted`]. Coordinates are CSS pixels in the main frame's
//! viewport, the same space `InputEvent` uses.

use copper_engine::{InputEvent, Modifiers, MouseButton};
use serde_json::{Value, json};

use crate::keys::{self, modifier_bits};

/// Interpolated moves of a drag (so `dragover`/pointer listeners see a path).
const DRAG_STEPS: u32 = 12;

/// One CDP call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Call {
    pub method: &'static str,
    pub params: Value,
}

fn call(method: &'static str, params: Value) -> Call {
    Call { method, params }
}

/// What delivering an `InputEvent` takes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plan {
    /// Points that must have an element under them (else `NotFound`).
    pub hit_test: Vec<(f64, f64)>,
    pub calls: Vec<Call>,
    /// Where the pointer is afterwards (for later `Scroll`s).
    pub pointer: Option<(f64, f64)>,
}

fn button_name(b: MouseButton) -> &'static str {
    match b {
        MouseButton::Left => "left",
        MouseButton::Middle => "middle",
        MouseButton::Right => "right",
    }
}

/// The `buttons` bit of a held button (DOM `MouseEvent.buttons`).
fn button_bit(b: MouseButton) -> u32 {
    match b {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 4,
    }
}

fn mouse(kind: &str, x: f64, y: f64, button: &str, buttons: u32, count: u32, m: u32) -> Call {
    call(
        "Input.dispatchMouseEvent",
        json!({
            "type": kind,
            "x": x,
            "y": y,
            "button": button,
            "buttons": buttons,
            "clickCount": count,
            "modifiers": m,
            "pointerType": "mouse",
        }),
    )
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|f| f.is_finite())
}

/// Plan `event`. `pointer` is the last pointer position, `centre` the
/// viewport centre (where a scroll lands before the pointer ever moved).
pub(crate) fn plan(
    event: &InputEvent,
    pointer: Option<(f64, f64)>,
    centre: (f64, f64),
) -> Option<Plan> {
    let mut calls = Vec::new();
    let plan = match event {
        InputEvent::Move { x, y, modifiers } => {
            if !finite(&[*x, *y]) {
                return None;
            }
            calls.push(mouse(
                "mouseMoved",
                *x,
                *y,
                "none",
                0,
                0,
                modifier_bits(*modifiers),
            ));
            Plan {
                hit_test: vec![(*x, *y)],
                calls,
                pointer: Some((*x, *y)),
            }
        }
        InputEvent::Click {
            x,
            y,
            button,
            count,
            modifiers,
        } => {
            if !finite(&[*x, *y]) {
                return None;
            }
            let m = modifier_bits(*modifiers);
            let name = button_name(*button);
            calls.push(mouse("mouseMoved", *x, *y, "none", 0, 0, m));
            for n in 1..=(*count).max(1) {
                calls.push(mouse(
                    "mousePressed",
                    *x,
                    *y,
                    name,
                    button_bit(*button),
                    n,
                    m,
                ));
                calls.push(mouse("mouseReleased", *x, *y, name, 0, n, m));
            }
            Plan {
                hit_test: vec![(*x, *y)],
                calls,
                pointer: Some((*x, *y)),
            }
        }
        InputEvent::Drag {
            from_x,
            from_y,
            to_x,
            to_y,
            modifiers,
        } => {
            if !finite(&[*from_x, *from_y, *to_x, *to_y]) {
                return None;
            }
            let m = modifier_bits(*modifiers);
            calls.push(mouse("mouseMoved", *from_x, *from_y, "none", 0, 0, m));
            calls.push(mouse("mousePressed", *from_x, *from_y, "left", 1, 1, m));
            for i in 1..=DRAG_STEPS {
                let t = f64::from(i) / f64::from(DRAG_STEPS);
                let x = from_x + (to_x - from_x) * t;
                let y = from_y + (to_y - from_y) * t;
                calls.push(mouse("mouseMoved", x, y, "left", 1, 0, m));
            }
            calls.push(mouse("mouseReleased", *to_x, *to_y, "left", 0, 1, m));
            Plan {
                hit_test: vec![(*from_x, *from_y)],
                calls,
                pointer: Some((*to_x, *to_y)),
            }
        }
        InputEvent::Scroll { dx, dy } => {
            if !finite(&[*dx, *dy]) {
                return None;
            }
            let (x, y) = pointer.unwrap_or(centre);
            calls.push(call(
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mouseWheel",
                    "x": x,
                    "y": y,
                    "deltaX": dx,
                    "deltaY": dy,
                    "modifiers": 0,
                    "pointerType": "mouse",
                }),
            ));
            Plan {
                hit_test: Vec::new(),
                calls,
                pointer: Some((x, y)),
            }
        }
        InputEvent::Key {
            key,
            text,
            modifiers,
        } => {
            key_press(&mut calls, key, text.as_deref(), *modifiers);
            Plan {
                hit_test: Vec::new(),
                calls,
                pointer,
            }
        }
        InputEvent::Type(text) => {
            type_text(&mut calls, text);
            Plan {
                hit_test: Vec::new(),
                calls,
                pointer,
            }
        }
    };
    Some(plan)
}

/// One key: down (typing `text` unless a shortcut modifier is held), up.
fn key_press(calls: &mut Vec<Call>, key: &str, text: Option<&str>, m: Modifiers) {
    let shortcut = m.ctrl || m.alt || m.meta;
    let bits = modifier_bits(m);
    match keys::lookup(key) {
        Some(def) => {
            let text = text
                .map(str::to_owned)
                .or(def.text.clone())
                .filter(|_| !shortcut);
            let mut down = json!({
                "type": if text.is_some() { "keyDown" } else { "rawKeyDown" },
                "key": def.key,
                "code": def.code,
                "windowsVirtualKeyCode": def.vk,
                "nativeVirtualKeyCode": def.vk,
                "modifiers": bits,
                "location": def.location,
            });
            if let Some(t) = &text {
                down["text"] = json!(t);
                down["unmodifiedText"] = json!(t);
            }
            calls.push(call("Input.dispatchKeyEvent", down));
            calls.push(call(
                "Input.dispatchKeyEvent",
                json!({
                    "type": "keyUp",
                    "key": def.key,
                    "code": def.code,
                    "windowsVirtualKeyCode": def.vk,
                    "nativeVirtualKeyCode": def.vk,
                    "modifiers": bits,
                    "location": def.location,
                }),
            ));
        }
        None => match text.filter(|t| !t.is_empty() && !shortcut) {
            // A character no US key makes: the editor inserts it.
            Some(t) => calls.push(call("Input.insertText", json!({ "text": t }))),
            None => {
                for kind in ["rawKeyDown", "keyUp"] {
                    calls.push(call(
                        "Input.dispatchKeyEvent",
                        json!({ "type": kind, "key": key, "modifiers": bits }),
                    ));
                }
            }
        },
    }
}

/// Text one key at a time: `\n`/`\r` are Enter, `\t` is Tab, characters on
/// a US keyboard are key presses (Shift where it takes one), and runs of
/// anything else are inserted by the editor.
fn type_text(calls: &mut Vec<Call>, text: &str) {
    let mut other = String::new();
    let flush = |calls: &mut Vec<Call>, other: &mut String| {
        if !other.is_empty() {
            calls.push(call(
                "Input.insertText",
                json!({ "text": std::mem::take(other) }),
            ));
        }
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let key = match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                "Enter".to_owned()
            }
            '\n' => "Enter".to_owned(),
            '\t' => "Tab".to_owned(),
            c => c.to_string(),
        };
        match keys::lookup(&key) {
            Some(def) => {
                flush(calls, &mut other);
                let m = Modifiers {
                    shift: def.shifted,
                    ..Modifiers::NONE
                };
                key_press(calls, &key, None, m);
            }
            None => other.push(c),
        }
    }
    flush(calls, &mut other);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(p: &Plan) -> Vec<String> {
        p.calls
            .iter()
            .map(|c| match c.params.get("type") {
                Some(t) => t.as_str().unwrap().to_owned(),
                None => c.method.to_owned(),
            })
            .collect()
    }

    #[test]
    fn clicks() {
        let p = plan(&InputEvent::click(10.0, 20.0), None, (0.0, 0.0)).unwrap();
        assert_eq!(kinds(&p), ["mouseMoved", "mousePressed", "mouseReleased"]);
        assert_eq!(p.hit_test, vec![(10.0, 20.0)]);
        assert_eq!(p.calls[1].params["buttons"], 1);
        assert_eq!(p.calls[1].params["clickCount"], 1);
        let dbl = InputEvent::Click {
            x: 1.0,
            y: 2.0,
            button: MouseButton::Right,
            count: 2,
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        };
        let p = plan(&dbl, None, (0.0, 0.0)).unwrap();
        assert_eq!(p.calls.len(), 5);
        assert_eq!(p.calls[3].params["clickCount"], 2);
        assert_eq!(p.calls[3].params["button"], "right");
        assert_eq!(p.calls[3].params["buttons"], 2);
        assert_eq!(p.calls[3].params["modifiers"], 8);
        assert!(plan(&InputEvent::click(f64::NAN, 0.0), None, (0.0, 0.0)).is_none());
    }

    #[test]
    fn drags_and_scrolls() {
        let d = InputEvent::Drag {
            from_x: 0.0,
            from_y: 0.0,
            to_x: 120.0,
            to_y: 0.0,
            modifiers: Modifiers::NONE,
        };
        let p = plan(&d, None, (0.0, 0.0)).unwrap();
        assert_eq!(p.calls.len(), 2 + DRAG_STEPS as usize + 1);
        assert_eq!(p.calls[2].params["x"], 10.0);
        assert_eq!(p.calls[2].params["buttons"], 1);
        assert_eq!(p.pointer, Some((120.0, 0.0)));
        let s = plan(
            &InputEvent::Scroll { dx: 0.0, dy: 300.0 },
            None,
            (50.0, 60.0),
        )
        .unwrap();
        assert_eq!(s.calls[0].params["x"], 50.0);
        assert_eq!(s.calls[0].params["deltaY"], 300.0);
        let s = plan(
            &InputEvent::Scroll { dx: 0.0, dy: 1.0 },
            Some((5.0, 6.0)),
            (50.0, 60.0),
        )
        .unwrap();
        assert_eq!(s.calls[0].params["y"], 6.0);
    }

    #[test]
    fn keys_and_text() {
        let p = plan(&InputEvent::key("Enter"), None, (0.0, 0.0)).unwrap();
        assert_eq!(kinds(&p), ["keyDown", "keyUp"]);
        assert_eq!(p.calls[0].params["text"], "\r");
        let ctrl_a = InputEvent::Key {
            key: "a".into(),
            text: Some("a".into()),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        };
        let p = plan(&ctrl_a, None, (0.0, 0.0)).unwrap();
        assert_eq!(kinds(&p), ["rawKeyDown", "keyUp"]);
        assert!(p.calls[0].params.get("text").is_none());
        assert_eq!(p.calls[0].params["windowsVirtualKeyCode"], 0x41);
        let p = plan(&InputEvent::Type("Hi!\né€\tx".into()), None, (0.0, 0.0)).unwrap();
        assert_eq!(
            kinds(&p),
            [
                "keyDown",
                "keyUp", // H
                "keyDown",
                "keyUp", // i
                "keyDown",
                "keyUp", // !
                "keyDown",
                "keyUp",            // Enter
                "Input.insertText", // é€
                "rawKeyDown",
                "keyUp", // Tab
                "keyDown",
                "keyUp", // x
            ]
        );
        assert_eq!(p.calls[0].params["modifiers"], 8);
        assert_eq!(p.calls[0].params["text"], "H");
        assert_eq!(p.calls[8].params["text"], "é€");
        let p = plan(
            &InputEvent::Key {
                key: "ß".into(),
                text: Some("ß".into()),
                modifiers: Modifiers::NONE,
            },
            None,
            (0.0, 0.0),
        )
        .unwrap();
        assert_eq!(kinds(&p), ["Input.insertText"]);
        let p = plan(&InputEvent::Type("a\r\nb".into()), None, (0.0, 0.0)).unwrap();
        assert_eq!(p.calls.len(), 6);
    }
}
