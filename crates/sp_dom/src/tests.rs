use crate::DomException::{
    HierarchyRequestError, InvalidCharacterError, NamespaceError, NotFoundError, NotSupportedError,
};
use crate::*;

fn html_doc() -> (Document, NodeId) {
    let doc = Document::new(DocumentKind::Html);
    let root = doc.document_node();
    (doc, root)
}

fn el(doc: &mut Document, name: &str) -> NodeId {
    let root = doc.document_node();
    doc.create_element(root, Atom::new(name), Some(ns::HTML), None)
}

fn doctype(doc: &mut Document) -> NodeId {
    let root = doc.document_node();
    doc.create_doctype(
        root,
        Doctype {
            name: "html".into(),
            public_id: String::new(),
            system_id: String::new(),
        },
    )
}

fn names(doc: &Document, ids: impl Iterator<Item = NodeId>) -> Vec<String> {
    ids.map(|id| match doc.node(id).data() {
        NodeData::Element(e) => e.local_name().to_string(),
        NodeData::Text(t) => format!("#{t}"),
        NodeData::Doctype(_) => "!doctype".into(),
        NodeData::Comment(c) => format!("<!--{c}-->"),
        _ => "?".into(),
    })
    .collect()
}

#[test]
fn append_insert_and_remove_keep_links() {
    let (mut doc, root) = html_doc();
    let html = el(&mut doc, "html");
    let (a, b, c) = (el(&mut doc, "a"), el(&mut doc, "b"), el(&mut doc, "c"));
    doc.append(html, root).unwrap();
    doc.append(a, html).unwrap();
    doc.append(c, html).unwrap();
    doc.pre_insert(b, html, Some(c)).unwrap();
    assert_eq!(names(&doc, doc.children(html)), ["a", "b", "c"]);
    assert_eq!(doc.node(b).previous_sibling(), Some(a));
    assert_eq!(doc.node(b).next_sibling(), Some(c));
    assert_eq!(doc.node(html).first_child(), Some(a));
    assert_eq!(doc.node(html).last_child(), Some(c));
    assert_eq!(doc.index(c), 2);
    assert_eq!(doc.parent(b), Some(html));
    assert!(doc.is_connected(b));

    // inserting a node before itself leaves it where it is.
    doc.pre_insert(b, html, Some(b)).unwrap();
    assert_eq!(names(&doc, doc.children(html)), ["a", "b", "c"]);
    // moving within the same parent.
    doc.pre_insert(c, html, Some(a)).unwrap();
    assert_eq!(names(&doc, doc.children(html)), ["c", "a", "b"]);

    assert_eq!(doc.pre_remove(a, html), Ok(a));
    assert_eq!(names(&doc, doc.children(html)), ["c", "b"]);
    assert_eq!(doc.parent(a), None);
    assert_eq!(doc.node(a).next_sibling(), None);
    assert!(!doc.is_connected(a));
    assert_eq!(doc.node(c).next_sibling(), Some(b));
    assert_eq!(doc.node(b).previous_sibling(), Some(c));
    assert_eq!(doc.pre_remove(a, html), Err(NotFoundError));
    doc.remove(b, false);
    doc.remove(c, false);
    assert_eq!(doc.node(html).first_child(), None);
    assert_eq!(doc.node(html).last_child(), None);
}

#[test]
fn traversal_is_tree_order() {
    let (mut doc, root) = html_doc();
    let html = el(&mut doc, "html");
    let head = el(&mut doc, "head");
    let body = el(&mut doc, "body");
    let p = el(&mut doc, "p");
    let text = doc.create_text(root, "hi");
    let comment = doc.create_comment(root, "c");
    doc.append(html, root).unwrap();
    doc.append(head, html).unwrap();
    doc.append(body, html).unwrap();
    doc.append(p, body).unwrap();
    doc.append(text, p).unwrap();
    doc.append(comment, body).unwrap();
    assert_eq!(
        names(&doc, doc.descendants(root)),
        ["html", "head", "body", "p", "#hi", "<!--c-->"]
    );
    assert_eq!(names(&doc, doc.inclusive_descendants(head)), ["head"]);
    assert_eq!(names(&doc, doc.descendants(body)), ["p", "#hi", "<!--c-->"]);
    assert_eq!(names(&doc, doc.ancestors(text)), ["p", "body", "html", "?"]);
    assert_eq!(names(&doc, doc.element_children(body)), ["p"]);
    assert_eq!(doc.document_element(), Some(html));
    assert_eq!(doc.root(text), root);
    assert_eq!(doc.parent_element(html), None);
    assert_eq!(doc.parent_element(body), Some(html));
    assert!(doc.is_inclusive_ancestor(html, text));
    assert!(!doc.is_inclusive_ancestor(head, text));
    assert_eq!(doc.descendant_text_content(html), "hi");
    assert_eq!(doc.child_text_content(p), "hi");
    assert_eq!(doc.child_text_content(body), "");
}

