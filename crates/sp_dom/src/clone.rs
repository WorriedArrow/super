//! cloning, and moving nodes between documents' arenas.

use crate::document::Document;
use crate::node::{Element, NodeData, NodeId};
use crate::tree::{DomException, DomResult};

/// one node of a subtree being copied, in an order where parents come
/// before their children and siblings keep their order.
struct Entry {
    data: NodeData,
    /// index of the parent entry.
    parent: Option<usize>,
    /// this entry is the template contents of its parent entry.
    contents: bool,
}

impl Document {
    /// `root` and, if `subtree`, its descendants and the template contents
    /// of each, breadth first. creation order isn't observable until custom
    /// element reactions exist.
    fn collect(&self, root: NodeId, subtree: bool) -> Vec<(NodeId, Option<usize>, bool)> {
        let mut out = vec![(root, None, false)];
        if !subtree {
            return out;
        }
        let mut i = 0;
        while i < out.len() {
            let id = out[i].0;
            if let Some(contents) = self.template_contents(id) {
                out.push((contents, Some(i), true));
            }
            out.extend(self.children(id).map(|c| (c, Some(i), false)));
            i += 1;
        }
        out
    }

    fn snapshot(&self, root: NodeId, subtree: bool) -> Vec<Entry> {
        self.collect(root, subtree)
            .into_iter()
            .map(|(id, parent, contents)| Entry {
                data: self.node(id).data.clone(),
                parent,
                contents,
            })
            .collect()
    }

    /// builds copies of `entries` in this arena with node document
    /// `document`, appended to `parent` if given, and returns the copy of
    /// the first entry.
    fn materialize(
        &mut self,
        entries: Vec<Entry>,
        document: NodeId,
        parent: Option<NodeId>,
    ) -> NodeId {
        let mut ids: Vec<NodeId> = Vec::with_capacity(entries.len());
        for entry in entries {
            let parent_copy = entry.parent.map(|p| ids[p]);
            let id = match (entry.contents, parent_copy) {
                // the copied template already got fresh contents when it was
                // created. its children are cloned into those.
                (true, Some(template)) => self
                    .template_contents(template)
                    .expect("template copy has contents"),
                _ => {
                    let doc = parent_copy.map_or(document, |p| self.node(p).document);
                    let id = self.clone_single_node(entry.data, doc);
                    // the source tree was valid, so appending the copy
                    // can't fail pre-insert validity.
                    if let Some(p) = parent_copy.or(if ids.is_empty() { parent } else { None }) {
                        self.link_last(id, p);
                    }
                    id
                }
            };
            ids.push(id);
        }
        ids[0]
    }

    /// https://dom.spec.whatwg.org/#clone-a-single-node, for anything but a
    /// document.
    fn clone_single_node(&mut self, data: NodeData, document: NodeId) -> NodeId {
        match data {
            // step 2: create an element, then append a copy of each
            // attribute.
            NodeData::Element(Element {
                namespace,
                prefix,
                local_name,
                attributes,
                ..
            }) => {
                let copy = self.create_element(document, local_name, namespace, prefix);
                for attribute in attributes {
                    self.append_attribute(copy, attribute);
                }
                copy
            }
            // step 3: a fragment's host isn't copied.
            NodeData::DocumentFragment { .. } => self.create_document_fragment(document),
            NodeData::Document(_) => unreachable!("documents are cloned by clone_document"),
            data => self.alloc(document, data),
        }
    }

    /// https://dom.spec.whatwg.org/#concept-node-clone, with `document` a
    /// document node in this arena. a document is cloned with
    /// `clone_document` instead, since its copy gets its own arena; here it
    /// fails with NotSupportedError.
    pub fn clone_node(
        &mut self,
        node: NodeId,
        document: NodeId,
        subtree: bool,
    ) -> DomResult<NodeId> {
        if self.node(node).is_document() {
            return Err(DomException::NotSupportedError);
        }
        // step 2-5, with the html template cloning steps folded into
        // `collect`.
        let entries = self.snapshot(node, subtree);
        Ok(self.materialize(entries, document, None))
    }

    /// clone a node from another arena into this one, as importNode() does.
    /// fails with NotSupportedError for a document.
    pub fn import_node(
        &mut self,
        source: &Document,
        node: NodeId,
        document: NodeId,
        subtree: bool,
    ) -> DomResult<NodeId> {
        if source.node(node).is_document() {
            return Err(DomException::NotSupportedError);
        }
        let entries = source.snapshot(node, subtree);
        Ok(self.materialize(entries, document, None))
    }

    /// https://dom.spec.whatwg.org/#concept-node-clone for this document's
    /// document node. the copy is a new arena.
    pub fn clone_document(&self, subtree: bool) -> Document {
        let data = self
            .node(self.document_node())
            .as_document()
            .expect("document node");
        // clone a single node step 3: copy type and mode.
        let mut copy = Document::new(data.kind);
        copy.set_quirks_mode(data.mode);
        if subtree {
            let root = copy.document_node();
            for child in self.children(self.document_node()) {
                let entries = self.snapshot(child, true);
                copy.materialize(entries, root, Some(root));
            }
        }
        copy
    }

    /// https://dom.spec.whatwg.org/#concept-node-adopt for a node in another
    /// arena: moves `node` and its subtree from `source` into this arena with
    /// node document `document`, and returns its new id. every id of the
    /// moved nodes in `source` goes stale. fails with NotSupportedError for
    /// a document, as adoptNode() does.
    pub fn adopt_from(
        &mut self,
        source: &mut Document,
        node: NodeId,
        document: NodeId,
    ) -> DomResult<NodeId> {
        if source.node(node).is_document() {
            return Err(DomException::NotSupportedError);
        }
        // step 2
        source.remove(node, false);
        // step 3.1 and the template adopting steps: the subtree, with each
        // template's contents moved into this document's appropriate
        // template contents owner document.
        let entries: Vec<Entry> = source
            .collect(node, true)
            .into_iter()
            .map(|(id, parent, contents)| Entry {
                data: source.take(id).data,
                parent,
                contents,
            })
            .collect();
        // step 3.2: adoptedCallback (custom elements).
        Ok(self.materialize(entries, document, None))
    }

    /// https://dom.spec.whatwg.org/#concept-node-pre-insert for a node in
    /// another arena. validity is checked against `source` before anything
    /// moves, then insert's adopt step moves the nodes here. returns the new
    /// id of `node`, or for a fragment, which stays in `source`, `node`
    /// itself.
    pub fn pre_insert_from(
        &mut self,
        source: &mut Document,
        node: NodeId,
        parent: NodeId,
        child: Option<NodeId>,
    ) -> DomResult<NodeId> {
        // step 1. a node in another arena can't be an ancestor of parent,
        // and can't be child.
        self.check_validity(source.candidate(node), None, parent, child, false)?;
        // step 4: insert. step 1 and 4.1 of insert take the fragment's
        // children out of the fragment, step 7.1 adopts each one.
        let is_fragment = source.node(node).is_document_fragment();
        let nodes: Vec<NodeId> = if is_fragment {
            source.children(node).collect()
        } else {
            vec![node]
        };
        // insert step 3
        if nodes.is_empty() {
            return Ok(node);
        }
        let document = self.node(parent).document;
        let mut moved = Vec::with_capacity(nodes.len());
        for n in nodes {
            moved.push(self.adopt_from(source, n, document)?);
        }
        self.insert_nodes(&moved, parent, child, false);
        // step 5
        Ok(if is_fragment { node } else { moved[0] })
    }
}
