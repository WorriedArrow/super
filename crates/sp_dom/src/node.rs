//! node ids and node data.

use std::borrow::Cow;
use std::num::NonZeroU32;

use crate::atom::{Atom, ns};

/// a node in a `Document`'s arena. the generation makes ids of freed slots
/// stale instead of silently pointing at whatever reuses the slot.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId {
    pub(crate) index: u32,
    pub(crate) generation: NonZeroU32,
}

impl NodeId {
    /// the arena slot, for side tables indexed by node (style, layout).
    /// always below `Document::slot_count`.
    pub fn index(self) -> usize {
        self.index as usize
    }
}

/// https://dom.spec.whatwg.org/#concept-document-mode
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum QuirksMode {
    #[default]
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

/// https://dom.spec.whatwg.org/#concept-document-type
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocumentKind {
    Xml,
    Html,
}

#[derive(Clone, Debug)]
pub struct DocumentData {
    pub kind: DocumentKind,
    pub mode: QuirksMode,
    /// https://html.spec.whatwg.org/multipage/scripting.html#associated-inert-template-document
    pub(crate) inert_template_document: Option<NodeId>,
    /// true for documents created by the appropriate template contents
    /// owner document algorithm.
    pub(crate) is_inert_template_document: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doctype {
    pub name: String,
    pub public_id: String,
    pub system_id: String,
}

/// https://dom.spec.whatwg.org/#concept-attribute. attributes are plain
/// values in their element's attribute list, not arena nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub namespace: Option<Atom>,
    pub prefix: Option<Atom>,
    pub local_name: Atom,
    pub value: String,
}

impl Attribute {
    /// an attribute with a null namespace and prefix.
    pub fn new(local_name: Atom, value: impl Into<String>) -> Attribute {
        Attribute {
            namespace: None,
            prefix: None,
            local_name,
            value: value.into(),
        }
    }

    /// https://dom.spec.whatwg.org/#concept-attribute-qualified-name
    pub fn qualified_name(&self) -> Cow<'static, str> {
        qualified_name(self.prefix, self.local_name)
    }

    pub(crate) fn qualified_name_is(&self, name: &str) -> bool {
        qualified_name_is(self.prefix, self.local_name, name)
    }
}

fn qualified_name(prefix: Option<Atom>, local_name: Atom) -> Cow<'static, str> {
    match prefix {
        None => Cow::Borrowed(local_name.as_str()),
        Some(prefix) => Cow::Owned(format!("{prefix}:{local_name}")),
    }
}

fn qualified_name_is(prefix: Option<Atom>, local_name: Atom, name: &str) -> bool {
    match prefix {
        None => local_name == name,
        Some(prefix) => name
            .strip_prefix(prefix.as_str())
            .and_then(|rest| rest.strip_prefix(':'))
            .is_some_and(|rest| local_name == rest),
    }
}

/// https://dom.spec.whatwg.org/#interface-element
#[derive(Clone, Debug)]
pub struct Element {
    pub(crate) namespace: Option<Atom>,
    pub(crate) prefix: Option<Atom>,
    pub(crate) local_name: Atom,
    pub(crate) attributes: Vec<Attribute>,
    /// https://html.spec.whatwg.org/multipage/scripting.html#template-contents
    pub(crate) template_contents: Option<NodeId>,
}

impl Element {
    pub fn namespace(&self) -> Option<Atom> {
        self.namespace
    }

    pub fn prefix(&self) -> Option<Atom> {
        self.prefix
    }

    pub fn local_name(&self) -> Atom {
        self.local_name
    }

    /// https://dom.spec.whatwg.org/#concept-element-qualified-name
    pub fn qualified_name(&self) -> Cow<'static, str> {
        qualified_name(self.prefix, self.local_name)
    }

    /// true for an element in the html namespace with this local name.
    pub fn is_html(&self, local_name: Atom) -> bool {
        self.namespace == Some(ns::HTML) && self.local_name == local_name
    }

    pub fn is_in_namespace(&self, namespace: Atom) -> bool {
        self.namespace == Some(namespace)
    }

    pub fn attributes(&self) -> &[Attribute] {
        &self.attributes
    }

    /// the value of the attribute with this local name and a null
    /// namespace, which is how html attributes are stored.
    pub fn attribute(&self, local_name: Atom) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.namespace.is_none() && a.local_name == local_name)
            .map(|a| a.value.as_str())
    }

    pub fn has_attribute(&self, local_name: Atom) -> bool {
        self.attribute(local_name).is_some()
    }

    /// the template contents fragment, for html template elements.
    pub fn template_contents(&self) -> Option<NodeId> {
        self.template_contents
    }
}

/// what kind of node this is and its kind-specific state.
#[derive(Clone, Debug)]
pub enum NodeData {
    Document(DocumentData),
    Doctype(Doctype),
    DocumentFragment {
        /// https://dom.spec.whatwg.org/#concept-documentfragment-host
        host: Option<NodeId>,
    },
    Element(Element),
    Text(String),
    CdataSection(String),
    Comment(String),
    ProcessingInstruction {
        target: String,
        data: String,
    },
}

/// one node in the arena.
#[derive(Clone, Debug)]
pub struct Node {
    pub(crate) document: NodeId,
    pub(crate) parent: Option<NodeId>,
    pub(crate) first_child: Option<NodeId>,
    pub(crate) last_child: Option<NodeId>,
    pub(crate) prev_sibling: Option<NodeId>,
    pub(crate) next_sibling: Option<NodeId>,
    pub(crate) data: NodeData,
}

impl Node {
    /// https://dom.spec.whatwg.org/#concept-node-document. a document's node
    /// document is itself.
    pub fn node_document(&self) -> NodeId {
        self.document
    }

    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    pub fn first_child(&self) -> Option<NodeId> {
        self.first_child
    }

    pub fn last_child(&self) -> Option<NodeId> {
        self.last_child
    }

    pub fn previous_sibling(&self) -> Option<NodeId> {
        self.prev_sibling
    }

    pub fn next_sibling(&self) -> Option<NodeId> {
        self.next_sibling
    }

    pub fn data(&self) -> &NodeData {
        &self.data
    }

    pub fn as_element(&self) -> Option<&Element> {
        match &self.data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_document(&self) -> Option<&DocumentData> {
        match &self.data {
            NodeData::Document(d) => Some(d),
            _ => None,
        }
    }

    /// the data of a text, cdata section, comment or processing instruction.
    pub fn character_data(&self) -> Option<&str> {
        match &self.data {
            NodeData::Text(s) | NodeData::CdataSection(s) | NodeData::Comment(s) => Some(s),
            NodeData::ProcessingInstruction { data, .. } => Some(data),
            _ => None,
        }
    }

    pub fn is_element(&self) -> bool {
        matches!(self.data, NodeData::Element(_))
    }

    /// https://dom.spec.whatwg.org/#text. cdata sections are text nodes.
    pub fn is_text(&self) -> bool {
        matches!(self.data, NodeData::Text(_) | NodeData::CdataSection(_))
    }

    pub fn is_document(&self) -> bool {
        matches!(self.data, NodeData::Document(_))
    }

    pub fn is_doctype(&self) -> bool {
        matches!(self.data, NodeData::Doctype(_))
    }

    pub fn is_document_fragment(&self) -> bool {
        matches!(self.data, NodeData::DocumentFragment { .. })
    }

    pub fn is_character_data(&self) -> bool {
        self.character_data().is_some()
    }
}
