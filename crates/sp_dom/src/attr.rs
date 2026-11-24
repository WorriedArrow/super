//! element attributes and name validation.
//!
//! attributes are addressed by their index in the element's attribute list,
//! since they aren't arena nodes.

use std::borrow::Cow;

use crate::atom::{Atom, ns};
use crate::document::Document;
use crate::node::{Attribute, NodeId};
use crate::tree::{DomException, DomResult};
use crate::wtf8::{Wtf8, Wtf8Buf};

fn is_ascii_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' ')
}

/// https://dom.spec.whatwg.org/#valid-namespace-prefix
pub fn is_valid_namespace_prefix(s: &str) -> bool {
    !s.is_empty()
        && !s
            .chars()
            .any(|c| is_ascii_whitespace(c) || matches!(c, '\0' | '/' | '>'))
}

/// https://dom.spec.whatwg.org/#valid-attribute-local-name
pub fn is_valid_attribute_local_name(s: &str) -> bool {
    !s.is_empty()
        && !s
            .chars()
            .any(|c| is_ascii_whitespace(c) || matches!(c, '\0' | '/' | '=' | '>'))
}

/// https://dom.spec.whatwg.org/#valid-element-local-name
pub fn is_valid_element_local_name(s: &str) -> bool {
    let mut chars = s.chars();
    // step 1
    let Some(first) = chars.next() else {
        return false;
    };
    // step 2
    if first.is_ascii_alphabetic() {
        return !s
            .chars()
            .any(|c| is_ascii_whitespace(c) || matches!(c, '\0' | '/' | '>'));
    }
    // step 3
    if !(matches!(first, ':' | '_') || first >= '\u{80}') {
        return false;
    }
    // step 4-5
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '_') || c >= '\u{80}')
}

