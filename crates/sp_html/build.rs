// generates the named character references table from entities.json, vendored from
// https://html.spec.whatwg.org/entities.json
// (https://html.spec.whatwg.org/multipage/named-characters.html#named-character-references).

// build scripts run on the build host, not in the renderer, so the renderer-side ban on
// filesystem access (clippy.toml) doesn't apply to writing the generated table.
#![allow(clippy::disallowed_methods)]

use std::fmt::Write as _;
use std::path::Path;

const ENTITIES_JSON: &str = include_str!("entities.json");

fn main() {
    println!("cargo::rerun-if-changed=entities.json");
    println!("cargo::rerun-if-changed=build.rs");

    // every entry is on its own line:
    //   "&AElig": { "codepoints": [198], "characters": "Æ" },
    let mut entries: Vec<(String, Vec<u32>)> = Vec::new();
    for line in ENTITIES_JSON.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("\"&") else {
            continue;
        };
        let (name, rest) = rest.split_once('"').expect("entity name is quoted");
        let (_, rest) = rest.split_once('[').expect("codepoints array");
        let (codepoints, _) = rest.split_once(']').expect("codepoints array end");
        let codepoints = codepoints
            .split(',')
            .map(|n| n.trim().parse().expect("codepoint is a number"))
            .collect();
        entries.push((name.to_owned(), codepoints));
    }
    assert_eq!(entries.len(), 2231, "entities.json has 2231 entries");
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    let mut out =
        String::from("pub(crate) static NAMED_CHARACTER_REFERENCES: &[(&str, &str)] = &[\n");
    for (name, codepoints) in &entries {
        let mut value = String::new();
        for cp in codepoints {
            write!(value, "\\u{{{cp:x}}}").unwrap();
        }
        writeln!(out, "    (\"{name}\", \"{value}\"),").unwrap();
    }
    out.push_str("];\n");

    let out_dir = std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR");
    std::fs::write(
        Path::new(&out_dir).join("named_character_references.rs"),
        out,
    )
    .expect("write the generated table");
}
