//! the whatwg url standard: <https://url.spec.whatwg.org/>

pub mod api;
mod host;
mod idna;
mod origin;
mod parser;
pub mod percent;
mod punycode;
pub mod urlencoded;

use std::fmt;

pub use host::Host;
pub use idna::domain_to_unicode;
pub use origin::{OpaqueOrigin, Origin};
pub use parser::State;

/// the url parser returned failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError;

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid url")
    }
}

impl std::error::Error for ParseError {}

/// <https://url.spec.whatwg.org/#concept-url-path>
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum UrlPath {
    /// a url path segment: the url has an opaque path.
    Opaque(String),
    /// a list of zero or more url path segments.
    List(Vec<String>),
}

/// a url record: <https://url.spec.whatwg.org/#concept-url>
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Url {
    pub(crate) scheme: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) host: Option<Host>,
    pub(crate) port: Option<u16>,
    pub(crate) path: UrlPath,
    pub(crate) query: Option<String>,
    pub(crate) fragment: Option<String>,
}

impl Default for Url {
    fn default() -> Self {
        Url {
            scheme: String::new(),
            username: String::new(),
            password: String::new(),
            host: None,
            port: None,
            path: UrlPath::List(Vec::new()),
            query: None,
            fragment: None,
        }
    }
}

/// <https://url.spec.whatwg.org/#special-scheme>
pub fn is_special_scheme(scheme: &str) -> bool {
    matches!(scheme, "ftp" | "file" | "http" | "https" | "ws" | "wss")
}

/// <https://url.spec.whatwg.org/#default-port>
pub fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "ftp" => Some(21),
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    }
}

impl Url {
    /// the url parser: <https://url.spec.whatwg.org/#concept-url-parser>
    ///
    /// blob url entries live outside this crate, so this is the basic url
    /// parser without the blob url entry lookup.
    pub fn parse(input: &str, base: Option<&Url>) -> Result<Url, ParseError> {
        let mut url = Url::default();
        parser::basic_url_parse(input, base, &mut url, None)?;
        Ok(url)
    }

