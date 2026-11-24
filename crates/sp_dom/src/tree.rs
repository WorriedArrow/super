//! the dom standard's mutation algorithms.
//!
//! where the spec runs steps for mutation observers, live ranges, node
//! iterators, slots or custom element reactions, the step is a comment or a
//! no-op hook below. they get bodies when those features land.

use std::fmt;

use crate::document::Document;
use crate::node::{NodeData, NodeId};

/// https://webidl.spec.whatwg.org/#idl-DOMException-error-names
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomException {
    HierarchyRequestError,
    NotFoundError,
    InvalidCharacterError,
    NamespaceError,
    NotSupportedError,
}

impl fmt::Display for DomException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for DomException {}

pub type DomResult<T> = Result<T, DomException>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Document,
    Fragment,
    Doctype,
    Element,
    Text,
    OtherCharacterData,
}

/// what the validity checks need to know about the node being inserted, so
/// they also work for a node that is still in another document's arena.
#[derive(Clone, Copy)]
pub(crate) struct Candidate {
    kind: Kind,
    /// element children of a fragment, capped at 2.
    fragment_elements: u8,
    fragment_has_text: bool,
}

impl Document {
    pub(crate) fn candidate(&self, node: NodeId) -> Candidate {
        let kind = match self.node(node).data {
            NodeData::Document(_) => Kind::Document,
            NodeData::DocumentFragment { .. } => Kind::Fragment,
            NodeData::Doctype(_) => Kind::Doctype,
            NodeData::Element(_) => Kind::Element,
            NodeData::Text(_) | NodeData::CdataSection(_) => Kind::Text,
            NodeData::Comment(_) | NodeData::ProcessingInstruction { .. } => {
                Kind::OtherCharacterData
            }
        };
        let mut candidate = Candidate {
            kind,
            fragment_elements: 0,
            fragment_has_text: false,
        };
        if kind == Kind::Fragment {
            for c in self.children(node) {
                let child = self.node(c);
                if child.is_element() {
                    candidate.fragment_elements = (candidate.fragment_elements + 1).min(2);
                }
                candidate.fragment_has_text |= child.is_text();
            }
        }
        candidate
    }

    /// https://dom.spec.whatwg.org/#concept-node-ensure-pre-insert-validity
    pub fn ensure_pre_insert_validity(
        &self,
        node: NodeId,
        parent: NodeId,
        child: Option<NodeId>,
    ) -> DomResult<()> {
        self.check_validity(self.candidate(node), Some(node), parent, child, false)
    }