#[test]
fn pre_insert_validity_failures() {
    let (mut doc, root) = html_doc();
    let html = el(&mut doc, "html");
    let div = el(&mut doc, "div");
    let span = el(&mut doc, "span");
    let text = doc.create_text(root, "t");
    let dt = doctype(&mut doc);
    doc.append(html, root).unwrap();
    doc.append(div, html).unwrap();

    // step 1: parent isn't a document, fragment or element.
    assert_eq!(doc.append(span, text), Err(HierarchyRequestError));
    // step 2: node is an inclusive ancestor of parent.
    assert_eq!(doc.append(html, div), Err(HierarchyRequestError));
    assert_eq!(doc.append(div, div), Err(HierarchyRequestError));
    // step 3: child isn't a child of parent.
    assert_eq!(doc.pre_insert(span, html, Some(span)), Err(NotFoundError));
    // step 4: node is a document.
    let mut other = Document::new(DocumentKind::Html);
    let other_root = other.document_node();
    let other_el = el(&mut other, "x");
    assert_eq!(
        other.append(other_root, other_el),
        Err(HierarchyRequestError)
    );
    // step 5: text into a document, doctype into an element.
    assert_eq!(doc.append(text, root), Err(HierarchyRequestError));
    assert_eq!(doc.append(dt, div), Err(HierarchyRequestError));
    // step 6, element: the document already has an element child.
    assert_eq!(doc.append(span, root), Err(HierarchyRequestError));
    // step 6, doctype: an element precedes child, or child is null and
    // the document has an element child.
    let comment = doc.create_comment(root, "c");
    doc.append(comment, root).unwrap();
    assert_eq!(
        doc.pre_insert(dt, root, Some(comment)),
        Err(HierarchyRequestError)
    );
    assert_eq!(doc.append(dt, root), Err(HierarchyRequestError));
    doc.pre_insert(dt, root, Some(html)).unwrap();
    // step 6, doctype: the document already has a doctype child.
    let dt2 = doctype(&mut doc);
    assert_eq!(
        doc.pre_insert(dt2, root, Some(dt)),
        Err(HierarchyRequestError)
    );

    // step 6, element: child is a doctype, or a doctype follows child.
    doc.remove(html, false);
    assert_eq!(
        doc.pre_insert(span, root, Some(dt)),
        Err(HierarchyRequestError)
    );
    let first = doc.create_comment(root, "first");
    doc.pre_insert(first, root, Some(dt)).unwrap();
    assert_eq!(
        doc.pre_insert(span, root, Some(first)),
        Err(HierarchyRequestError)
    );
    doc.append(span, root).unwrap();

    // step 6, fragment: more than one element, a text child, or one element
    // when the document has one.
    let frag = doc.create_document_fragment(root);
    let (f1, f2) = (el(&mut doc, "f1"), el(&mut doc, "f2"));
    doc.append(f1, frag).unwrap();
    doc.append(f2, frag).unwrap();
    doc.remove(span, false);
    assert_eq!(doc.append(frag, root), Err(HierarchyRequestError));
    doc.remove(f2, false);
    let ft = doc.create_text(root, "x");
    doc.append(ft, frag).unwrap();
    assert_eq!(doc.append(frag, root), Err(HierarchyRequestError));
    doc.remove(ft, false);
    doc.append(span, root).unwrap();
    assert_eq!(doc.append(frag, root), Err(HierarchyRequestError));
    doc.remove(span, false);
    assert_eq!(
        doc.pre_insert(frag, root, Some(dt)),
        Err(HierarchyRequestError)
    );
    doc.append(frag, root).unwrap();
    assert_eq!(doc.document_element(), Some(f1));
    assert_eq!(doc.node(frag).first_child(), None);
}

