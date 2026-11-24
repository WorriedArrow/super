//! the dom: one arena per document, node types and the dom standard's tree
//! algorithms (https://dom.spec.whatwg.org/).

mod atom;
mod attr;
mod clone;
mod document;
mod node;
mod tree;

pub use atom::{Atom, atoms, ns};
pub use attr::{
    NameContext, is_valid_attribute_local_name, is_valid_doctype_name, is_valid_element_local_name,
    is_valid_namespace_prefix, validate_and_extract,
};
pub use document::{Descendants, Document, Siblings};
pub use node::{
    Attribute, Doctype, DocumentData, DocumentKind, Element, Node, NodeData, NodeId, QuirksMode,
};
pub use tree::{DomException, DomResult};

#[cfg(test)]
mod tests;