    /// the shared steps 1-6 of ensure pre-insert validity (`replace` false)
    /// and replace (`replace` true, `child` is the node being replaced).
    /// `node` is none when the candidate lives in another arena, where it
    /// can't be an ancestor of `parent`.
    pub(crate) fn check_validity(
        &self,
        candidate: Candidate,
        node: Option<NodeId>,
        parent: NodeId,
        child: Option<NodeId>,
        replace: bool,
    ) -> DomResult<()> {
        use DomException::{HierarchyRequestError, NotFoundError};
        let parent_node = self.node(parent);
        // step 1
        if !matches!(
            parent_node.data,
            NodeData::Document(_) | NodeData::DocumentFragment { .. } | NodeData::Element(_)
        ) {
            return Err(HierarchyRequestError);
        }
        // step 2
        if let Some(node) = node
            && self.is_host_including_inclusive_ancestor(node, parent)
        {
            return Err(HierarchyRequestError);
        }
        // step 3
        if let Some(child) = child
            && self.node(child).parent != Some(parent)
        {
            return Err(NotFoundError);
        }
        // step 4
        if candidate.kind == Kind::Document {
            return Err(HierarchyRequestError);
        }
        // step 5
        let parent_is_document = parent_node.is_document();
        if (candidate.kind == Kind::Text && parent_is_document)
            || (candidate.kind == Kind::Doctype && !parent_is_document)
        {
            return Err(HierarchyRequestError);
        }
        // step 6
        if !parent_is_document {
            return Ok(());
        }
        let is_element = |n: NodeId| self.node(n).is_element();
        let is_doctype = |n: NodeId| self.node(n).is_doctype();
        // a document's children are only doctypes, elements, comments and
        // processing instructions, so "following" and "preceding" child
        // reduce to its following and preceding siblings.
        let doctype_following = |c: NodeId| self.following_siblings(c).any(is_doctype);
        let element_rule = || match (replace, child) {
            (true, Some(c)) => {
                self.children(parent).any(|e| e != c && is_element(e)) || doctype_following(c)
            }
            _ => {
                self.children(parent).any(is_element)
                    || child.is_some_and(|c| is_doctype(c) || doctype_following(c))
            }
        };
        let invalid = match candidate.kind {
            Kind::Fragment => {
                candidate.fragment_elements > 1
                    || candidate.fragment_has_text
                    || (candidate.fragment_elements == 1 && element_rule())
            }
            Kind::Element => element_rule(),
            Kind::Doctype => match (replace, child) {
                (true, Some(c)) => {
                    self.children(parent).any(|d| d != c && is_doctype(d))
                        || self.preceding_siblings(c).any(is_element)
                }
                _ => {
                    self.children(parent).any(is_doctype)
                        || match child {
                            Some(c) => self.preceding_siblings(c).any(is_element),
                            None => self.children(parent).any(is_element),
                        }
                }
            },
            _ => false,
        };
        if invalid {
            return Err(HierarchyRequestError);
        }
        Ok(())
    }

    /// https://dom.spec.whatwg.org/#concept-node-pre-insert
    pub fn pre_insert(
        &mut self,
        node: NodeId,
        parent: NodeId,
        child: Option<NodeId>,
    ) -> DomResult<NodeId> {
        // step 1
        self.ensure_pre_insert_validity(node, parent, child)?;
        // step 2-3
        let mut reference_child = child;
        if reference_child == Some(node) {
            reference_child = self.node(node).next_sibling;
        }
        // step 4
        self.insert(node, parent, reference_child, false);
        // step 5
        Ok(node)
    }

    /// https://dom.spec.whatwg.org/#concept-node-append
    pub fn append(&mut self, node: NodeId, parent: NodeId) -> DomResult<NodeId> {
        self.pre_insert(node, parent, None)
    }

    /// https://dom.spec.whatwg.org/#concept-node-insert
    ///
    /// the caller must have ensured pre-insert validity (the html tree
    /// builder's "if it is possible to insert"), and `child`, if any, must be
    /// a child of `parent` other than `node`.
    pub fn insert(
        &mut self,
        node: NodeId,
        parent: NodeId,
        child: Option<NodeId>,
        suppress_observers: bool,
    ) {
        if self.node(node).is_document_fragment() {
            // step 1-3
            let nodes: Vec<NodeId> = self.children(node).collect();
            if nodes.is_empty() {
                return;
            }
            // step 4.1
            for &n in &nodes {
                self.remove(n, true);
            }
            // step 4.2: queue a tree mutation record for node (mutation
            // observers).
            self.insert_nodes(&nodes, parent, child, suppress_observers);
        } else {
            self.insert_nodes(
                std::slice::from_ref(&node),
                parent,
                child,
                suppress_observers,
            );
        }
    }

    /// steps 5-10 of insert.
    pub(crate) fn insert_nodes(
        &mut self,
        nodes: &[NodeId],
        parent: NodeId,
        child: Option<NodeId>,
        _suppress_observers: bool,
    ) {
        // step 5: live range updates (ranges).
        // step 6: previousSibling only feeds the mutation record.
        let document = self.node(parent).document;
        // step 7
        for &node in nodes {
            // step 7.1
            self.adopt_node(node, document);
            // step 7.2-7.3
            self.link(node, parent, child);
            // step 7.4-7.6: slot assignment (shadow dom).
            // step 7.7
            self.run_insertion_steps(node);
        }
        // step 8: queue a tree mutation record unless suppressed.
        // step 9
        self.run_children_changed_steps(parent);
        // step 10: post-connection steps.
    }

