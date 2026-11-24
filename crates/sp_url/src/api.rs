//! the getters and setters of the URL class, as rust functions on a url
//! record: <https://url.spec.whatwg.org/#api>
//!
//! the URLSearchParams query object belongs to the js bindings, which
//! update it after the href and search setters.

use crate::{ParseError, State, Url, UrlPath};

/// <https://url.spec.whatwg.org/#api-url-parser>
pub fn api_url_parse(url: &str, base: Option<&str>) -> Result<Url, ParseError> {
    // steps 1 and 2
    let parsed_base = match base {
        Some(base) => Some(Url::parse(base, None)?),
        None => None,
    };
    // step 3
    Url::parse(url, parsed_base.as_ref())
}

/// <https://url.spec.whatwg.org/#dom-url-href>
pub fn href(url: &Url) -> String {
    url.serialize(false)
}

/// <https://url.spec.whatwg.org/#dom-url-href>
pub fn set_href(url: &mut Url, value: &str) -> Result<(), ParseError> {
    // steps 1 and 2
    let parsed = Url::parse(value, None)?;
    // step 3
    *url = parsed;
    Ok(())
}

/// <https://url.spec.whatwg.org/#dom-url-origin>
pub fn origin(url: &Url) -> String {
    url.origin().serialize()
}

/// <https://url.spec.whatwg.org/#dom-url-protocol>
pub fn protocol(url: &Url) -> String {
    format!("{}:", url.scheme)
}

/// <https://url.spec.whatwg.org/#dom-url-protocol>
pub fn set_protocol(url: &mut Url, value: &str) {
    // failure leaves the url as it was.
    let _ = url.parse_with_state_override(&format!("{value}:"), State::SchemeStart);
}

/// <https://url.spec.whatwg.org/#dom-url-username>
pub fn username(url: &Url) -> String {
    url.username.clone()
}

/// <https://url.spec.whatwg.org/#dom-url-username>
pub fn set_username(url: &mut Url, value: &str) {
    // step 1
    if url.cannot_have_username_password_port() {
        return;
    }
    // step 2
    url.set_username(value);
}

/// <https://url.spec.whatwg.org/#dom-url-password>
pub fn password(url: &Url) -> String {
    url.password.clone()
}

/// <https://url.spec.whatwg.org/#dom-url-password>
pub fn set_password(url: &mut Url, value: &str) {
    // step 1
    if url.cannot_have_username_password_port() {
        return;
    }
    // step 2
    url.set_password(value);
}

/// <https://url.spec.whatwg.org/#dom-url-host>
pub fn host(url: &Url) -> String {
    // steps 1 and 2
    let Some(host) = &url.host else {
        return String::new();
    };
    // step 3
    let Some(port) = url.port else {
        return host.serialize();
    };
    // step 4
    format!("{}:{port}", host.serialize())
}

/// <https://url.spec.whatwg.org/#dom-url-host>
pub fn set_host(url: &mut Url, value: &str) {
    // step 1
    if url.has_opaque_path() {
        return;
    }
    // step 2
    let _ = url.parse_with_state_override(value, State::Host);
}

/// <https://url.spec.whatwg.org/#dom-url-hostname>
pub fn hostname(url: &Url) -> String {
    url.host.as_ref().map(|h| h.serialize()).unwrap_or_default()
}

/// <https://url.spec.whatwg.org/#dom-url-hostname>
pub fn set_hostname(url: &mut Url, value: &str) {
    // step 1
    if url.has_opaque_path() {
        return;
    }
    // step 2
    let _ = url.parse_with_state_override(value, State::Hostname);
}

/// <https://url.spec.whatwg.org/#dom-url-port>
pub fn port(url: &Url) -> String {
    url.port.map(|p| p.to_string()).unwrap_or_default()
}

/// <https://url.spec.whatwg.org/#dom-url-port>
pub fn set_port(url: &mut Url, value: &str) {
    // step 1
    if url.cannot_have_username_password_port() {
        return;
    }
    if value.is_empty() {
        // step 2
        url.port = None;
    } else {
        // step 3
        let _ = url.parse_with_state_override(value, State::Port);
    }
}

/// <https://url.spec.whatwg.org/#dom-url-pathname>
pub fn pathname(url: &Url) -> String {
    url.serialize_path()
}

/// <https://url.spec.whatwg.org/#dom-url-pathname>
pub fn set_pathname(url: &mut Url, value: &str) {
    // step 1
    if url.has_opaque_path() {
        return;
    }
    // step 2
    url.path = UrlPath::List(Vec::new());
    // step 3
    let _ = url.parse_with_state_override(value, State::PathStart);
}

/// <https://url.spec.whatwg.org/#dom-url-search>
pub fn search(url: &Url) -> String {
    match url.query.as_deref() {
        // step 1
        None | Some("") => String::new(),
        // step 2
        Some(query) => format!("?{query}"),
    }
}

/// <https://url.spec.whatwg.org/#dom-url-search>
pub fn set_search(url: &mut Url, value: &str) {
    // step 2
    if value.is_empty() {
        url.query = None;
        return;
    }
    // step 3
    let input = value.strip_prefix('?').unwrap_or(value);
    // step 4
    url.query = Some(String::new());
    // step 5
    let _ = url.parse_with_state_override(input, State::Query);
}

/// <https://url.spec.whatwg.org/#dom-url-hash>
pub fn hash(url: &Url) -> String {
    match url.fragment.as_deref() {
        // step 1
        None | Some("") => String::new(),
        // step 2
        Some(fragment) => format!("#{fragment}"),
    }
}

/// <https://url.spec.whatwg.org/#dom-url-hash>
pub fn set_hash(url: &mut Url, value: &str) {
    // step 1
    if value.is_empty() {
        url.fragment = None;
        return;
    }
    // step 2
    let input = value.strip_prefix('#').unwrap_or(value);
    // step 3
    url.fragment = Some(String::new());
    // step 4
    let _ = url.parse_with_state_override(input, State::Fragment);
}
