//! interned strings for local names, namespaces and attribute names.
//!
//! an `Atom` is an index into one process-wide table, so comparing two atoms
//! is one integer compare and atoms can be shared between documents,
//! stylesheets and threads. interned strings are never freed.

use std::collections::HashMap;
use std::fmt;
use std::sync::{OnceLock, PoisonError, RwLock};

/// an interned string.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Atom(u32);

macro_rules! static_atoms {
    ($($name:ident = $s:literal,)*) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(u32)]
        enum StaticIndex { $($name,)* }

        static STATIC_ATOMS: &[&str] = &[$($s,)*];

        /// atoms interned before anything else, usable as `match` patterns.
        pub mod atoms {
            use super::{Atom, StaticIndex};
            $(pub const $name: Atom = Atom(StaticIndex::$name as u32);)*
        }
    };
}

static_atoms! {
    EMPTY = "",
    NS_HTML = "http://www.w3.org/1999/xhtml",
    NS_MATHML = "http://www.w3.org/1998/Math/MathML",
    NS_SVG = "http://www.w3.org/2000/svg",
    NS_XLINK = "http://www.w3.org/1999/xlink",
    NS_XML = "http://www.w3.org/XML/1998/namespace",
    NS_XMLNS = "http://www.w3.org/2000/xmlns/",
    // html elements
    A = "a", ABBR = "abbr", ADDRESS = "address", APPLET = "applet", AREA = "area",
    ARTICLE = "article", ASIDE = "aside", AUDIO = "audio", B = "b", BASE = "base",
    BASEFONT = "basefont", BDI = "bdi", BDO = "bdo", BGSOUND = "bgsound", BIG = "big",
    BLOCKQUOTE = "blockquote", BODY = "body", BR = "br", BUTTON = "button",
    CANVAS = "canvas", CAPTION = "caption", CENTER = "center", CITE = "cite",
    CODE = "code", COL = "col", COLGROUP = "colgroup", DATA = "data",
    DATALIST = "datalist", DD = "dd", DEL = "del", DETAILS = "details", DFN = "dfn",
    DIALOG = "dialog", DIR = "dir", DIV = "div", DL = "dl", DT = "dt", EM = "em",
    EMBED = "embed", FIELDSET = "fieldset", FIGCAPTION = "figcaption",
    FIGURE = "figure", FONT = "font", FOOTER = "footer", FORM = "form",
    FRAME = "frame", FRAMESET = "frameset", H1 = "h1", H2 = "h2", H3 = "h3",
    H4 = "h4", H5 = "h5", H6 = "h6", HEAD = "head", HEADER = "header",
    HGROUP = "hgroup", HR = "hr", HTML = "html", I = "i", IFRAME = "iframe",
    IMAGE = "image", IMG = "img", INPUT = "input", INS = "ins", KBD = "kbd",
    KEYGEN = "keygen", LABEL = "label", LEGEND = "legend", LI = "li", LINK = "link",
    LISTING = "listing", MAIN = "main", MAP = "map", MARK = "mark",
    MARQUEE = "marquee", MENU = "menu", META = "meta", METER = "meter", NAV = "nav",
    NOBR = "nobr", NOEMBED = "noembed", NOFRAMES = "noframes",
    NOSCRIPT = "noscript", OBJECT = "object", OL = "ol", OPTGROUP = "optgroup",
    OPTION = "option", OUTPUT = "output", P = "p", PARAM = "param",
    PICTURE = "picture", PLAINTEXT = "plaintext", PRE = "pre",
    PROGRESS = "progress", Q = "q", RB = "rb", RP = "rp", RT = "rt", RTC = "rtc",
    RUBY = "ruby", S = "s", SAMP = "samp", SCRIPT = "script", SEARCH = "search",
    SECTION = "section", SELECT = "select", SLOT = "slot", SMALL = "small",
    SOURCE = "source", SPAN = "span", STRIKE = "strike", STRONG = "strong",
    STYLE = "style", SUB = "sub", SUMMARY = "summary", SUP = "sup", TABLE = "table",
    TBODY = "tbody", TD = "td", TEMPLATE = "template", TEXTAREA = "textarea",
    TFOOT = "tfoot", TH = "th", THEAD = "thead", TIME = "time", TITLE = "title",
    TR = "tr", TRACK = "track", TT = "tt", U = "u", UL = "ul", VAR = "var",
    VIDEO = "video", WBR = "wbr", XMP = "xmp",
    // svg and mathml elements the tree builder names
    SVG = "svg", MATH = "math", FOREIGN_OBJECT = "foreignObject", DESC = "desc",
    MI = "mi", MO = "mo", MN = "mn", MS = "ms", MTEXT = "mtext",
    ANNOTATION_XML = "annotation-xml", MGLYPH = "mglyph",
    MALIGNMARK = "malignmark",
    // attributes
    ID = "id", CLASS = "class", HREF = "href", SRC = "src", TYPE = "type",
    NAME = "name", VALUE = "value", REL = "rel", LANG = "lang", ALT = "alt",
    WIDTH = "width", HEIGHT = "height", CHARSET = "charset", CONTENT = "content",
    HTTP_EQUIV = "http-equiv", MEDIA = "media", COLSPAN = "colspan",
    ROWSPAN = "rowspan", START = "start", REVERSED = "reversed", HIDDEN = "hidden",
    DISABLED = "disabled", CHECKED = "checked", SELECTED = "selected",
    MULTIPLE = "multiple", ACTION = "action", METHOD = "method", FOR = "for",
    COLOR = "color", FACE = "face", SIZE = "size", ENCODING = "encoding",
    DEFINITION_URL = "definitionURL", XMLNS = "xmlns", XLINK = "xlink",
    ALIGN = "align", VALIGN = "valign", BGCOLOR = "bgcolor",
    BACKGROUND = "background", BORDER = "border", CELLPADDING = "cellpadding",
    CELLSPACING = "cellspacing", NOWRAP = "nowrap", TEXT = "text",
    SRCSET = "srcset", SIZES = "sizes", TARGET = "target", TABINDEX = "tabindex",
    ROLE = "role", OPEN = "open", COLS = "cols", ROWS = "rows", ASYNC = "async",
    DEFER = "defer", PROMPT = "prompt", SHADOWROOTMODE = "shadowrootmode",
}