#[test]
fn fragment_insertion_moves_children() {
    let (mut doc, root) = html_doc();
    let div = el(&mut doc, "div");
    let last = el(&mut doc, "last");
    doc.append(last, div).unwrap();
    let frag = doc.create_document_fragment(root);
    for name in ["a", "b", "c"] {
        let e = el(&mut doc, name);
        doc.append(e, frag).unwrap();
    }
    doc.pre_insert(frag, div, Some(last)).unwrap();
    assert_eq!(names(&doc, doc.children(div)), ["a", "b", "c", "last"]);
    assert_eq!(doc.children(frag).count(), 0);
    // an empty fragment inserts nothing.
    doc.append(frag, div).unwrap();
    assert_eq!(doc.children(div).count(), 4);
}

#[test]
fn replace_and_its_validity_failures() {
    let (mut doc, root) = html_doc();
    let html = el(&mut doc, "html");
    let (a, b, c) = (el(&mut doc, "a"), el(&mut doc, "b"), el(&mut doc, "c"));
    doc.append(html, root).unwrap();
    doc.append(a, html).unwrap();
    doc.append(b, html).unwrap();
    assert_eq!(doc.replace(a, c, html), Ok(a));
    assert_eq!(names(&doc, doc.children(html)), ["c", "b"]);
    assert_eq!(doc.parent(a), None);
    // replacing with the next sibling.
    doc.replace(c, b, html).unwrap();
    assert_eq!(names(&doc, doc.children(html)), ["b"]);
    // replacing a node with itself.
    doc.replace(b, b, html).unwrap();
    assert_eq!(names(&doc, doc.children(html)), ["b"]);

    let text = doc.create_text(root, "t");
    assert_eq!(doc.replace(a, c, text), Err(HierarchyRequestError));
    assert_eq!(doc.replace(b, html, html), Err(HierarchyRequestError));
    assert_eq!(doc.replace(a, c, html), Err(NotFoundError));
    let dt = doctype(&mut doc);
    assert_eq!(doc.replace(b, dt, html), Err(HierarchyRequestError));
    assert_eq!(doc.replace(html, text, root), Err(HierarchyRequestError));

    // in a document: replacing the document element with another element
    // is fine, but not some other child with an element.
    let comment = doc.create_comment(root, "c");
    doc.append(comment, root).unwrap();
    assert_eq!(doc.replace(comment, c, root), Err(HierarchyRequestError));
    doc.replace(html, c, root).unwrap();
    assert_eq!(doc.document_element(), Some(c));
    // a doctype can't go after the element, and an element can't go before
    // a doctype.
    assert_eq!(doc.replace(comment, dt, root), Err(HierarchyRequestError));
    doc.pre_insert(dt, root, Some(c)).unwrap();
    let dt2 = doctype(&mut doc);
    assert_eq!(doc.replace(c, dt2, root), Err(HierarchyRequestError));
    doc.replace(dt, dt2, root).unwrap();
    let first = doc.create_comment(root, "first");
    doc.pre_insert(first, root, Some(dt2)).unwrap();
    doc.remove(c, false);
    assert_eq!(doc.replace(first, html, root), Err(HierarchyRequestError));
    doc.replace(dt2, html, root).unwrap();
}

#[test]
fn replace_all_children() {
    let (mut doc, _) = html_doc();
    let div = el(&mut doc, "div");
    for name in ["a", "b"] {
        let e = el(&mut doc, name);
        doc.append(e, div).unwrap();
    }
    let x = el(&mut doc, "x");
    doc.replace_all(Some(x), div);
    assert_eq!(names(&doc, doc.children(div)), ["x"]);
    doc.replace_all(None, div);
    assert_eq!(doc.children(div).count(), 0);
}

