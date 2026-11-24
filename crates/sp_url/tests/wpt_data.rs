//! runs the data files from wpt's url directory against the rust api, the
//! way the wpt js harnesses run them against the URL class.

use serde_json::Value;
use sp_url::{Url, api, urlencoded};

const URLTESTDATA: &str = include_str!("../../../tests/wpt/url/resources/urltestdata.json");
const URLTESTDATA_JS_ONLY: &str =
    include_str!("../../../tests/wpt/url/resources/urltestdata-javascript-only.json");
const SETTERS: &str = include_str!("../../../tests/wpt/url/resources/setters_tests.json");
const TOASCII: &str = include_str!("../../../tests/wpt/url/resources/toascii.json");
const IDNA_TEST_V2: &str = include_str!("../../../tests/wpt/url/resources/IdnaTestV2.json");
const IDNA_TEST_V2_REMOVED: &str =
    include_str!("../../../tests/wpt/url/resources/IdnaTestV2-removed.json");
const PERCENT_ENCODING: &str =
    include_str!("../../../tests/wpt/url/resources/percent-encoding.json");

/// webidl's USVString conversion replaces lone surrogates with u+fffd before
/// the url parser sees them. json escapes can encode lone surrogates, which
/// serde_json rejects, so do that conversion on the escapes.
fn replace_lone_surrogate_escapes(json: &str) -> String {
    fn escape_at(s: &str, i: usize) -> Option<u16> {
        let hex = s.get(i..i + 6)?.strip_prefix("\\u")?;
        u16::from_str_radix(hex, 16).ok()
    }
    let mut out = String::with_capacity(json.len());
    let mut i = 0;
    while i < json.len() {
        // an escaped backslash can't start a \u escape.
        if json[i..].starts_with("\\\\") {
            out.push_str("\\\\");
            i += 2;
            continue;
        }
        if let Some(unit) = escape_at(json, i) {
            if (0xd800..0xdc00).contains(&unit)
                && escape_at(json, i + 6).is_some_and(|low| (0xdc00..0xe000).contains(&low))
            {
                out.push_str(&json[i..i + 12]);
                i += 12;
                continue;
            }
            if (0xd800..0xe000).contains(&unit) {
                out.push_str("\\uFFFD");
            } else {
                out.push_str(&json[i..i + 6]);
            }
            i += 6;
            continue;
        }
        let c = json[i..].chars().next().expect("in bounds");
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn load(json: &str) -> Value {
    serde_json::from_str(&replace_lone_surrogate_escapes(json)).expect("valid json")
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn getter(url: &Url, attribute: &str) -> String {
    match attribute {
        "href" => api::href(url),
        "origin" => api::origin(url),
        "protocol" => api::protocol(url),
        "username" => api::username(url),
        "password" => api::password(url),
        "host" => api::host(url),
        "hostname" => api::hostname(url),
        "port" => api::port(url),
        "pathname" => api::pathname(url),
        "search" => api::search(url),
        "hash" => api::hash(url),
        "searchParams" => urlencoded::serialize(&urlencoded::parse_str(url.query().unwrap_or(""))),
        other => panic!("unknown attribute {other}"),
    }
}

fn setter(url: &mut Url, attribute: &str, value: &str) {
    match attribute {
        "href" => {
            let _ = api::set_href(url, value);
        }
        "protocol" => api::set_protocol(url, value),
        "username" => api::set_username(url, value),
        "password" => api::set_password(url, value),
        "host" => api::set_host(url, value),
        "hostname" => api::set_hostname(url, value),
        "port" => api::set_port(url, value),
        "pathname" => api::set_pathname(url, value),
        "search" => api::set_search(url, value),
        "hash" => api::set_hash(url, value),
        other => panic!("unknown attribute {other}"),
    }
}

const URL_ATTRIBUTES: &[&str] = &[
    "href",
    "origin",
    "protocol",
    "username",
    "password",
    "host",
    "hostname",
    "port",
    "pathname",
    "search",
    "searchParams",
    "hash",
];

fn report(name: &str, total: usize, failures: &[String]) {
    assert!(total > 0, "{name}: no cases ran");
    assert!(
        failures.is_empty(),
        "{name}: {} of {total} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    eprintln!("{name}: {total}/{total} cases pass");
}

/// url/url-constructor.any.js and url/url-origin.any.js
fn run_url_tests(data: &str, name: &str) {
    let cases = load(data);
    let mut total = 0;
    let mut failures = Vec::new();
    for case in cases.as_array().expect("array") {
        if case.is_string() {
            continue;
        }
        total += 1;
        let input = str_field(case, "input").expect("input");
        let base = str_field(case, "base");
        let result = api::api_url_parse(input, base);
        let label = format!("<{input:?}> against <{base:?}>");
        if case.get("failure").is_some() {
            if let Ok(url) = result {
                failures.push(format!("{label}: expected failure, got {url}"));
            }
            continue;
        }
        let url = match result {
            Ok(url) => url,
            Err(_) => {
                failures.push(format!("{label}: unexpected failure"));
                continue;
            }
        };
        for attribute in URL_ATTRIBUTES {
            let Some(expected) = str_field(case, attribute) else {
                continue;
            };
            let actual = getter(&url, attribute);
            if actual != expected {
                failures.push(format!("{label}: {attribute} {actual:?} != {expected:?}"));
            }
        }
    }
    report(name, total, &failures);
}

#[test]
fn urltestdata() {
    run_url_tests(URLTESTDATA, "urltestdata.json");
}

#[test]
fn urltestdata_javascript_only() {
    run_url_tests(URLTESTDATA_JS_ONLY, "urltestdata-javascript-only.json");
}

/// url/url-setters.any.js
#[test]
fn setters() {
    let cases = load(SETTERS);
    let mut total = 0;
    let mut failures = Vec::new();
    for (attribute, cases) in cases.as_object().expect("object") {
        if attribute == "comment" {
            continue;
        }
        for case in cases.as_array().expect("array") {
            total += 1;
            let href = str_field(case, "href").expect("href");
            let new_value = str_field(case, "new_value").expect("new_value");
            let label = format!("<{href}>.{attribute} = {new_value:?}");
            let Ok(mut url) = Url::parse(href, None) else {
                failures.push(format!("{label}: href doesn't parse"));
                continue;
            };
            setter(&mut url, attribute, new_value);
            for (key, expected) in case["expected"].as_object().expect("object") {
                let actual = getter(&url, key);
                let expected = expected.as_str().expect("string");
                if actual != expected {
                    failures.push(format!("{label}: {key} {actual:?} != {expected:?}"));
                }
            }
        }
    }
    report("setters_tests.json", total, &failures);
}

/// the url half of url/toascii.window.js and url/IdnaTestV2.any.js.
fn check_host_parse(input: &str, output: Option<&str>, failures: &mut Vec<String>) {
    let result = Url::parse(&format!("https://{input}/x"), None);
    match (result, output) {
        (Ok(url), Some(output)) => {
            let checks = [
                ("host", api::host(&url), output.to_owned()),
                ("hostname", api::hostname(&url), output.to_owned()),
                ("pathname", api::pathname(&url), "/x".to_owned()),
                ("href", api::href(&url), format!("https://{output}/x")),
            ];
            for (attribute, actual, expected) in checks {
                if actual != expected {
                    failures.push(format!("{input:?}: {attribute} {actual:?} != {expected:?}"));
                }
            }
        }
        (Ok(url), None) => failures.push(format!("{input:?}: expected failure, got {url}")),
        (Err(_), Some(output)) => failures.push(format!("{input:?}: failed, expected {output:?}")),
        (Err(_), None) => {}
    }
}

/// url/toascii.window.js
#[test]
fn toascii() {
    let cases = load(TOASCII);
    let mut total = 0;
    let mut failures = Vec::new();
    for case in cases.as_array().expect("array") {
        if case.is_string() {
            continue;
        }
        total += 1;
        let input = str_field(case, "input").expect("input");
        let output = str_field(case, "output");
        check_host_parse(input, output, &mut failures);
        for attribute in ["host", "hostname"] {
            let mut url = Url::parse("https://x/x", None).expect("parses");
            setter(&mut url, attribute, input);
            let actual = getter(&url, attribute);
            let expected = output.unwrap_or("x");
            if actual != expected {
                failures.push(format!(
                    "{input:?} via {attribute} setter: {actual:?} != {expected:?}"
                ));
            }
        }
    }
    report("toascii.json", total, &failures);
}

/// url/IdnaTestV2.any.js and url/IdnaTestV2-removed.any.js
fn run_idna_tests(data: &str, name: &str) {
    let cases = load(data);
    let mut total = 0;
    let mut failures = Vec::new();
    for case in cases.as_array().expect("array") {
        if case.is_string() {
            continue;
        }
        let input = str_field(case, "input").expect("input");
        // the harness can't test the empty string through new URL().
        if input.is_empty() {
            continue;
        }
        total += 1;
        check_host_parse(input, str_field(case, "output"), &mut failures);
    }
    report(name, total, &failures);
}

#[test]
fn idna_test_v2() {
    run_idna_tests(IDNA_TEST_V2, "IdnaTestV2.json");
}

#[test]
fn idna_test_v2_removed() {
    run_idna_tests(IDNA_TEST_V2_REMOVED, "IdnaTestV2-removed.json");
}

/// url/percent-encoding.window.js, utf-8 only: query encodings other than
/// utf-8 aren't supported.
#[test]
fn percent_encoding() {
    let cases = load(PERCENT_ENCODING);
    let mut total = 0;
    let mut failures = Vec::new();
    for case in cases.as_array().expect("array") {
        if case.is_string() {
            continue;
        }
        total += 1;
        let input = str_field(case, "input").expect("input");
        let expected = case["output"]["utf-8"].as_str().expect("utf-8 output");
        let url = Url::parse(
            &format!("https://doesnotmatter.invalid/?{input}#{input}"),
            None,
        )
        .expect("parses");
        if api::search(&url) != format!("?{expected}") || api::hash(&url) != format!("#{expected}")
        {
            failures.push(format!(
                "{input:?}: search {:?} hash {:?}, expected {expected:?}",
                api::search(&url),
                api::hash(&url)
            ));
        }
    }
    report("percent-encoding.json", total, &failures);
}
