//! the basic url parser: <https://url.spec.whatwg.org/#concept-basic-url-parser>

use crate::host::{Host, host_parse};
use crate::percent::{EncodeSet, utf8_percent_encode_into};
use crate::{
    ParseError, Url, UrlPath, default_port, is_normalized_windows_drive_letter, is_special_scheme,
    is_windows_drive_letter,
};

/// the parser states: <https://url.spec.whatwg.org/#scheme-start-state> and
/// following.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    SchemeStart,
    Scheme,
    NoScheme,
    SpecialRelativeOrAuthority,
    PathOrAuthority,
    Relative,
    RelativeSlash,
    SpecialAuthoritySlashes,
    SpecialAuthorityIgnoreSlashes,
    Authority,
    Host,
    Hostname,
    Port,
    File,
    FileSlash,
    FileHost,
    PathStart,
    Path,
    OpaquePath,
    Query,
    Fragment,
}

/// <https://url.spec.whatwg.org/#c0-control>, plus u+0020 space.
fn is_c0_control_or_space(c: char) -> bool {
    c <= ' '
}

/// <https://url.spec.whatwg.org/#single-dot-path-segment>
fn is_single_dot_segment(s: &str) -> bool {
    s == "." || s.eq_ignore_ascii_case("%2e")
}

/// <https://url.spec.whatwg.org/#double-dot-path-segment>
fn is_double_dot_segment(s: &str) -> bool {
    matches!(s.len(), 2 | 4 | 6)
        && (s == ".."
            || s.eq_ignore_ascii_case(".%2e")
            || s.eq_ignore_ascii_case("%2e.")
            || s.eq_ignore_ascii_case("%2e%2e"))
}

/// <https://url.spec.whatwg.org/#start-with-a-windows-drive-letter>
fn starts_with_windows_drive_letter(s: &[char]) -> bool {
    s.len() >= 2
        && s[0].is_ascii_alphabetic()
        && matches!(s[1], ':' | '|')
        && (s.len() == 2 || matches!(s[2], '/' | '\\' | '?' | '#'))
}

/// appends a segment to url's path.
fn append_segment(url: &mut Url, segment: String) {
    match &mut url.path {
        UrlPath::List(list) => list.push(segment),
        // the states that append segments never see an opaque path: one is
        // only set right before the opaque path state, and the setters that
        // parse a path return early on urls that have one.
        UrlPath::Opaque(_) => url.path = UrlPath::List(vec![segment]),
    }
}