#[test]
fn template_contents_live_in_the_inert_document() {
    let (mut doc, root) = html_doc();
    let template = el(&mut doc, "template");
    let contents = doc.template_contents(template).unwrap();
    assert_eq!(doc.host(contents), Some(template));
    let inert = doc.node(contents).node_document();
    assert_ne!(inert, root);
    assert!(doc.is_html_document(inert));
    assert_eq!(
        doc.appropriate_template_contents_owner_document(root),
        inert
    );
    assert_eq!(
        doc.appropriate_template_contents_owner_document(inert),
        inert
    );
    // a second template shares the inert document.
    let t2 = el(&mut doc, "template");
    let c2 = doc.template_contents(t2).unwrap();
    assert_eq!(doc.node(c2).node_document(), inert);

    // children appended to the contents are adopted into the inert
    // document, including their descendants.
    let div = el(&mut doc, "div");
    let span = el(&mut doc, "span");
    doc.append(span, div).unwrap();
    doc.append(div, contents).unwrap();
    assert_eq!(doc.node(div).node_document(), inert);
    assert_eq!(doc.node(span).node_document(), inert);
    assert!(!doc.is_connected(div));

    // host-including ancestors: a template can't go into its own contents.
    assert_eq!(doc.append(template, contents), Err(HierarchyRequestError));
    doc.append(t2, div).unwrap();
    assert_eq!(doc.append(template, c2), Err(HierarchyRequestError));

    // a non-template element has no contents.
    assert_eq!(doc.template_contents(span), None);
    let svg_template = doc.create_element(root, atoms::TEMPLATE, Some(ns::SVG), None);
    assert_eq!(doc.template_contents(svg_template), None);
}

#[test]
fn adopt_within_and_across_arenas() {
    let (mut doc, root) = html_doc();
    let div = el(&mut doc, "div");
    let inert = doc.appropriate_template_contents_owner_document(root);
    doc.adopt(div, inert).unwrap();
    assert_eq!(doc.node(div).node_document(), inert);
    assert_eq!(doc.adopt(root, inert), Err(NotSupportedError));

    let (mut other, other_root) = html_doc();
    let parent = el(&mut other, "parent");
    let template = el(&mut other, "template");
    let inner = el(&mut other, "inner");
    let text = other.create_text(other_root, "x");
    other.append(template, parent).unwrap();
    other.append(text, template).unwrap();
    let contents = other.template_contents(template).unwrap();
    other.append(inner, contents).unwrap();
    other.set_attribute(template, "id", "t").unwrap();

    let moved = doc.adopt_from(&mut other, template, root).unwrap();
    assert!(other.get(template).is_none());
    assert!(other.get(inner).is_none());
    assert!(other.get(text).is_none());
    assert_eq!(other.children(parent).count(), 0);
    assert_eq!(doc.node(moved).node_document(), root);
    assert_eq!(doc.element(moved).unwrap().attribute(atoms::ID), Some("t"));
    assert_eq!(doc.child_text_content(moved), "x");
    let new_contents = doc.template_contents(moved).unwrap();
    assert_eq!(doc.node(new_contents).node_document(), inert);
    assert_eq!(doc.host(new_contents), Some(moved));
    let new_inner = doc.node(new_contents).first_child().unwrap();
    assert_eq!(doc.node(new_inner).node_document(), inert);
    assert_eq!(doc.element(new_inner).unwrap().local_name(), "inner");
    assert_eq!(
        doc.adopt_from(&mut other, other_root, root),
        Err(NotSupportedError)
    );

    // freed slots are reused with a new generation.
    let freed = [template, contents, inner, text];
    let reused = el(&mut other, "reused");
    let old = freed.iter().find(|f| f.index() == reused.index()).unwrap();
    assert_ne!(*old, reused);
    assert!(other.get(*old).is_none());
}

#[test]
fn pre_insert_from_another_arena() {
    let (mut doc, root) = html_doc();
    let body = el(&mut doc, "body");
    doc.append(body, root).unwrap();
    let (mut other, other_root) = html_doc();
    let frag = other.create_document_fragment(other_root);
    for name in ["a", "b"] {
        let e = el(&mut other, name);
        other.append(e, frag).unwrap();
    }
    // validity is checked before anything moves.
    assert_eq!(
        doc.pre_insert_from(&mut other, frag, root, None),
        Err(HierarchyRequestError)
    );
    assert_eq!(other.children(frag).count(), 2);
    assert_eq!(doc.pre_insert_from(&mut other, frag, body, None), Ok(frag));
    assert_eq!(names(&doc, doc.children(body)), ["a", "b"]);
    assert_eq!(other.children(frag).count(), 0);
    let c = el(&mut other, "c");
    let first = doc.node(body).first_child();
    let moved = doc.pre_insert_from(&mut other, c, body, first).unwrap();
    assert_eq!(names(&doc, doc.children(body)), ["c", "a", "b"]);
    assert_eq!(doc.node(moved).node_document(), root);
}

