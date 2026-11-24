// runs the html5lib-tests tokenizer suite (tests/html5lib-tests/tokenizer, format in its
// README.md). every test runs once per initial state, with the whole input in one chunk
// and again fed one code point at a time; both runs must produce the expected tokens and
// error codes, in order. prints pass/total per file and fails if anything fails.

use std::collections::BTreeMap;
use std::process::ExitCode;

use serde_json::Value;
use sp_html::Wtf8Buf;
use sp_html::tokenizer::{State, Token, Tokenizer};

macro_rules! suite {
    ($($file:literal,)*) => {
        &[$(($file, include_str!(concat!("../../../tests/html5lib-tests/tokenizer/", $file))),)*]
    };
}

// xmlViolation.test isn't listed: its "xmlViolationTests" expect the output of "coercing
// an html dom into an infoset", which isn't tokenizer behavior.
const FILES: &[(&str, &str)] = suite![
    "contentModelFlags.test",
    "domjs.test",
    "entities.test",
    "escapeFlag.test",
    "namedEntities.test",
    "numericEntities.test",
    "pendingSpecChanges.test",
    "test1.test",
    "test2.test",
    "test3.test",
    "test4.test",
    "unicodeChars.test",
    "unicodeCharsProblematic.test",
];

#[derive(Debug, PartialEq)]
enum Tok {
    Doctype(Option<Wtf8Buf>, Option<Wtf8Buf>, Option<Wtf8Buf>, bool),
    StartTag(Wtf8Buf, BTreeMap<Wtf8Buf, Wtf8Buf>, bool),
    EndTag(Wtf8Buf),
    Comment(Wtf8Buf),
    ProcessingInstruction(Wtf8Buf, Wtf8Buf),
    Character(Wtf8Buf),
}

type Error = (String, u64, u64);