/// https://dom.spec.whatwg.org/#valid-doctype-name
pub fn is_valid_doctype_name(s: &str) -> bool {
    !s.chars()
        .any(|c| is_ascii_whitespace(c) || matches!(c, '\0' | '>'))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameContext {
    Element,
    Attribute,
}

/// https://dom.spec.whatwg.org/#validate-and-extract. returns (namespace,
/// prefix, local name).
pub fn validate_and_extract(
    namespace: Option<&str>,
    qualified_name: &str,
    context: NameContext,
) -> DomResult<(Option<Atom>, Option<Atom>, Atom)> {
    use DomException::{InvalidCharacterError, NamespaceError};
    // step 1
    let namespace = namespace.filter(|n| !n.is_empty());
    // step 2-3
    let mut prefix = None;
    let mut local_name = qualified_name;
    // step 4
    if let Some((before, after)) = qualified_name.split_once(':') {
        prefix = Some(before);
        local_name = after;
        if !is_valid_namespace_prefix(before) {
            return Err(InvalidCharacterError);
        }
    }
    // step 6-7
    let valid = match context {
        NameContext::Attribute => is_valid_attribute_local_name(local_name),
        NameContext::Element => is_valid_element_local_name(local_name),
    };
    if !valid {
        return Err(InvalidCharacterError);
    }
    // step 8
    if prefix.is_some() && namespace.is_none() {
        return Err(NamespaceError);
    }
    // step 9
    if prefix == Some("xml") && namespace != Some(ns::XML.as_str()) {
        return Err(NamespaceError);
    }
    // step 10
    let is_xmlns = qualified_name == "xmlns" || prefix == Some("xmlns");
    if is_xmlns && namespace != Some(ns::XMLNS.as_str()) {
        return Err(NamespaceError);
    }
    // step 11
    if namespace == Some(ns::XMLNS.as_str()) && !is_xmlns {
        return Err(NamespaceError);
    }
    // step 12
    Ok((
        namespace.map(Atom::new),
        prefix.map(Atom::new),
        Atom::new(local_name),
    ))
}

impl Document {
    fn attributes(&self, element: NodeId) -> &[Attribute] {
        self.element(element)
            .map(|e| e.attributes())
            .expect("attribute algorithms take an element")
    }

    /// https://dom.spec.whatwg.org/#handle-attribute-changes. the new value
    /// is whatever the element holds now.
    fn handle_attribute_changes(
        &mut self,
        _element: NodeId,
        _local_name: Atom,
        _namespace: Option<Atom>,
        _old_value: Option<Wtf8<'_>>,
    ) {
        // step 1: queue a mutation record (mutation observers).
        // step 2: attributeChangedCallback (custom elements).
        // step 3: the attribute change steps. none exist yet.
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-change
    pub fn change_attribute(&mut self, element: NodeId, index: usize, value: impl Into<Wtf8Buf>) {
        let attribute = &mut self.element_mut(element).attributes[index];
        // step 1-2
        let old_value = std::mem::replace(&mut attribute.value, value.into());
        let (local_name, namespace) = (attribute.local_name, attribute.namespace);
        // step 3
        self.handle_attribute_changes(element, local_name, namespace, Some(old_value.as_wtf8()));
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-append
    pub fn append_attribute(&mut self, element: NodeId, attribute: Attribute) {
        let (local_name, namespace) = (attribute.local_name, attribute.namespace);
        // step 1. step 2-3 (attribute's element and node document) are
        // implied by where it is stored.
        self.element_mut(element).attributes.push(attribute);
        // step 4
        self.handle_attribute_changes(element, local_name, namespace, None);
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-remove
    pub fn remove_attribute(&mut self, element: NodeId, index: usize) -> Attribute {
        // step 1-3
        let removed = self.element_mut(element).attributes.remove(index);
        // step 4
        self.handle_attribute_changes(
            element,
            removed.local_name,
            removed.namespace,
            Some(removed.value.as_wtf8()),
        );
        removed
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-replace.
    /// returns the old attribute.
    pub fn replace_attribute(
        &mut self,
        element: NodeId,
        index: usize,
        new_attribute: Attribute,
    ) -> Attribute {
        // step 1-4
        let old = std::mem::replace(
            &mut self.element_mut(element).attributes[index],
            new_attribute,
        );
        // step 5
        self.handle_attribute_changes(
            element,
            old.local_name,
            old.namespace,
            Some(old.value.as_wtf8()),
        );
        old
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-get-by-name.
    /// returns the attribute's index.
    pub fn get_attribute_by_name(&self, element: NodeId, qualified_name: &str) -> Option<usize> {
        let name = self.adjust_attribute_name(element, qualified_name);
        // step 2
        self.attributes(element)
            .iter()
            .position(|a| a.qualified_name_is(&name))
    }

    /// step 1 of get an attribute by name and step 2 of setAttribute():
    /// lowercase for html elements in html documents.
    fn adjust_attribute_name<'a>(&self, element: NodeId, qualified_name: &'a str) -> Cow<'a, str> {
        let node = self.node(element);
        let html = node
            .as_element()
            .is_some_and(|e| e.is_in_namespace(ns::HTML))
            && self.is_html_document(node.document);
        if html && qualified_name.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(qualified_name.to_ascii_lowercase())
        } else {
            Cow::Borrowed(qualified_name)
        }
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-get-by-namespace.
    /// returns the attribute's index. step 1's empty-string-to-null is the
    /// caller's job, since `namespace` is already an atom or none.
    pub fn get_attribute_by_namespace(
        &self,
        element: NodeId,
        namespace: Option<Atom>,
        local_name: Atom,
    ) -> Option<usize> {
        // step 2
        self.attributes(element)
            .iter()
            .position(|a| a.namespace == namespace && a.local_name == local_name)
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-get-value
    pub fn get_attribute_value(
        &self,
        element: NodeId,
        local_name: Atom,
        namespace: Option<Atom>,
    ) -> Wtf8<'_> {
        // step 1-3
        match self.get_attribute_by_namespace(element, namespace, local_name) {
            Some(i) => self.attributes(element)[i].value.as_wtf8(),
            None => Wtf8::default(),
        }
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-set-value
    pub fn set_attribute_value(
        &mut self,
        element: NodeId,
        local_name: Atom,
        value: impl Into<Wtf8Buf>,
        prefix: Option<Atom>,
        namespace: Option<Atom>,
    ) {
        // step 1
        match self.get_attribute_by_namespace(element, namespace, local_name) {
            // step 2
            None => self.append_attribute(
                element,
                Attribute {
                    namespace,
                    prefix,
                    local_name,
                    value: value.into(),
                },
            ),
            // step 3
            Some(i) => self.change_attribute(element, i, value),
        }
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-remove-by-name
    pub fn remove_attribute_by_name(
        &mut self,
        element: NodeId,
        qualified_name: &str,
    ) -> Option<Attribute> {
        // step 1-3
        let index = self.get_attribute_by_name(element, qualified_name)?;
        Some(self.remove_attribute(element, index))
    }

    /// https://dom.spec.whatwg.org/#concept-element-attributes-remove-by-namespace
    pub fn remove_attribute_by_namespace(
        &mut self,
        element: NodeId,
        namespace: Option<Atom>,
        local_name: Atom,
    ) -> Option<Attribute> {
        // step 1-3
        let index = self.get_attribute_by_namespace(element, namespace, local_name)?;
        Some(self.remove_attribute(element, index))
    }

    /// https://dom.spec.whatwg.org/#dom-element-setattribute
    pub fn set_attribute(
        &mut self,
        element: NodeId,
        qualified_name: &str,
        value: impl Into<Wtf8Buf>,
    ) -> DomResult<()> {
        // step 1
        if !is_valid_attribute_local_name(qualified_name) {
            return Err(DomException::InvalidCharacterError);
        }
        // step 2
        let name = self.adjust_attribute_name(element, qualified_name);
        // step 3
        let found = self
            .attributes(element)
            .iter()
            .position(|a| a.qualified_name_is(&name));
        match found {
            // step 4
            None => {
                let attribute = Attribute::new(Atom::new(&name), value);
                self.append_attribute(element, attribute);
            }
            // step 5
            Some(i) => self.change_attribute(element, i, value),
        }
        Ok(())
    }

    /// https://dom.spec.whatwg.org/#dom-element-setattributens
    pub fn set_attribute_ns(
        &mut self,
        element: NodeId,
        namespace: Option<&str>,
        qualified_name: &str,
        value: impl Into<Wtf8Buf>,
    ) -> DomResult<()> {
        // step 1
        let (namespace, prefix, local_name) =
            validate_and_extract(namespace, qualified_name, NameContext::Attribute)?;
        // step 2
        self.set_attribute_value(element, local_name, value, prefix, namespace);
        Ok(())
    }
}
