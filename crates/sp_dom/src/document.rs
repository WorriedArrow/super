//! the per-document arena, node creation and traversal.

use std::num::NonZeroU32;

use crate::atom::{Atom, atoms, ns};
use crate::node::{
    Doctype, DocumentData, DocumentKind, Element, Node, NodeData, NodeId, QuirksMode,
};

struct Slot {
    generation: NonZeroU32,
    node: Option<Node>,
}

/// a document and every node whose node document it is, in one arena.
///
/// the document's associated inert template document (html spec) is
/// created lazily inside the same arena, so template contents share the
/// `NodeId` space of the document that owns the template.
pub struct Document {
    slots: Vec<Slot>,
    free: Vec<u32>,
    root: NodeId,
}

impl Document {
    pub fn new(kind: DocumentKind) -> Document {
        let mut doc = Document {
            slots: Vec::new(),
            free: Vec::new(),
            root: NodeId {
                index: 0,
                generation: NonZeroU32::MIN,
            },
        };
        doc.root = doc.alloc_document(kind, false);
        doc
    }

    fn alloc_document(&mut self, kind: DocumentKind, inert: bool) -> NodeId {
        let placeholder = self.root;
        let id = self.alloc(
            placeholder,
            NodeData::Document(DocumentData {
                kind,
                mode: QuirksMode::NoQuirks,
                inert_template_document: None,
                is_inert_template_document: inert,
            }),
        );
        self.node_mut(id).document = id;
        id
    }