/// well-known namespaces (https://infra.spec.whatwg.org/#namespaces).
pub mod ns {
    use super::{Atom, atoms};
    pub const HTML: Atom = atoms::NS_HTML;
    pub const MATHML: Atom = atoms::NS_MATHML;
    pub const SVG: Atom = atoms::NS_SVG;
    pub const XLINK: Atom = atoms::NS_XLINK;
    pub const XML: Atom = atoms::NS_XML;
    pub const XMLNS: Atom = atoms::NS_XMLNS;
}

struct Table {
    dynamic: Vec<&'static str>,
    map: HashMap<&'static str, Atom>,
}

fn table() -> &'static RwLock<Table> {
    static TABLE: OnceLock<RwLock<Table>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let map = (0u32..)
            .zip(STATIC_ATOMS)
            .map(|(i, &s)| (s, Atom(i)))
            .collect();
        RwLock::new(Table {
            dynamic: Vec::new(),
            map,
        })
    })
}

impl Atom {
    /// interns `s`, returning the same atom for equal strings.
    pub fn new(s: &str) -> Atom {
        if let Some(&atom) = table()
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .map
            .get(s)
        {
            return atom;
        }
        let mut table = table().write().unwrap_or_else(PoisonError::into_inner);
        if let Some(&atom) = table.map.get(s) {
            return atom;
        }
        let index = STATIC_ATOMS.len() + table.dynamic.len();
        let atom = Atom(u32::try_from(index).expect("fewer than 2^32 distinct atoms"));
        let leaked: &'static str = Box::leak(s.into());
        table.dynamic.push(leaked);
        table.map.insert(leaked, atom);
        atom
    }

    pub fn as_str(self) -> &'static str {
        let index = self.0 as usize;
        match STATIC_ATOMS.get(index) {
            Some(s) => s,
            None => {
                table()
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .dynamic[index - STATIC_ATOMS.len()]
            }
        }
    }
}

impl From<&str> for Atom {
    fn from(s: &str) -> Atom {
        Atom::new(s)
    }
}

impl PartialEq<str> for Atom {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Atom {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Debug for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn static_atoms_are_distinct() {
        let mut seen = HashSet::new();
        for s in STATIC_ATOMS {
            assert!(seen.insert(s), "duplicate static atom {s:?}");
        }
    }

    #[test]
    fn interning_is_stable() {
        assert_eq!(Atom::new("div"), atoms::DIV);
        assert_eq!(Atom::new("foreignObject"), atoms::FOREIGN_OBJECT);
        let a = Atom::new("x-custom-element");
        assert_eq!(a, Atom::new("x-custom-element"));
        assert_eq!(a.as_str(), "x-custom-element");
        assert_ne!(a, Atom::new("x-custom-elemenT"));
        assert_eq!(ns::HTML, "http://www.w3.org/1999/xhtml");
    }
}