    /// the basic url parser with a url and state override, as the api
    /// setters use it: <https://url.spec.whatwg.org/#concept-basic-url-parser>
    pub fn parse_with_state_override(
        &mut self,
        input: &str,
        state_override: State,
    ) -> Result<(), ParseError> {
        parser::basic_url_parse(input, None, self, Some(state_override))
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    pub fn host(&self) -> Option<&Host> {
        self.host.as_ref()
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// the port, or the scheme's default port when the port is null.
    pub fn port_or_default(&self) -> Option<u16> {
        self.port.or_else(|| default_port(&self.scheme))
    }

    pub fn path(&self) -> &UrlPath {
        &self.path
    }

    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub fn fragment(&self) -> Option<&str> {
        self.fragment.as_deref()
    }

    /// <https://url.spec.whatwg.org/#is-special>
    pub fn is_special(&self) -> bool {
        is_special_scheme(&self.scheme)
    }

    /// <https://url.spec.whatwg.org/#include-credentials>
    pub fn includes_credentials(&self) -> bool {
        !self.username.is_empty() || !self.password.is_empty()
    }

    /// <https://url.spec.whatwg.org/#url-opaque-path>
    pub fn has_opaque_path(&self) -> bool {
        matches!(self.path, UrlPath::Opaque(_))
    }

    /// <https://url.spec.whatwg.org/#cannot-have-a-username-password-port>
    pub fn cannot_have_username_password_port(&self) -> bool {
        matches!(self.host, None | Some(Host::Empty)) || self.scheme == "file"
    }

    /// <https://url.spec.whatwg.org/#set-the-username>
    pub fn set_username(&mut self, username: &str) {
        self.username = percent::utf8_percent_encode(username, percent::EncodeSet::Userinfo);
    }

    /// <https://url.spec.whatwg.org/#set-the-password>
    pub fn set_password(&mut self, password: &str) {
        self.password = percent::utf8_percent_encode(password, percent::EncodeSet::Userinfo);
    }

    pub fn set_port(&mut self, port: Option<u16>) {
        self.port = port;
    }

    pub fn set_query(&mut self, query: Option<String>) {
        self.query = query;
    }

    pub fn set_fragment(&mut self, fragment: Option<String>) {
        self.fragment = fragment;
    }

    /// <https://url.spec.whatwg.org/#shorten-a-urls-path>
    pub(crate) fn shorten_path(&mut self) {
        let is_file = self.scheme == "file";
        // step 1: assert url does not have an opaque path.
        let UrlPath::List(path) = &mut self.path else {
            return;
        };
        // step 3
        if is_file && path.len() == 1 && is_normalized_windows_drive_letter(&path[0]) {
            return;
        }
        // step 4
        path.pop();
    }

    /// <https://url.spec.whatwg.org/#concept-url-serializer>
    pub fn serialize(&self, exclude_fragment: bool) -> String {
        let mut output = String::new();
        self.serialize_into(&mut output, false, exclude_fragment);
        output
    }

    /// the url serializer with exclude scheme set to true.
    pub fn serialize_without_scheme(&self, exclude_fragment: bool) -> String {
        let mut output = String::new();
        self.serialize_into(&mut output, true, exclude_fragment);
        output
    }

    fn serialize_into(&self, output: &mut String, exclude_scheme: bool, exclude_fragment: bool) {
        // step 2
        if !exclude_scheme {
            output.push_str(&self.scheme);
            output.push(':');
        }
        // step 3
        if let Some(host) = &self.host {
            output.push_str("//");
            if self.includes_credentials() {
                output.push_str(&self.username);
                if !self.password.is_empty() {
                    output.push(':');
                    output.push_str(&self.password);
                }
                output.push('@');
            }
            host.serialize_into(output);
            if let Some(port) = self.port {
                output.push(':');
                output.push_str(&port.to_string());
            }
        }
        // step 4: keeps web+demo:/.//not-a-host/ from reparsing with a host.
        if self.host.is_none()
            && let UrlPath::List(path) = &self.path
            && path.len() > 1
            && path[0].is_empty()
        {
            output.push_str("/.");
        }
        // step 5
        self.serialize_path_into(output);
        // step 6
        if let Some(query) = &self.query {
            output.push('?');
            output.push_str(query);
        }
        // step 7
        if !exclude_fragment && let Some(fragment) = &self.fragment {
            output.push('#');
            output.push_str(fragment);
        }
    }

    /// <https://url.spec.whatwg.org/#url-path-serializer>
    pub fn serialize_path(&self) -> String {
        let mut output = String::new();
        self.serialize_path_into(&mut output);
        output
    }

    fn serialize_path_into(&self, output: &mut String) {
        match &self.path {
            UrlPath::Opaque(segment) => output.push_str(segment),
            UrlPath::List(segments) => {
                for segment in segments {
                    output.push('/');
                    output.push_str(segment);
                }
            }
        }
    }

    /// <https://url.spec.whatwg.org/#concept-url-equals>
    pub fn equals(&self, other: &Url, exclude_fragments: bool) -> bool {
        self.serialize(exclude_fragments) == other.serialize(exclude_fragments)
    }

    /// <https://url.spec.whatwg.org/#concept-url-origin>
    pub fn origin(&self) -> Origin {
        origin::url_origin(self)
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialize(false))
    }
}

impl std::str::FromStr for Url {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Url, ParseError> {
        Url::parse(s, None)
    }
}

/// <https://url.spec.whatwg.org/#windows-drive-letter>
pub(crate) fn is_windows_drive_letter(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && (b[1] == b':' || b[1] == b'|')
}

/// <https://url.spec.whatwg.org/#normalized-windows-drive-letter>
pub(crate) fn is_normalized_windows_drive_letter(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}