/// The `doubleEscaped` unescaping: each \uHHHH becomes that code point.
fn unescape(s: &str, double_escaped: bool) -> Wtf8Buf {
    let mut out = Wtf8Buf::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if double_escaped
            && let Some(hex) = rest.strip_prefix("\\u").and_then(|r| r.get(..4))
            && let Ok(cp) = u32::from_str_radix(hex, 16)
        {
            out.push_code_point(cp);
            rest = &rest[6..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn string(v: &Value, double_escaped: bool) -> Wtf8Buf {
    unescape(v.as_str().expect("string"), double_escaped)
}

fn opt_string(v: &Value, double_escaped: bool) -> Option<Wtf8Buf> {
    v.as_str().map(|s| unescape(s, double_escaped))
}

fn expected_tokens(output: &Value, de: bool) -> Vec<Tok> {
    let mut tokens = Vec::new();
    for t in output.as_array().expect("output is an array") {
        let t = t.as_array().expect("token is an array");
        let tok = match t[0].as_str().expect("token kind") {
            "DOCTYPE" => Tok::Doctype(
                opt_string(&t[1], de),
                opt_string(&t[2], de),
                opt_string(&t[3], de),
                t[4].as_bool().expect("correctness"),
            ),
            "StartTag" => Tok::StartTag(
                string(&t[1], de),
                t[2].as_object()
                    .expect("attributes")
                    .iter()
                    .map(|(k, v)| (unescape(k, de), string(v, de)))
                    .collect(),
                t.get(3).and_then(Value::as_bool).unwrap_or(false),
            ),
            "EndTag" => Tok::EndTag(string(&t[1], de)),
            "Comment" => Tok::Comment(string(&t[1], de)),
            "ProcessingInstruction" => {
                Tok::ProcessingInstruction(string(&t[1], de), string(&t[2], de))
            }
            "Character" => Tok::Character(string(&t[1], de)),
            other => panic!("unknown token kind {other}"),
        };
        push_coalesced(&mut tokens, tok);
    }
    tokens
}

fn push_coalesced(tokens: &mut Vec<Tok>, tok: Tok) {
    if let (Tok::Character(new), Some(Tok::Character(prev))) = (&tok, tokens.last_mut()) {
        prev.push_wtf8(new);
    } else {
        tokens.push(tok);
    }
}

fn initial_state(name: &str) -> State {
    match name {
        "Data state" => State::Data,
        "PLAINTEXT state" => State::Plaintext,
        "RCDATA state" => State::Rcdata,
        "RAWTEXT state" => State::Rawtext,
        "Script data state" => State::ScriptData,
        "CDATA section state" => State::CdataSection,
        other => panic!("unknown initial state {other}"),
    }
}

fn run(chunks: &[Wtf8Buf], state: State, last_start_tag: Option<&str>) -> (Vec<Tok>, Vec<Error>) {
    let mut t = Tokenizer::new();
    t.set_report_errors(true);
    t.set_state(state);
    t.set_last_start_tag(last_start_tag);
    let mut tokens = Vec::new();
    let mut collect = |t: &mut Tokenizer| {
        while let Some(token) = t.next_token() {
            let tok = match token {
                Token::Doctype(d) => {
                    Tok::Doctype(d.name, d.public_id, d.system_id, !d.force_quirks)
                }
                Token::StartTag(tag) => Tok::StartTag(
                    tag.name,
                    tag.attributes
                        .into_iter()
                        .map(|a| (a.name, a.value))
                        .collect(),
                    tag.self_closing,
                ),
                Token::EndTag(tag) => Tok::EndTag(tag.name),
                Token::Comment(data) => Tok::Comment(data),
                Token::ProcessingInstruction(pi) => {
                    Tok::ProcessingInstruction(pi.target.as_str().into(), pi.data)
                }
                Token::Characters(data) => Tok::Character(data),
                Token::Eof => continue,
            };
            push_coalesced(&mut tokens, tok);
        }
    };
    for chunk in chunks {
        t.push_wtf8(chunk.clone());
        collect(&mut t);
    }
    t.close();
    collect(&mut t);
    let errors = t
        .take_errors()
        .into_iter()
        .map(|e| {
            (
                e.code.as_str().to_owned(),
                u64::from(e.line),
                u64::from(e.col),
            )
        })
        .collect();
    (tokens, errors)
}

fn main() -> ExitCode {
    let mut failures = Vec::new();
    let mut position_mismatches = Vec::new();
    let (mut all_passed, mut all_total) = (0, 0);
    for (file, json) in FILES {
        let suite: Value = serde_json::from_str(json).expect("test file is json");
        let (mut passed, mut total) = (0, 0);
        for test in suite["tests"].as_array().expect("tests array") {
            let de = test["doubleEscaped"].as_bool().unwrap_or(false);
            let description = test["description"].as_str().unwrap_or("");
            let input = string(&test["input"], de);
            let expected = expected_tokens(&test["output"], de);
            let expected_errors: Vec<Error> = test["errors"]
                .as_array()
                .map(|errors| {
                    errors
                        .iter()
                        .map(|e| {
                            (
                                e["code"].as_str().expect("error code").to_owned(),
                                e["line"].as_u64().expect("line"),
                                e["col"].as_u64().expect("col"),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let states: Vec<&str> = match test["initialStates"].as_array() {
                Some(states) => states.iter().filter_map(Value::as_str).collect(),
                None => vec!["Data state"],
            };
            let last_start_tag = test["lastStartTag"].as_str();
            let one_by_one: Vec<Wtf8Buf> = input
                .code_points()
                .map(|(cp, _)| {
                    let mut chunk = Wtf8Buf::new();
                    chunk.push_code_point(cp);
                    chunk
                })
                .collect();
            for state_name in states {
                total += 1;
                let state = initial_state(state_name);
                let whole = run(std::slice::from_ref(&input), state, last_start_tag);
                let split = run(&one_by_one, state, last_start_tag);
                let codes = |errors: &[Error]| -> Vec<String> {
                    errors.iter().map(|e| e.0.clone()).collect()
                };
                let codes_match = codes(&whole.1) == codes(&expected_errors);
                if whole.0 == expected && codes_match && split == whole {
                    passed += 1;
                    if whole.1 != expected_errors {
                        position_mismatches.push(format!(
                            "{file}: {description} ({state_name}): expected {expected_errors:?}, \
                             got {:?}",
                            whole.1
                        ));
                    }
                } else {
                    failures.push(format!(
                        "{file}: {description} ({state_name})\n  input:           {input:?}\n  \
                         expected:        {expected:?}\n  got:             {:?}\n  \
                         expected errors: {expected_errors:?}\n  got errors:      {:?}\n  \
                         chunked matches: {}",
                        whole.0,
                        whole.1,
                        split == whole
                    ));
                }
            }
        }
        println!("{file}: {passed}/{total}");
        all_passed += passed;
        all_total += total;
    }
    println!("tokenizer: {all_passed}/{all_total}");
    // error positions aren't defined by the spec, so they're reported but don't fail
    // the run. known upstream mismatch: html5lib-tests issue #202 (eof-in-processing-
    // instruction columns in test3.test).
    println!(
        "error positions: {}/{all_passed} passing runs match",
        all_passed - position_mismatches.len()
    );
    for mismatch in &position_mismatches {
        println!("  position differs: {mismatch}");
    }
    if failures.is_empty() {
        return ExitCode::SUCCESS;
    }
    for failure in failures.iter().take(40) {
        println!("\nFAIL {failure}");
    }
    if failures.len() > 40 {
        println!("\n... and {} more failures", failures.len() - 40);
    }
    ExitCode::FAILURE
}
