use super::super::el::{
    Act, El, FieldId, Kind, PillLook, Tone, card, field, line, pill, row, text,
};
use super::Mount;
use copper_win_ui::node::Content;
use copper_win_ui::tree::Tree;
use copper_win_ui::{Node, NodeId};

fn tree() -> (Tree, NodeId) {
    let mut t = Tree::new();
    let root = t.add_root(Node::column());
    let list = t.add(root, Node::column());
    (t, list)
}

fn page(words: &str, with_field: bool, ready: bool) -> Vec<El> {
    let mut rows = vec![line(
        "first",
        "Email",
        Some(words),
        if with_field {
            vec![field(FieldId::Email, "you@example.com", Some(220.0), false)]
        } else {
            vec![]
        },
    )];
    rows.push(row(
        "submit",
        vec![
            text("note", words, 11.5, Tone::Muted),
            pill("go", "Sign in", Act::AccountSubmit, PillLook::Filled, ready),
        ],
    ));
    vec![card("card", rows)]
}

fn text_of(t: &Tree, id: NodeId) -> String {
    match &t.get(id).unwrap().content {
        Content::Text(x) => x.text.clone(),
        _ => panic!("not text"),
    }
}

#[test]
fn a_refresh_keeps_the_field_and_patches_the_words() {
    let (mut t, list) = tree();
    let mut m = Mount::default();
    m.sync(&mut t, list, &page("one", true, false));
    let f = m.node_of(FieldId::Email).unwrap();
    t.focus(Some(f), true);
    let go = m
        .acts
        .iter()
        .find(|(_, a)| **a == Act::AccountSubmit)
        .map(|(n, _)| *n)
        .unwrap();
    assert!(t.get(go).unwrap().flags.disabled);
    m.sync(&mut t, list, &page("two", true, true));
    assert_eq!(m.node_of(FieldId::Email), Some(f));
    assert_eq!(t.focused(), Some(f));
    assert_eq!(m.act(go), Some(&Act::AccountSubmit));
    assert!(!t.get(go).unwrap().flags.disabled);
    assert_eq!(m.field_of(f), Some(FieldId::Email));
    // The detail line was patched, not rebuilt.
    let card = t.children(list)[0];
    let first = t.children(card)[0];
    let words = t.children(first)[0];
    assert_eq!(text_of(&t, t.children(words)[1]), "two");
}

#[test]
fn what_went_away_is_removed_and_order_follows_the_page() {
    let (mut t, list) = tree();
    let mut m = Mount::default();
    m.sync(&mut t, list, &page("one", true, true));
    let f = m.node_of(FieldId::Email).unwrap();
    m.sync(&mut t, list, &page("one", false, true));
    assert!(!t.contains(f));
    assert_eq!(m.node_of(FieldId::Email), None);
    // Reordered siblings keep their nodes.
    let a = || El::new("a", Kind::Caption("A".into()));
    let b = || El::new("b", Kind::Caption("B".into()));
    m.sync(&mut t, list, &[a(), b()]);
    let (na, nb) = (t.children(list)[0], t.children(list)[1]);
    m.sync(&mut t, list, &[b(), a()]);
    assert_eq!(t.children(list), &[nb, na]);
}

#[test]
fn a_new_list_starts_afresh() {
    let (mut t, list) = tree();
    let mut m = Mount::default();
    m.sync(&mut t, list, &page("one", true, true));
    let root = t.root().unwrap();
    let other = t.add(root, Node::column());
    m.sync(&mut t, other, &page("one", true, true));
    assert_eq!(t.children(other).len(), 1);
    // Something else emptied the old list: nothing stale is kept.
    t.clear_children(other);
    m.sync(&mut t, other, &page("one", true, true));
    assert_eq!(t.children(other).len(), 1);
    assert!(m.node_of(FieldId::Email).is_some_and(|f| t.contains(f)));
}

#[test]
fn buttons_say_their_words_once() {
    let (mut t, list) = tree();
    let mut m = Mount::default();
    m.sync(
        &mut t,
        list,
        &[El::new(
            "press",
            Kind::Press {
                act: Act::ToggleLog,
                name: "Log".into(),
                help: None,
                pad: (0.0, 0.0),
                lead: 0.0,
                value: Some("Hidden".into()),
            },
        )
        .with(vec![text("t", "Log", 11.5, Tone::Muted)])],
    );
    let press = t.children(list)[0];
    assert_eq!(t.get(press).unwrap().a11y.name, "Log, Hidden");
    let inner = t.children(press)[0];
    assert_eq!(t.get(inner).unwrap().a11y.role, copper_win_ui::Role::None);
    assert!(!t.get(inner).unwrap().flags.hit);
}