/// runs the basic url parser on `input`, modifying `url`.
///
/// without a state override, `url` must be a new url and the caller returns
/// it on success. with one, this is the setter form, and `Ok` covers both
/// the algorithm's "return" and running to the end.
pub(crate) fn basic_url_parse(
    input: &str,
    base: Option<&Url>,
    url: &mut Url,
    state_override: Option<State>,
) -> Result<(), ParseError> {
    // step 1: url is "not given" exactly when there's no state override.
    let input = if state_override.is_none() {
        // steps 1.2 and 1.3
        input.trim_matches(is_c0_control_or_space)
    } else {
        input
    };
    // steps 2 and 3
    let input: Vec<char> = input
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    // step 4
    let mut state = state_override.unwrap_or(State::SchemeStart);
    // step 5: only utf-8 is supported, see percent_encode_query.
    // step 6
    let mut buffer = String::new();
    // step 7
    let mut at_sign_seen = false;
    let mut inside_brackets = false;
    let mut password_token_seen = false;
    // step 8: the pointer can step back one before the start of input.
    let len = input.len() as isize;
    let mut pointer: isize = 0;
    let at = |p: isize| -> Option<char> {
        if p >= 0 && p < len {
            Some(input[p as usize])
        } else {
            None
        }
    };
    let remaining = |p: isize| -> &[char] {
        let start = (p + 1).clamp(0, len) as usize;
        &input[start..]
    };

    // step 9
    loop {
        let c = at(pointer);
        match state {
            // https://url.spec.whatwg.org/#scheme-start-state
            State::SchemeStart => {
                if let Some(c) = c.filter(char::is_ascii_alphabetic) {
                    // step 1
                    buffer.push(c.to_ascii_lowercase());
                    state = State::Scheme;
                } else if state_override.is_none() {
                    // step 2
                    state = State::NoScheme;
                    pointer -= 1;
                } else {
                    // step 3
                    return Err(ParseError);
                }
            }
            // https://url.spec.whatwg.org/#scheme-state
            State::Scheme => {
                if let Some(c) =
                    c.filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                {
                    // step 1
                    buffer.push(c.to_ascii_lowercase());
                } else if c == Some(':') {
                    // step 2.1
                    if state_override.is_some() {
                        let url_special = url.is_special();
                        let buffer_special = is_special_scheme(&buffer);
                        // steps 2.1.1 and 2.1.2
                        if url_special != buffer_special {
                            return Ok(());
                        }
                        // step 2.1.3
                        if (url.includes_credentials() || url.port.is_some()) && buffer == "file" {
                            return Ok(());
                        }
                        // step 2.1.4
                        if url.scheme == "file" && url.host == Some(Host::Empty) {
                            return Ok(());
                        }
                    }
                    // step 2.2
                    url.scheme = std::mem::take(&mut buffer);
                    // step 2.3
                    if state_override.is_some() {
                        // step 2.3.1
                        if url.port.is_some() && url.port == default_port(&url.scheme) {
                            url.port = None;
                        }
                        // step 2.3.2
                        return Ok(());
                    }
                    // step 2.4: buffer was taken above.
                    if url.scheme == "file" {
                        // step 2.5
                        state = State::File;
                    } else if let Some(base) =
                        base.filter(|b| url.is_special() && b.scheme == url.scheme)
                    {
                        // step 2.6: base is special, so it doesn't have an
                        // opaque path.
                        debug_assert!(base.is_special());
                        state = State::SpecialRelativeOrAuthority;
                    } else if url.is_special() {
                        // step 2.7
                        state = State::SpecialAuthoritySlashes;
                    } else if remaining(pointer).first() == Some(&'/') {
                        // step 2.8
                        state = State::PathOrAuthority;
                        pointer += 1;
                    } else {
                        // step 2.9
                        url.path = UrlPath::Opaque(String::new());
                        state = State::OpaquePath;
                    }
                } else if state_override.is_none() {
                    // step 3
                    buffer.clear();
                    state = State::NoScheme;
                    pointer = -1;
                } else {
                    // step 4
                    return Err(ParseError);
                }
            }
            // https://url.spec.whatwg.org/#no-scheme-state
            State::NoScheme => {
                let Some(base) = base else {
                    // step 1
                    return Err(ParseError);
                };
                if base.has_opaque_path() {
                    if c != Some('#') {
                        // step 1
                        return Err(ParseError);
                    }
                    // step 2
                    url.scheme = base.scheme.clone();
                    url.path = base.path.clone();
                    url.query = base.query.clone();
                    url.fragment = Some(String::new());
                    state = State::Fragment;
                } else if base.scheme != "file" {
                    // step 3
                    state = State::Relative;
                    pointer -= 1;
                } else {
                    // step 4
                    state = State::File;
                    pointer -= 1;
                }
            }
            // https://url.spec.whatwg.org/#special-relative-or-authority-state
            State::SpecialRelativeOrAuthority => {
                if c == Some('/') && remaining(pointer).first() == Some(&'/') {
                    // step 1
                    state = State::SpecialAuthorityIgnoreSlashes;
                    pointer += 1;
                } else {
                    // step 2
                    state = State::Relative;
                    pointer -= 1;
                }
            }
            // https://url.spec.whatwg.org/#path-or-authority-state
            State::PathOrAuthority => {
                if c == Some('/') {
                    state = State::Authority;
                } else {
                    state = State::Path;
                    pointer -= 1;
                }
            }
            // https://url.spec.whatwg.org/#relative-state
            State::Relative => {
                // the no scheme and special relative or authority states only
                // get here with a base.
                let Some(base) = base else {
                    return Err(ParseError);
                };
                // steps 1 and 2
                url.scheme = base.scheme.clone();
                if c == Some('/') {
                    // step 3
                    state = State::RelativeSlash;
                } else if url.is_special() && c == Some('\\') {
                    // step 4
                    state = State::RelativeSlash;
                } else {
                    // step 5.1
                    url.username = base.username.clone();
                    url.password = base.password.clone();
                    url.host = base.host.clone();
                    url.port = base.port;
                    url.path = base.path.clone();
                    url.query = base.query.clone();
                    if c == Some('?') {
                        // step 5.2
                        url.query = Some(String::new());
                        state = State::Query;
                    } else if c == Some('#') {
                        // step 5.3
                        url.fragment = Some(String::new());
                        state = State::Fragment;
                    } else if c.is_some() {
                        // step 5.4
                        url.query = None;
                        url.shorten_path();
                        state = State::Path;
                        pointer -= 1;
                    }
                }
            }
            // https://url.spec.whatwg.org/#relative-slash-state
            State::RelativeSlash => {
                if url.is_special() && matches!(c, Some('/' | '\\')) {
                    // step 1
                    state = State::SpecialAuthorityIgnoreSlashes;
                } else if c == Some('/') {
                    // step 2
                    state = State::Authority;
                } else {
                    // step 3
                    let Some(base) = base else {
                        return Err(ParseError);
                    };
                    url.username = base.username.clone();
                    url.password = base.password.clone();
                    url.host = base.host.clone();
                    url.port = base.port;
                    state = State::Path;
                    pointer -= 1;
                }
            }
            // https://url.spec.whatwg.org/#special-authority-slashes-state
            State::SpecialAuthoritySlashes => {
                if c == Some('/') && remaining(pointer).first() == Some(&'/') {
                    // step 1
                    state = State::SpecialAuthorityIgnoreSlashes;
                    pointer += 1;
                } else {
                    // step 2
                    state = State::SpecialAuthorityIgnoreSlashes;
                    pointer -= 1;
                }
            }
            // https://url.spec.whatwg.org/#special-authority-ignore-slashes-state
            State::SpecialAuthorityIgnoreSlashes => {
                if !matches!(c, Some('/' | '\\')) {
                    // step 1
                    state = State::Authority;
                    pointer -= 1;
                }
                // step 2: a validation error only.
            }
            // https://url.spec.whatwg.org/#authority-state
            State::Authority => {
                if c == Some('@') {
                    // step 1.2
                    if at_sign_seen {
                        buffer.insert_str(0, "%40");
                    }
                    // step 1.3
                    at_sign_seen = true;
                    // step 1.4
                    for code_point in buffer.chars() {
                        // step 1.4.1
                        if code_point == ':' && !password_token_seen {
                            password_token_seen = true;
                            continue;
                        }
                        // steps 1.4.2 to 1.4.4
                        let mut tmp = [0u8; 4];
                        let target = if password_token_seen {
                            &mut url.password
                        } else {
                            &mut url.username
                        };
                        utf8_percent_encode_into(
                            code_point.encode_utf8(&mut tmp),
                            EncodeSet::Userinfo,
                            target,
                        );
                    }
                    // step 1.5
                    buffer.clear();
                } else if matches!(c, None | Some('/' | '?' | '#'))
                    || (url.is_special() && c == Some('\\'))
                {
                    // step 2.1
                    if at_sign_seen && buffer.is_empty() {
                        return Err(ParseError);
                    }
                    // step 2.2
                    pointer -= buffer.chars().count() as isize + 1;
                    buffer.clear();
                    state = State::Host;
                } else if let Some(c) = c {
                    // step 3
                    buffer.push(c);
                }
            }
            // https://url.spec.whatwg.org/#host-state
            State::Host | State::Hostname => {
                if state_override.is_some() && url.scheme == "file" {
                    // step 1
                    pointer -= 1;
                    state = State::FileHost;
                } else if c == Some(':') && !inside_brackets {
                    // step 2.1
                    if buffer.is_empty() {
                        return Err(ParseError);
                    }
                    // step 2.2
                    if state_override == Some(State::Hostname) {
                        return Err(ParseError);
                    }
                    // steps 2.3 and 2.4
                    let host = host_parse(&buffer, !url.is_special()).ok_or(ParseError)?;
                    // step 2.5
                    url.host = Some(host);
                    buffer.clear();
                    state = State::Port;
                } else if matches!(c, None | Some('/' | '?' | '#'))
                    || (url.is_special() && c == Some('\\'))
                {
                    // step 3
                    pointer -= 1;
                    if url.is_special() && buffer.is_empty() {
                        // step 3.1
                        return Err(ParseError);
                    } else if state_override.is_some()
                        && buffer.is_empty()
                        && (url.includes_credentials() || url.port.is_some())
                    {
                        // step 3.2
                        return Err(ParseError);
                    }
                    // steps 3.3 and 3.4
                    let host = host_parse(&buffer, !url.is_special()).ok_or(ParseError)?;
                    // step 3.5
                    url.host = Some(host);
                    buffer.clear();
                    state = State::PathStart;
                    // step 3.6
                    if state_override.is_some() {
                        return Ok(());
                    }
                } else if let Some(c) = c {
                    // step 4
                    if c == '[' {
                        inside_brackets = true;
                    }
                    if c == ']' {
                        inside_brackets = false;
                    }
                    buffer.push(c);
                }
            }
            // https://url.spec.whatwg.org/#port-state
            State::Port => {
                if let Some(c) = c.filter(char::is_ascii_digit) {
                    // step 1
                    buffer.push(c);
                } else if matches!(c, None | Some('/' | '?' | '#'))
                    || (url.is_special() && c == Some('\\'))
                    || state_override.is_some()
                {
                    // step 2
                    if !buffer.is_empty() {
                        // step 2.1.1
                        let mut port: u32 = 0;
                        for digit in buffer.bytes() {
                            port = port * 10 + u32::from(digit - b'0');
                            // step 2.1.2
                            if port > u32::from(u16::MAX) {
                                return Err(ParseError);
                            }
                        }
                        let port = port as u16;
                        // step 2.1.3
                        url.port = if default_port(&url.scheme) == Some(port) {
                            None
                        } else {
                            Some(port)
                        };
                        // step 2.1.4
                        buffer.clear();
                        // step 2.1.5
                        if state_override.is_some() {
                            return Ok(());
                        }
                    }
                    // step 2.2
                    if state_override.is_some() {
                        return Err(ParseError);
                    }
                    // step 2.3
                    state = State::PathStart;
                    pointer -= 1;
                } else {
                    // step 3
                    return Err(ParseError);
                }
            }
            // https://url.spec.whatwg.org/#file-state
            State::File => {
                // steps 1 and 2
                url.scheme = "file".to_owned();
                url.host = Some(Host::Empty);
                if matches!(c, Some('/' | '\\')) {
                    // step 3
                    state = State::FileSlash;
                } else if let Some(base) = base.filter(|b| b.scheme == "file") {
                    // step 4.1
                    url.host = base.host.clone();
                    url.path = base.path.clone();
                    url.query = base.query.clone();
                    if c == Some('?') {
                        // step 4.2
                        url.query = Some(String::new());
                        state = State::Query;
                    } else if c == Some('#') {
                        // step 4.3
                        url.fragment = Some(String::new());
                        state = State::Fragment;
                    } else if c.is_some() {
                        // step 4.4.1
                        url.query = None;
                        // step 4.4.2
                        let rest = &input[pointer as usize..];
                        if !starts_with_windows_drive_letter(rest) {
                            url.shorten_path();
                        } else {
                            url.path = UrlPath::List(Vec::new());
                        }
                        // step 4.4.3
                        state = State::Path;
                        pointer -= 1;
                    }
                } else {
                    // step 5
                    state = State::Path;
                    pointer -= 1;
                }
            }
            // https://url.spec.whatwg.org/#file-slash-state
            State::FileSlash => {
                if matches!(c, Some('/' | '\\')) {
                    // step 1
                    state = State::FileHost;
                } else {
                    // step 2.1
                    if let Some(base) = base.filter(|b| b.scheme == "file") {
                        // step 2.1.1
                        url.host = base.host.clone();
                        // step 2.1.2
                        let rest = &input[pointer.clamp(0, len) as usize..];
                        if !starts_with_windows_drive_letter(rest)
                            && let UrlPath::List(base_path) = &base.path
                            && let Some(first) = base_path.first()
                            && is_normalized_windows_drive_letter(first)
                        {
                            append_segment(url, first.clone());
                        }
                    }
                    // step 2.2
                    state = State::Path;
                    pointer -= 1;
                }
            }
            // https://url.spec.whatwg.org/#file-host-state
            State::FileHost => {
                if matches!(c, None | Some('/' | '\\' | '?' | '#')) {
                    // step 1
                    pointer -= 1;
                    if state_override.is_none() && is_windows_drive_letter(&buffer) {
                        // step 1.1: buffer is kept for the path state.
                        state = State::Path;
                    } else if buffer.is_empty() {
                        // step 1.2.1
                        url.host = Some(Host::Empty);
                        // step 1.2.2
                        if state_override.is_some() {
                            return Ok(());
                        }
                        // step 1.2.3
                        state = State::PathStart;
                    } else {
                        // steps 1.3.1 and 1.3.2
                        let mut host = host_parse(&buffer, !url.is_special()).ok_or(ParseError)?;
                        // step 1.3.3
                        if host == Host::Domain("localhost".to_owned()) {
                            host = Host::Empty;
                        }
                        // step 1.3.4
                        url.host = Some(host);
                        // step 1.3.5
                        if state_override.is_some() {
                            return Ok(());
                        }
                        // step 1.3.6
                        buffer.clear();
                        state = State::PathStart;
                    }
                } else if let Some(c) = c {
                    // step 2
                    buffer.push(c);
                }
            }
            // https://url.spec.whatwg.org/#path-start-state
            State::PathStart => {
                if url.is_special() {
                    // step 1
                    state = State::Path;
                    if !matches!(c, Some('/' | '\\')) {
                        pointer -= 1;
                    }
                } else if state_override.is_none() && c == Some('?') {
                    // step 2
                    url.query = Some(String::new());
                    state = State::Query;
                } else if state_override.is_none() && c == Some('#') {
                    // step 3
                    url.fragment = Some(String::new());
                    state = State::Fragment;
                } else if let Some(c) = c {
                    // step 4
                    state = State::Path;
                    if c != '/' {
                        pointer -= 1;
                    }
                } else if state_override.is_some() && url.host.is_none() {
                    // step 5
                    append_segment(url, String::new());
                }
            }
            // https://url.spec.whatwg.org/#path-state
            State::Path => {
                let slash = c == Some('/') || (url.is_special() && c == Some('\\'));
                if c.is_none()
                    || slash
                    || (state_override.is_none() && matches!(c, Some('?' | '#')))
                {
                    if is_double_dot_segment(&buffer) {
                        // step 1.2
                        url.shorten_path();
                        if !slash {
                            append_segment(url, String::new());
                        }
                    } else if is_single_dot_segment(&buffer) && !slash {
                        // step 1.3
                        append_segment(url, String::new());
                    } else if !is_single_dot_segment(&buffer) {
                        // step 1.4.1
                        let path_is_empty = match &url.path {
                            UrlPath::List(path) => path.is_empty(),
                            UrlPath::Opaque(path) => path.is_empty(),
                        };
                        if url.scheme == "file" && path_is_empty && is_windows_drive_letter(&buffer)
                        {
                            buffer.replace_range(1..2, ":");
                        }
                        // step 1.4.2
                        append_segment(url, std::mem::take(&mut buffer));
                    }
                    // step 1.5
                    buffer.clear();
                    if c == Some('?') {
                        // step 1.6
                        url.query = Some(String::new());
                        state = State::Query;
                    } else if c == Some('#') {
                        // step 1.7
                        url.fragment = Some(String::new());
                        state = State::Fragment;
                    }
                } else if let Some(c) = c {
                    // step 2.3
                    let mut tmp = [0u8; 4];
                    utf8_percent_encode_into(c.encode_utf8(&mut tmp), EncodeSet::Path, &mut buffer);
                }
            }
            // https://url.spec.whatwg.org/#cannot-be-a-base-url-path-state
            State::OpaquePath => {
                let UrlPath::Opaque(path) = &mut url.path else {
                    // only the scheme state enters this state, right after
                    // setting an opaque path.
                    return Err(ParseError);
                };
                match c {
                    // step 1
                    Some('?') => {
                        url.query = Some(String::new());
                        state = State::Query;
                    }
                    // step 2
                    Some('#') => {
                        url.fragment = Some(String::new());
                        state = State::Fragment;
                    }
                    // step 3
                    Some(' ') => {
                        if matches!(remaining(pointer).first(), Some('?' | '#')) {
                            path.push_str("%20");
                        } else {
                            path.push(' ');
                        }
                    }
                    // step 4
                    Some(c) => {
                        let mut tmp = [0u8; 4];
                        utf8_percent_encode_into(
                            c.encode_utf8(&mut tmp),
                            EncodeSet::C0Control,
                            path,
                        );
                    }
                    None => {}
                }
            }
            // https://url.spec.whatwg.org/#query-state
            State::Query => {
                // step 1: the encoding is always utf-8.
                if c.is_none() || (state_override.is_none() && c == Some('#')) {
                    // step 2.1
                    let set = if url.is_special() {
                        EncodeSet::SpecialQuery
                    } else {
                        EncodeSet::Query
                    };
                    // step 2.2
                    let query = url.query.get_or_insert_with(String::new);
                    utf8_percent_encode_into(&buffer, set, query);
                    // step 2.3
                    buffer.clear();
                    // step 2.4
                    if c == Some('#') {
                        url.fragment = Some(String::new());
                        state = State::Fragment;
                    }
                } else if let Some(c) = c {
                    // step 3.3
                    buffer.push(c);
                }
            }
            // https://url.spec.whatwg.org/#fragment-state
            State::Fragment => {
                if let Some(c) = c {
                    // step 1.3
                    let fragment = url.fragment.get_or_insert_with(String::new);
                    let mut tmp = [0u8; 4];
                    utf8_percent_encode_into(
                        c.encode_utf8(&mut tmp),
                        EncodeSet::Fragment,
                        fragment,
                    );
                }
            }
        }
        if pointer >= len {
            break;
        }
        pointer += 1;
    }
    // step 10
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str, base: Option<&str>) -> Option<String> {
        let base = base.map(|b| Url::parse(b, None).expect("base parses"));
        Url::parse(input, base.as_ref())
            .ok()
            .map(|u| u.serialize(false))
    }

    #[test]
    fn basics() {
        assert_eq!(
            parse("HTTP://EXAMPLE.com", None).as_deref(),
            Some("http://example.com/")
        );
        assert_eq!(
            parse("  https://a:b@c:443/d/../e?f#g \n", None).as_deref(),
            Some("https://a:b@c/e?f#g")
        );
        assert_eq!(parse("foo", None), None);
        assert_eq!(
            parse("foo", Some("http://x/a/b")).as_deref(),
            Some("http://x/a/foo")
        );
        assert_eq!(
            parse("//y", Some("http://x/a/b")).as_deref(),
            Some("http://y/")
        );
        assert_eq!(
            parse("?q", Some("http://x/a/b")).as_deref(),
            Some("http://x/a/b?q")
        );
        assert_eq!(parse("#f", Some("mailto:x")).as_deref(), Some("mailto:x#f"));
        assert_eq!(parse("a", Some("mailto:x")), None);
    }

    #[test]
    fn file() {
        assert_eq!(
            parse("file:c:\\foo", None).as_deref(),
            Some("file:///c:/foo")
        );
        assert_eq!(
            parse("file://localhost/x", None).as_deref(),
            Some("file:///x")
        );
        assert_eq!(
            parse("/c|/..", Some("file:///d:/")).as_deref(),
            Some("file:///c:/")
        );
        assert_eq!(
            parse("..", Some("file:///C:/")).as_deref(),
            Some("file:///C:/")
        );
    }

    #[test]
    fn opaque_path() {
        assert_eq!(
            parse("data:text/plain,a  b ", None).as_deref(),
            Some("data:text/plain,a  b")
        );
        assert_eq!(parse("sc:a ?b", None).as_deref(), Some("sc:a%20?b"));
        assert_eq!(parse("sc:/.//p", None).as_deref(), Some("sc:/.//p"));
    }
}