    /// step 7.7 of insert: the insertion steps and custom element reactions
    /// for each shadow-including inclusive descendant of `node`. no
    /// insertion steps exist yet, so the descendant walk is skipped rather
    /// than paid for nothing.
    fn run_insertion_steps(&mut self, _node: NodeId) {}

    /// https://dom.spec.whatwg.org/#concept-node-children-changed-ext
    fn run_children_changed_steps(&mut self, _parent: NodeId) {}

    /// https://dom.spec.whatwg.org/#concept-node-remove-ext, run by step 15
    /// and 18 of remove for `node` and its shadow-including descendants.
    fn run_removing_steps(&mut self, _node: NodeId, _parent: NodeId) {}

    /// links `node` (which has no parent) into `parent` before `child`, or
    /// last when `child` is none.
    fn link(&mut self, node: NodeId, parent: NodeId, child: Option<NodeId>) {
        let prev = match child {
            Some(c) => self.node(c).prev_sibling,
            None => self.node(parent).last_child,
        };
        {
            let n = self.node_mut(node);
            n.parent = Some(parent);
            n.prev_sibling = prev;
            n.next_sibling = child;
        }
        match prev {
            Some(p) => self.node_mut(p).next_sibling = Some(node),
            None => self.node_mut(parent).first_child = Some(node),
        }
        match child {
            Some(c) => self.node_mut(c).prev_sibling = Some(node),
            None => self.node_mut(parent).last_child = Some(node),
        }
    }

    pub(crate) fn link_last(&mut self, node: NodeId, parent: NodeId) {
        self.link(node, parent, None);
    }

    fn unlink(&mut self, node: NodeId) {
        let (parent, prev, next) = {
            let n = self.node_mut(node);
            (
                n.parent.take(),
                n.prev_sibling.take(),
                n.next_sibling.take(),
            )
        };
        let Some(parent) = parent else { return };
        match prev {
            Some(p) => self.node_mut(p).next_sibling = next,
            None => self.node_mut(parent).first_child = next,
        }
        match next {
            Some(n) => self.node_mut(n).prev_sibling = prev,
            None => self.node_mut(parent).last_child = prev,
        }
    }

    /// https://dom.spec.whatwg.org/#concept-node-pre-remove
    pub fn pre_remove(&mut self, child: NodeId, parent: NodeId) -> DomResult<NodeId> {
        // step 1
        if self.node(child).parent != Some(parent) {
            return Err(DomException::NotFoundError);
        }
        // step 2
        self.remove(child, false);
        // step 3
        Ok(child)
    }

    /// https://dom.spec.whatwg.org/#concept-node-remove. does nothing if
    /// `node` has no parent.
    pub fn remove(&mut self, node: NodeId, _suppress_observers: bool) {
        // step 1-2
        let Some(parent) = self.node(node).parent else {
            return;
        };
        // step 3-8: node's index, live range and node iterator updates
        // (ranges). the index is only computed once those exist, since it
        // costs a walk over the preceding siblings.
        // step 9-10: oldPreviousSibling and oldNextSibling feed the mutation
        // record.
        // step 11
        self.unlink(node);
        // step 12-14: slot assignment (shadow dom).
        // step 15
        self.run_removing_steps(node, parent);
        // step 16-17: disconnectedCallback (custom elements).
        // step 18: removing steps for shadow-including descendants, and
        // their custom element reactions. none exist yet.
        // step 19: transient registered observers (mutation observers).
        // step 20: queue a tree mutation record unless suppressed.
        // step 21
        self.run_children_changed_steps(parent);
    }