#[test]
fn clone_shallow_deep_and_across_arenas() {
    let (mut doc, root) = html_doc();
    let div = el(&mut doc, "div");
    doc.set_attribute(div, "class", "x").unwrap();
    let text = doc.create_text(root, "hello");
    doc.append(text, div).unwrap();
    let template = el(&mut doc, "template");
    doc.append(template, div).unwrap();
    let inner = el(&mut doc, "inner");
    doc.append(inner, doc.template_contents(template).unwrap())
        .unwrap();

    let shallow = doc.clone_node(div, root, false).unwrap();
    assert_eq!(doc.children(shallow).count(), 0);
    assert_eq!(
        doc.element(shallow).unwrap().attribute(atoms::CLASS),
        Some("x")
    );
    assert_eq!(doc.parent(shallow), None);

    let deep = doc.clone_node(div, root, true).unwrap();
    assert_eq!(names(&doc, doc.children(deep)), ["#hello", "template"]);
    let t_copy = doc.node(deep).last_child().unwrap();
    let contents_copy = doc.template_contents(t_copy).unwrap();
    assert_ne!(Some(contents_copy), doc.template_contents(template));
    assert_eq!(doc.host(contents_copy), Some(t_copy));
    assert_eq!(names(&doc, doc.children(contents_copy)), ["inner"]);

    // shallow template clones get empty contents.
    let t_shallow = doc.clone_node(template, root, false).unwrap();
    let c = doc.template_contents(t_shallow).unwrap();
    assert_eq!(doc.children(c).count(), 0);
    // a cloned fragment has no host.
    let frag_copy = doc
        .clone_node(doc.template_contents(template).unwrap(), root, true)
        .unwrap();
    assert_eq!(doc.host(frag_copy), None);
    assert_eq!(names(&doc, doc.children(frag_copy)), ["inner"]);

    assert_eq!(doc.clone_node(root, root, true), Err(NotSupportedError));

    let (mut other, other_root) = html_doc();
    let imported = other.import_node(&doc, div, other_root, true).unwrap();
    assert_eq!(other.descendant_text_content(imported), "hello");
    assert_eq!(other.node(imported).node_document(), other_root);
    assert_eq!(doc.children(div).count(), 2);
    assert_eq!(
        other.import_node(&doc, root, other_root, true),
        Err(NotSupportedError)
    );
}

#[test]
fn clone_document_copies_mode_and_children() {
    let (mut doc, root) = html_doc();
    doc.set_quirks_mode(QuirksMode::Quirks);
    let dt = doctype(&mut doc);
    let html = el(&mut doc, "html");
    doc.append(dt, root).unwrap();
    doc.append(html, root).unwrap();
    let copy = doc.clone_document(true);
    assert_eq!(copy.quirks_mode(), QuirksMode::Quirks);
    assert_eq!(copy.kind(), DocumentKind::Html);
    assert_eq!(
        names(&copy, copy.children(copy.document_node())),
        ["!doctype", "html"]
    );
    let empty = doc.clone_document(false);
    assert_eq!(empty.children(empty.document_node()).count(), 0);
}