    pub(crate) fn alloc(&mut self, document: NodeId, data: NodeData) -> NodeId {
        let node = Node {
            document,
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            data,
        };
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.node = Some(node);
            return NodeId {
                index,
                generation: slot.generation,
            };
        }
        let index = u32::try_from(self.slots.len()).expect("fewer than 2^32 nodes");
        self.slots.push(Slot {
            generation: NonZeroU32::MIN,
            node: Some(node),
        });
        NodeId {
            index,
            generation: NonZeroU32::MIN,
        }
    }

    /// takes a node out of the arena. every id for its slot goes stale.
    pub(crate) fn take(&mut self, id: NodeId) -> Node {
        let slot = &mut self.slots[id.index as usize];
        assert_eq!(slot.generation, id.generation, "stale NodeId {id:?}");
        let node = slot.node.take().expect("live node");
        slot.generation = slot.generation.checked_add(1).unwrap_or(NonZeroU32::MIN);
        self.free.push(id.index);
        node
    }

    /// the node for `id`, or none if `id` is stale.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_ref()
    }

    /// the node for `id`. panics if `id` is stale or from another document.
    pub fn node(&self, id: NodeId) -> &Node {
        match self.get(id) {
            Some(node) => node,
            None => panic!("stale NodeId {id:?}"),
        }
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> &mut Node {
        let slot = &mut self.slots[id.index as usize];
        assert_eq!(slot.generation, id.generation, "stale NodeId {id:?}");
        slot.node.as_mut().expect("live node")
    }

    pub(crate) fn element_mut(&mut self, id: NodeId) -> &mut Element {
        match &mut self.node_mut(id).data {
            NodeData::Element(e) => e,
            _ => panic!("{id:?} is not an element"),
        }
    }

    /// one past the largest `NodeId::index` in use.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// the document node itself.
    pub fn document_node(&self) -> NodeId {
        self.root
    }

    fn document_data(&self, document: NodeId) -> &DocumentData {
        self.node(document)
            .as_document()
            .expect("node document is a document")
    }

    fn document_data_mut(&mut self, document: NodeId) -> &mut DocumentData {
        match &mut self.node_mut(document).data {
            NodeData::Document(d) => d,
            _ => panic!("{document:?} is not a document"),
        }
    }

    /// whether `document` (a document node in this arena) is an html
    /// document.
    pub fn is_html_document(&self, document: NodeId) -> bool {
        self.document_data(document).kind == DocumentKind::Html
    }

    pub fn kind(&self) -> DocumentKind {
        self.document_data(self.root).kind
    }

    pub fn quirks_mode(&self) -> QuirksMode {
        self.document_data(self.root).mode
    }

    pub fn set_quirks_mode(&mut self, mode: QuirksMode) {
        let root = self.root;
        self.document_data_mut(root).mode = mode;
    }

    /// https://html.spec.whatwg.org/multipage/scripting.html#appropriate-template-contents-owner-document
    pub fn appropriate_template_contents_owner_document(&mut self, document: NodeId) -> NodeId {
        let data = self.document_data(document);
        // step 1: if doc is not a document created by this algorithm
        if data.is_inert_template_document {
            return document;
        }
        if let Some(inert) = data.inert_template_document {
            // step 1.2
            return inert;
        }
        // step 1.1.1-1.1.2: a new document, html if doc is
        let new_doc = self.alloc_document(data.kind, true);
        // step 1.1.3
        self.document_data_mut(document).inert_template_document = Some(new_doc);
        new_doc
    }

    // node creation. `document` is the new node's node document and must be
    // a document node in this arena.

    /// https://dom.spec.whatwg.org/#concept-create-element, for elements
    /// that aren't custom.
    pub fn create_element(
        &mut self,
        document: NodeId,
        local_name: Atom,
        namespace: Option<Atom>,
        prefix: Option<Atom>,
    ) -> NodeId {
        // step 6 (otherwise branch): custom element lookup, the is value and
        // custom element state would go here once custom elements exist.
        let element = self.alloc(
            document,
            NodeData::Element(Element {
                namespace,
                prefix,
                local_name,
                attributes: Vec::new(),
                template_contents: None,
            }),
        );
        if namespace == Some(ns::HTML) && local_name == atoms::TEMPLATE {
            // https://html.spec.whatwg.org/multipage/scripting.html#the-template-element
            // "when a template element is created":
            // step 1
            let doc = self.appropriate_template_contents_owner_document(document);
            // step 2
            let contents = self.alloc(
                doc,
                NodeData::DocumentFragment {
                    host: Some(element),
                },
            );
            // step 3
            self.element_mut(element).template_contents = Some(contents);
        }
        element
    }

    pub fn create_text(&mut self, document: NodeId, data: impl Into<String>) -> NodeId {
        self.alloc(document, NodeData::Text(data.into()))
    }

    pub fn create_cdata_section(&mut self, document: NodeId, data: impl Into<String>) -> NodeId {
        self.alloc(document, NodeData::CdataSection(data.into()))
    }

    pub fn create_comment(&mut self, document: NodeId, data: impl Into<String>) -> NodeId {
        self.alloc(document, NodeData::Comment(data.into()))
    }

    pub fn create_processing_instruction(
        &mut self,
        document: NodeId,
        target: impl Into<String>,
        data: impl Into<String>,
    ) -> NodeId {
        self.alloc(
            document,
            NodeData::ProcessingInstruction {
                target: target.into(),
                data: data.into(),
            },
        )
    }

    pub fn create_doctype(&mut self, document: NodeId, doctype: Doctype) -> NodeId {
        self.alloc(document, NodeData::Doctype(doctype))
    }

    pub fn create_document_fragment(&mut self, document: NodeId) -> NodeId {
        self.alloc(document, NodeData::DocumentFragment { host: None })
    }

    // accessors

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        self.node(id).as_element()
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    /// the parent if it is an element.
    pub fn parent_element(&self, id: NodeId) -> Option<NodeId> {
        self.parent(id).filter(|&p| self.node(p).is_element())
    }

    pub fn template_contents(&self, id: NodeId) -> Option<NodeId> {
        self.element(id).and_then(Element::template_contents)
    }

    /// https://dom.spec.whatwg.org/#concept-documentfragment-host
    pub fn host(&self, id: NodeId) -> Option<NodeId> {
        match self.node(id).data {
            NodeData::DocumentFragment { host } => host,
            _ => None,
        }
    }

    // traversal

    pub fn children(&self, id: NodeId) -> Siblings<'_> {
        Siblings {
            doc: self,
            next: self.node(id).first_child,
        }
    }

    pub fn element_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(id).filter(|&c| self.node(c).is_element())
    }

    pub fn first_element_child(&self, id: NodeId) -> Option<NodeId> {
        self.element_children(id).next()
    }

    /// siblings after `id`, nearest first.
    pub fn following_siblings(&self, id: NodeId) -> Siblings<'_> {
        Siblings {
            doc: self,
            next: self.node(id).next_sibling,
        }
    }

    /// siblings before `id`, nearest first.
    pub fn preceding_siblings(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.node(id).prev_sibling, |&s| self.node(s).prev_sibling)
    }

    /// ancestors of `id`, nearest first, not including `id`.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.node(id).parent, |&p| self.node(p).parent)
    }

    pub fn inclusive_ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(Some(id), |&p| self.node(p).parent)
    }

    /// https://dom.spec.whatwg.org/#concept-tree-descendant, in tree order.
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        let mut iter = self.inclusive_descendants(id);
        iter.next();
        iter
    }

    /// `id` then its descendants, in tree order.
    pub fn inclusive_descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants {
            doc: self,
            root: id,
            next: Some(id),
        }
    }

    /// the node after `id` in tree order, staying inside `root`'s subtree.
    pub fn next_in_tree_order(&self, id: NodeId, root: NodeId) -> Option<NodeId> {
        let node = self.node(id);
        if let Some(child) = node.first_child {
            return Some(child);
        }
        let mut current = id;
        loop {
            if current == root {
                return None;
            }
            let node = self.node(current);
            if let Some(next) = node.next_sibling {
                return Some(next);
            }
            current = node.parent?;
        }
    }

    /// https://dom.spec.whatwg.org/#concept-tree-root
    pub fn root(&self, id: NodeId) -> NodeId {
        self.inclusive_ancestors(id).last().unwrap_or(id)
    }

    /// https://dom.spec.whatwg.org/#connected. there are no shadow roots yet,
    /// so the shadow-including root is the root.
    pub fn is_connected(&self, id: NodeId) -> bool {
        self.node(self.root(id)).is_document()
    }

    /// https://dom.spec.whatwg.org/#concept-tree-index
    pub fn index(&self, id: NodeId) -> usize {
        self.preceding_siblings(id).count()
    }

    /// https://dom.spec.whatwg.org/#concept-tree-inclusive-ancestor
    pub fn is_inclusive_ancestor(&self, ancestor: NodeId, id: NodeId) -> bool {
        self.inclusive_ancestors(id).any(|a| a == ancestor)
    }

    /// https://dom.spec.whatwg.org/#concept-tree-host-including-inclusive-ancestor
    pub fn is_host_including_inclusive_ancestor(&self, ancestor: NodeId, id: NodeId) -> bool {
        let mut current = id;
        loop {
            if self.is_inclusive_ancestor(ancestor, current) {
                return true;
            }
            match self.host(self.root(current)) {
                Some(host) => current = host,
                None => return false,
            }
        }
    }

    /// https://dom.spec.whatwg.org/#document-element
    pub fn document_element(&self) -> Option<NodeId> {
        self.first_element_child(self.root)
    }

    /// https://dom.spec.whatwg.org/#concept-descendant-text-content
    pub fn descendant_text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        for d in self.descendants(id) {
            if let NodeData::Text(s) | NodeData::CdataSection(s) = &self.node(d).data {
                out.push_str(s);
            }
        }
        out
    }

    /// https://dom.spec.whatwg.org/#concept-child-text-content
    pub fn child_text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        for c in self.children(id) {
            if let NodeData::Text(s) | NodeData::CdataSection(s) = &self.node(c).data {
                out.push_str(s);
            }
        }
        out
    }

    /// https://html.spec.whatwg.org/multipage/dom.html#document.title (getter)
    pub fn title(&self) -> String {
        let root_element = self.document_element();
        let svg_root = root_element.filter(|&e| {
            self.element(e)
                .is_some_and(|e| e.is_in_namespace(ns::SVG) && e.local_name == atoms::SVG)
        });
        let title = match svg_root {
            // step 1: the first svg title element child of the svg document
            // element.
            Some(svg) => self.element_children(svg).find(|&c| {
                self.element(c)
                    .is_some_and(|e| e.is_in_namespace(ns::SVG) && e.local_name == atoms::TITLE)
            }),
            // step 2: the title element, the first html title element in the
            // document in tree order.
            None => self
                .descendants(self.root)
                .find(|&d| self.element(d).is_some_and(|e| e.is_html(atoms::TITLE))),
        };
        let value = title
            .map(|t| self.child_text_content(t))
            .unwrap_or_default();
        // step 3: strip and collapse ascii whitespace
        strip_and_collapse_ascii_whitespace(&value)
    }
}

/// https://infra.spec.whatwg.org/#strip-and-collapse-ascii-whitespace
fn strip_and_collapse_ascii_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split(|c: char| c.is_ascii_whitespace()) {
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// children or following siblings, in tree order.
pub struct Siblings<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl Iterator for Siblings<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let id = self.next?;
        self.next = self.doc.node(id).next_sibling;
        Some(id)
    }
}

/// a subtree in tree order (preorder, depth first).
pub struct Descendants<'a> {
    doc: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let id = self.next?;
        self.next = self.doc.next_in_tree_order(id, self.root);
        Some(id)
    }
}