    /// https://dom.spec.whatwg.org/#concept-node-replace. replaces `child`
    /// with `node` within `parent`.
    pub fn replace(&mut self, child: NodeId, node: NodeId, parent: NodeId) -> DomResult<NodeId> {
        // step 1-6
        self.check_validity(self.candidate(node), Some(node), parent, Some(child), true)?;
        // step 7-8
        let mut reference_child = self.node(child).next_sibling;
        if reference_child == Some(node) {
            reference_child = self.node(node).next_sibling;
        }
        // step 9-10: previousSibling and removedNodes feed the mutation
        // record.
        // step 11
        if self.node(child).parent.is_some() {
            self.remove(child, true);
        }
        // step 12: nodes feeds the mutation record.
        // step 13
        self.insert(node, parent, reference_child, true);
        // step 14: queue a tree mutation record.
        // step 15
        Ok(child)
    }

    /// https://dom.spec.whatwg.org/#concept-node-replace-all
    pub fn replace_all(&mut self, node: Option<NodeId>, parent: NodeId) {
        // step 1-4: removedNodes and addedNodes feed the mutation record.
        // step 5
        while let Some(child) = self.node(parent).first_child {
            self.remove(child, true);
        }
        // step 6
        if let Some(node) = node {
            self.insert(node, parent, None, true);
        }
        // step 7: queue a tree mutation record.
    }

    /// https://dom.spec.whatwg.org/#concept-node-adopt, into `document`, a
    /// document node in this arena. fails with NotSupportedError for a
    /// document node, as adoptNode() does.
    pub fn adopt(&mut self, node: NodeId, document: NodeId) -> DomResult<()> {
        if self.node(node).is_document() {
            return Err(DomException::NotSupportedError);
        }
        self.adopt_node(node, document);
        Ok(())
    }

    pub(crate) fn adopt_node(&mut self, node: NodeId, document: NodeId) {
        // step 1
        let old_document = self.node(node).document;
        // step 2
        if self.node(node).parent.is_some() {
            self.remove(node, false);
        }
        // step 3
        if document == old_document {
            return;
        }
        // step 3.1: attributes hold no node document of their own here.
        let mut next = Some(node);
        while let Some(id) = next {
            self.node_mut(id).document = document;
            next = self.next_in_tree_order(id, node);
        }
        // step 3.2: adoptedCallback (custom elements).
        // step 3.3: adopting steps for each inclusive descendant.
        let mut next = Some(node);
        while let Some(id) = next {
            self.run_adopting_steps(id);
            next = self.next_in_tree_order(id, node);
        }
    }

    /// https://dom.spec.whatwg.org/#concept-node-adopt-ext
    fn run_adopting_steps(&mut self, node: NodeId) {
        // https://html.spec.whatwg.org/multipage/scripting.html#template-adopting-steps
        if let Some(contents) = self.template_contents(node) {
            // step 1
            let document = self.node(node).document;
            let doc = self.appropriate_template_contents_owner_document(document);
            // step 2
            self.adopt_node(contents, doc);
        }
    }

    /// https://dom.spec.whatwg.org/#concept-cd-replace with offset 0 and
    /// count the whole length, as the data setter does.
    pub fn set_data(&mut self, node: NodeId, data: impl Into<String>) {
        if let Some(s) = self.character_data_mut(node) {
            *s = data.into();
        }
        self.after_replace_data(node);
    }

    /// https://dom.spec.whatwg.org/#concept-cd-append, which is replace data
    /// at offset length with count 0.
    pub fn append_data(&mut self, node: NodeId, data: &str) {
        if let Some(s) = self.character_data_mut(node) {
            s.push_str(data);
        }
        self.after_replace_data(node);
    }

    fn character_data_mut(&mut self, node: NodeId) -> Option<&mut String> {
        match &mut self.node_mut(node).data {
            NodeData::Text(s) | NodeData::CdataSection(s) | NodeData::Comment(s) => Some(s),
            NodeData::ProcessingInstruction { data, .. } => Some(data),
            _ => None,
        }
    }

    /// replace data step 4 (mutation record), steps 8-11 (live ranges) and
    /// step 12.
    fn after_replace_data(&mut self, node: NodeId) {
        // step 12
        if let Some(parent) = self.node(node).parent {
            self.run_children_changed_steps(parent);
        }
    }
}