#[test]
fn attributes() {
    let (mut doc, root) = html_doc();
    let div = el(&mut doc, "div");
    doc.set_attribute(div, "ID", "a").unwrap();
    assert_eq!(doc.element(div).unwrap().attribute(atoms::ID), Some("a"));
    doc.set_attribute(div, "id", "b").unwrap();
    assert_eq!(doc.element(div).unwrap().attributes().len(), 1);
    assert_eq!(doc.get_attribute_value(div, atoms::ID, None), "b");
    assert_eq!(doc.get_attribute_by_name(div, "Id"), Some(0));
    assert_eq!(
        doc.set_attribute(div, "a b", "x"),
        Err(InvalidCharacterError)
    );
    assert_eq!(doc.set_attribute(div, "", "x"), Err(InvalidCharacterError));

    doc.set_attribute_ns(div, Some(ns::XLINK.as_str()), "xlink:href", "#x")
        .unwrap();
    let i = doc.get_attribute_by_name(div, "xlink:href").unwrap();
    let attribute = &doc.element(div).unwrap().attributes()[i];
    assert_eq!(attribute.local_name, atoms::HREF);
    assert_eq!(attribute.prefix, Some(atoms::XLINK));
    assert_eq!(attribute.qualified_name(), "xlink:href");
    assert_eq!(doc.get_attribute_value(div, atoms::HREF, None), "");
    assert_eq!(
        doc.get_attribute_value(div, atoms::HREF, Some(ns::XLINK)),
        "#x"
    );
    // the null-namespace href is a separate attribute.
    doc.set_attribute_value(div, atoms::HREF, "plain", None, None);
    assert_eq!(
        doc.element(div).unwrap().attribute(atoms::HREF),
        Some("plain")
    );

    let removed = doc
        .remove_attribute_by_namespace(div, Some(ns::XLINK), atoms::HREF)
        .unwrap();
    assert_eq!(removed.value, "#x");
    assert!(doc.remove_attribute_by_name(div, "ID").is_some());
    assert!(doc.remove_attribute_by_name(div, "id").is_none());
    let old = doc.replace_attribute(div, 0, Attribute::new(atoms::CLASS, "c"));
    assert_eq!(old.local_name, atoms::HREF);
    assert_eq!(doc.element(div).unwrap().attribute(atoms::CLASS), Some("c"));
    doc.append_attribute(div, Attribute::new(atoms::TITLE, "t"));
    doc.change_attribute(div, 1, "u");
    assert_eq!(doc.element(div).unwrap().attribute(atoms::TITLE), Some("u"));
    assert_eq!(doc.remove_attribute(div, 0).local_name, atoms::CLASS);

    // no lowercasing outside the html namespace or in xml documents.
    let svg = doc.create_element(root, atoms::SVG, Some(ns::SVG), None);
    doc.set_attribute(svg, "viewBox", "0 0 1 1").unwrap();
    assert_eq!(doc.get_attribute_by_name(svg, "viewbox"), None);
    assert_eq!(doc.get_attribute_by_name(svg, "viewBox"), Some(0));
    let mut xml = Document::new(DocumentKind::Xml);
    let xml_root = xml.document_node();
    let x = xml.create_element(xml_root, atoms::DIV, Some(ns::HTML), None);
    xml.set_attribute(x, "ID", "a").unwrap();
    assert_eq!(xml.get_attribute_by_name(x, "id"), None);
    assert_eq!(xml.get_attribute_by_name(x, "ID"), Some(0));
}

#[test]
fn name_validation() {
    use NameContext::{Attribute, Element};
    let xml = Some(ns::XML.as_str());
    let xmlns = Some(ns::XMLNS.as_str());
    assert_eq!(
        validate_and_extract(Some(""), "a", Element),
        Ok((None, None, Atom::new("a")))
    );
    assert_eq!(
        validate_and_extract(Some("urn:x"), "p:a:b", Attribute),
        Ok((
            Some(Atom::new("urn:x")),
            Some(Atom::new("p")),
            Atom::new("a:b")
        ))
    );
    assert_eq!(
        validate_and_extract(None, "p:a", Element),
        Err(NamespaceError)
    );
    assert_eq!(
        validate_and_extract(Some("urn:x"), ":a", Element),
        Err(InvalidCharacterError)
    );
    assert_eq!(
        validate_and_extract(None, "1a", Element),
        Err(InvalidCharacterError)
    );
    assert_eq!(
        validate_and_extract(None, "a=", Attribute),
        Err(InvalidCharacterError)
    );
    assert_eq!(
        validate_and_extract(Some("urn:x"), "xml:a", Attribute),
        Err(NamespaceError)
    );
    assert!(validate_and_extract(xml, "xml:lang", Attribute).is_ok());
    assert_eq!(
        validate_and_extract(None, "xmlns", Attribute),
        Err(NamespaceError)
    );
    assert!(validate_and_extract(xmlns, "xmlns", Attribute).is_ok());
    assert!(validate_and_extract(xmlns, "xmlns:x", Attribute).is_ok());
    assert_eq!(
        validate_and_extract(xmlns, "x", Attribute),
        Err(NamespaceError)
    );

    assert!(is_valid_element_local_name("my-element"));
    assert!(is_valid_element_local_name("a\u{1F600}"));
    assert!(is_valid_element_local_name("_x.y"));
    assert!(is_valid_element_local_name("\u{e9}t\u{e9}"));
    assert!(!is_valid_element_local_name(""));
    assert!(!is_valid_element_local_name("a b"));
    assert!(!is_valid_element_local_name("-a"));
    assert!(!is_valid_element_local_name("_a!"));
    assert!(is_valid_attribute_local_name("@click"));
    assert!(!is_valid_attribute_local_name("a/b"));
    assert!(is_valid_namespace_prefix("p"));
    assert!(!is_valid_namespace_prefix(""));
    assert!(is_valid_doctype_name(""));
    assert!(!is_valid_doctype_name("a>"));
}

