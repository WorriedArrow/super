//! <https://url.spec.whatwg.org/#origin> and the origin concept from
//! <https://html.spec.whatwg.org/multipage/browsers.html#concept-origin>

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Host, Url};

/// an opaque origin. each one made by [`OpaqueOrigin::new`] is distinct and
/// only equal to its own copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OpaqueOrigin(u64);

impl OpaqueOrigin {
    /// <https://html.spec.whatwg.org/multipage/browsers.html#concept-origin-opaque>
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        OpaqueOrigin(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// <https://html.spec.whatwg.org/multipage/browsers.html#concept-origin>
///
/// the tuple origin's domain is html state and isn't tracked here.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    Opaque(OpaqueOrigin),
    Tuple {
        scheme: String,
        host: Host,
        port: Option<u16>,
    },
}

impl Origin {
    pub fn new_opaque() -> Self {
        Origin::Opaque(OpaqueOrigin::new())
    }

    pub fn is_opaque(&self) -> bool {
        matches!(self, Origin::Opaque(_))
    }

    /// <https://html.spec.whatwg.org/multipage/browsers.html#ascii-serialisation-of-an-origin>
    pub fn serialize(&self) -> String {
        match self {
            // step 1
            Origin::Opaque(_) => "null".to_owned(),
            Origin::Tuple { scheme, host, port } => {
                // steps 2 to 4
                let mut result = format!("{scheme}://{}", host.serialize());
                // step 5
                if let Some(port) = port {
                    result.push(':');
                    result.push_str(&port.to_string());
                }
                // step 6
                result
            }
        }
    }
}

/// <https://url.spec.whatwg.org/#concept-url-origin>
pub(crate) fn url_origin(url: &Url) -> Origin {
    match url.scheme.as_str() {
        "blob" => {
            // step 1: blob url entries live outside this crate.
            // step 2
            let Ok(path_url) = Url::parse(&url.serialize_path(), None) else {
                // step 3
                return Origin::new_opaque();
            };
            // step 4
            if matches!(path_url.scheme.as_str(), "http" | "https" | "file") {
                return path_url.origin();
            }
            // step 5
            Origin::new_opaque()
        }
        "ftp" | "http" | "https" | "ws" | "wss" => Origin::Tuple {
            scheme: url.scheme.clone(),
            host: url.host.clone().unwrap_or(Host::Empty),
            port: url.port,
        },
        // "file" is left as an exercise to the reader; the spec says to
        // return a new opaque origin when in doubt.
        _ => Origin::new_opaque(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(s: &str) -> String {
        Url::parse(s, None).expect("parses").origin().serialize()
    }

    #[test]
    fn tuple() {
        assert_eq!(
            origin("https://example.org:8443/x"),
            "https://example.org:8443"
        );
        assert_eq!(origin("http://[::1]:80/"), "http://[::1]");
        assert_eq!(
            origin("blob:https://whatwg.org/d0360e2f-caee-469f-9a2f-87d5b0456f6f"),
            "https://whatwg.org"
        );
    }

    #[test]
    fn opaque() {
        assert_eq!(origin("data:text/plain,x"), "null");
        assert_eq!(origin("blob:ftp://x/"), "null");
        assert_eq!(origin("file:///x"), "null");
        let url = Url::parse("about:blank", None).expect("parses");
        assert_ne!(url.origin(), url.origin());
        let o = Origin::new_opaque();
        assert_eq!(o.clone(), o);
    }
}
