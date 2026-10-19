//! The accessibility view of a tree: only nodes with a role, with the
//! nodes in between (layout stacks, decorations) lifted out, hidden
//! subtrees left out.
//!
//! The UI Automation provider navigates this view; it is rebuilt lazily
//! when the tree's accessibility revision moves.

use std::collections::HashMap;

use crate::node::Role;
use crate::tree::{NodeId, Tree};

/// Parent and children of every accessible node. `None` stands for the
/// window itself.
#[derive(Debug, Default)]
pub struct View {
    rev: Option<u32>,
    parent: HashMap<NodeId, Option<NodeId>>,
    children: HashMap<Option<NodeId>, Vec<NodeId>>,
    order: Vec<NodeId>,
}

impl View {
    pub fn new() -> View {
        View::default()
    }

    /// Bring the view up to date with `tree` (cheap when nothing changed).
    pub fn sync(&mut self, tree: &Tree) {
        let rev = tree.a11y_rev();
        if self.rev == Some(rev) {
            return;
        }
        self.rev = Some(rev);
        self.parent.clear();
        for v in self.children.values_mut() {
            v.clear();
        }
        self.order.clear();
        if let Some(root) = tree.root() {
            self.visit(tree, root, None);
        }
        self.children.retain(|_, v| !v.is_empty());
    }

    fn visit(&mut self, tree: &Tree, id: NodeId, parent: Option<NodeId>) {
        let Some(n) = tree.get(id) else {
            return;
        };
        if n.style.hidden {
            return;
        }
        let here = if n.a11y.role == Role::None {
            parent
        } else {
            self.parent.insert(id, parent);
            self.children.entry(parent).or_default().push(id);
            self.order.push(id);
            Some(id)
        };
        for &c in tree.children(id) {
            self.visit(tree, c, here);
        }
    }

    /// Whether `id` is in the view.
    pub fn contains(&self, id: NodeId) -> bool {
        self.parent.contains_key(&id)
    }

    /// The accessible parent (`None`: the window). `None` outside the view
    /// as well; check [`View::contains`].
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.parent.get(&id).copied().flatten()
    }

    /// The accessible children of `id` (`None`: of the window).
    pub fn children(&self, id: Option<NodeId>) -> &[NodeId] {
        self.children.get(&id).map_or(&[], Vec::as_slice)
    }

    pub fn first_child(&self, id: Option<NodeId>) -> Option<NodeId> {
        self.children(id).first().copied()
    }

    pub fn last_child(&self, id: Option<NodeId>) -> Option<NodeId> {
        self.children(id).last().copied()
    }

    /// The sibling `step` places along (`1` next, `-1` previous).
    pub fn sibling(&self, id: NodeId, step: isize) -> Option<NodeId> {
        let parent = *self.parent.get(&id)?;
        let kids = self.children(parent);
        let i = kids.iter().position(|&k| k == id)?;
        let j = i.checked_add_signed(step)?;
        kids.get(j).copied()
    }

    /// Every accessible node in document order.
    pub fn all(&self) -> &[NodeId] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    #[test]
    fn lifts_plain_nodes_and_skips_hidden() {
        let mut t = Tree::new();
        let root = t.add_root(Node::stack());
        let row = t.add(root, Node::row());
        let a = t.add(row, Node::new().role(Role::Button).name("A"));
        let group = t.add(row, Node::column().role(Role::Group));
        let b = t.add(group, Node::text("B", 13.0));
        let hidden = t.add(root, Node::new().role(Role::Button));
        t.set_hidden(hidden, true);
        let mut v = View::new();
        v.sync(&t);
        assert_eq!(v.children(None), &[a, group]);
        assert_eq!(v.children(Some(group)), &[b]);
        assert_eq!(v.parent(b), Some(group));
        assert_eq!(v.parent(a), None);
        assert!(!v.contains(hidden));
        assert!(!v.contains(row));
        assert_eq!(v.sibling(a, 1), Some(group));
        assert_eq!(v.sibling(a, -1), None);
        assert_eq!(v.all(), &[a, group, b]);
    }

    #[test]
    fn follows_changes() {
        let mut t = Tree::new();
        let root = t.add_root(Node::stack());
        let a = t.add(root, Node::new().role(Role::Button));
        let mut v = View::new();
        v.sync(&t);
        assert_eq!(v.children(None), &[a]);
        t.set_hidden(a, true);
        v.sync(&t);
        assert!(v.children(None).is_empty());
    }
}