#[test]
fn character_data() {
    let (mut doc, root) = html_doc();
    let t = doc.create_text(root, "a");
    doc.append_data(t, "bc");
    assert_eq!(doc.node(t).character_data(), Some("abc"));
    doc.set_data(t, "x");
    assert_eq!(doc.node(t).character_data(), Some("x"));
    let pi = doc.create_processing_instruction(root, "xml-stylesheet", "d");
    doc.append_data(pi, "e");
    assert_eq!(doc.node(pi).character_data(), Some("de"));
}

#[test]
fn document_title() {
    let (mut doc, root) = html_doc();
    assert_eq!(doc.title(), "");
    let html = el(&mut doc, "html");
    let head = el(&mut doc, "head");
    let title = el(&mut doc, "title");
    let t = doc.create_text(root, "  hello \n\t world ");
    let t2 = doc.create_text(root, "again");
    let second = el(&mut doc, "title");
    let second_text = doc.create_text(root, "second");
    doc.append(html, root).unwrap();
    doc.append(head, html).unwrap();
    doc.append(title, head).unwrap();
    doc.append(t, title).unwrap();
    doc.append(t2, title).unwrap();
    doc.append(second, head).unwrap();
    doc.append(second_text, second).unwrap();
    assert_eq!(doc.title(), "hello world again");

    let (mut svg_doc, svg_root) = html_doc();
    let svg = svg_doc.create_element(svg_root, atoms::SVG, Some(ns::SVG), None);
    let g = svg_doc.create_element(svg_root, Atom::new("g"), Some(ns::SVG), None);
    let nested = svg_doc.create_element(svg_root, atoms::TITLE, Some(ns::SVG), None);
    let nested_text = svg_doc.create_text(svg_root, "nested");
    svg_doc.append(svg, svg_root).unwrap();
    svg_doc.append(g, svg).unwrap();
    svg_doc.append(nested, g).unwrap();
    svg_doc.append(nested_text, nested).unwrap();
    assert_eq!(svg_doc.title(), "");
    let title = svg_doc.create_element(svg_root, atoms::TITLE, Some(ns::SVG), None);
    let text = svg_doc.create_text(svg_root, " svg ");
    svg_doc.append(title, svg).unwrap();
    svg_doc.append(text, title).unwrap();
    assert_eq!(svg_doc.title(), "svg");
}

#[test]
fn quirks_mode_and_kind() {
    let (mut doc, _) = html_doc();
    assert_eq!(doc.quirks_mode(), QuirksMode::NoQuirks);
    doc.set_quirks_mode(QuirksMode::LimitedQuirks);
    assert_eq!(doc.quirks_mode(), QuirksMode::LimitedQuirks);
    assert_eq!(doc.kind(), DocumentKind::Html);
    assert!(doc.is_html_document(doc.document_node()));
    assert_eq!(Document::new(DocumentKind::Xml).kind(), DocumentKind::Xml);
}

#[test]
fn element_accessors() {
    let (mut doc, root) = html_doc();
    let e = doc.create_element(root, Atom::new("rect"), Some(ns::SVG), Some(Atom::new("s")));
    let element = doc.element(e).unwrap();
    assert_eq!(element.qualified_name(), "s:rect");
    assert!(element.is_in_namespace(ns::SVG));
    assert!(!element.is_html(Atom::new("rect")));
    assert_eq!(element.prefix(), Some(Atom::new("s")));
    let p = el(&mut doc, "p");
    assert!(doc.element(p).unwrap().is_html(atoms::P));
    match doc.element(p).unwrap().local_name() {
        atoms::P => {}
        other => panic!("{other}"),
    }
}

#[test]
fn deep_trees_do_not_recurse() {
    let (mut doc, root) = html_doc();
    let mut parent = el(&mut doc, "html");
    doc.append(parent, root).unwrap();
    for _ in 0..100_000 {
        let child = el(&mut doc, "div");
        doc.insert(child, parent, None, false);
        parent = child;
    }
    assert_eq!(doc.descendants(root).count(), 100_001);
    let copy = doc.clone_document(true);
    assert_eq!(copy.descendants(copy.document_node()).count(), 100_001);
}
