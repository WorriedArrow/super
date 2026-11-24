use super::*;

fn tokenize_chunks(chunks: &[&str], state: State) -> (Vec<Token>, Vec<ParseError>) {
    let mut t = Tokenizer::new();
    t.set_report_errors(true);
    t.set_state(state);
    let mut tokens = Vec::new();
    for chunk in chunks {
        t.push_str(chunk);
        while let Some(token) = t.next_token() {
            tokens.push(token);
        }
    }
    t.close();
    while let Some(token) = t.next_token() {
        tokens.push(token);
    }
    (coalesce(tokens), t.take_errors())
}

fn coalesce(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    for token in tokens {
        if let (Token::Characters(new), Some(Token::Characters(prev))) = (&token, out.last_mut()) {
            prev.push_wtf8(new);
        } else {
            out.push(token);
        }
    }
    out
}

fn chars(s: &str) -> Token {
    Token::Characters(s.into())
}

fn start(name: &str, attrs: &[(&str, &str)]) -> Token {
    Token::StartTag(Tag {
        name: name.into(),
        self_closing: false,
        attributes: attrs
            .iter()
            .map(|&(name, value)| Attribute {
                name: name.into(),
                value: value.into(),
            })
            .collect(),
    })
}

fn end(name: &str) -> Token {
    Token::EndTag(Tag {
        name: name.into(),
        ..Tag::default()
    })
}

#[test]
fn simple_document() {
    let (tokens, errors) = tokenize_chunks(&["<p class=a>x &amp; y</p>"], State::Data);
    assert_eq!(
        tokens,
        [
            start("p", &[("class", "a")]),
            chars("x & y"),
            end("p"),
            Token::Eof
        ]
    );
    assert!(errors.is_empty());
}

#[test]
fn input_split_anywhere_gives_the_same_tokens() {
    let input = "<!DOCTYPE html><p a='&notin;' b=\"&amp\">&notit; &#x41;&#66</p>\
                 <!-- c --><?pi data?><![CDATA[x]]>\r\n<script><!--<script></script>-->";
    let whole = tokenize_chunks(&[input], State::Data);
    let cps: Vec<String> = input.chars().map(String::from).collect();
    let one_by_one: Vec<&str> = cps.iter().map(String::as_str).collect();
    assert_eq!(tokenize_chunks(&one_by_one, State::Data), whole);
    for split in 1..input.len() {
        if input.is_char_boundary(split) {
            let (a, b) = input.split_at(split);
            assert_eq!(
                tokenize_chunks(&[a, b], State::Data),
                whole,
                "split at {split}"
            );
        }
    }
}

#[test]
fn tree_builder_switches_state_between_tokens() {
    let mut t = Tokenizer::new();
    t.push_str("<title>a<b>&amp;</title><p>");
    t.close();
    assert_eq!(t.next_token(), Some(start("title", &[])));
    t.set_state(State::Rcdata);
    assert_eq!(t.next_token(), Some(chars("a<b>&")));
    assert_eq!(t.next_token(), Some(end("title")));
    assert_eq!(t.next_token(), Some(start("p", &[])));
    assert_eq!(t.next_token(), Some(Token::Eof));
    assert_eq!(t.next_token(), None);
}

#[test]
fn cdata_needs_foreign_content() {
    let mut t = Tokenizer::new();
    t.set_cdata_allowed(true);
    t.push_str("<![CDATA[a<b]]>");
    t.close();
    assert_eq!(t.next_token(), Some(chars("a<b")));
    let (tokens, _) = tokenize_chunks(&["<![CDATA[a<b]]>"], State::Data);
    assert_eq!(tokens[0], Token::Comment("[CDATA[a<b]]".into()));
}

#[test]
fn document_write_at_insertion_point() {
    let mut t = Tokenizer::new();
    t.push_str("<script></script>after");
    assert_eq!(t.next_token(), Some(start("script", &[])));
    t.set_state(State::ScriptData);
    assert_eq!(t.next_token(), Some(end("script")));
    // the script runs: insertion point just before the next input character.
    t.set_insertion_point();
    t.insert_at_insertion_point("<di".into());
    assert_eq!(t.next_token_before_insertion_point(), None);
    t.insert_at_insertion_point("v id=x>w".into());
    assert_eq!(
        t.next_token_before_insertion_point(),
        Some(start("div", &[("id", "x")]))
    );
    assert_eq!(t.next_token_before_insertion_point(), Some(chars("w")));
    assert_eq!(t.next_token_before_insertion_point(), None);
    t.clear_insertion_point();
    t.close();
    assert_eq!(t.next_token(), Some(chars("after")));
    assert_eq!(t.next_token(), Some(Token::Eof));
}

#[test]
fn lone_surrogates_pass_through() {
    let mut t = Tokenizer::new();
    t.set_report_errors(true);
    t.push_wtf8(Wtf8Buf::from_utf16(&[0x3C, 0x61, 0xD800, 0x3E, 0xDC00]));
    t.close();
    assert_eq!(
        t.next_token(),
        Some(Token::StartTag(Tag {
            name: Wtf8Buf::from_utf16(&[0x61, 0xD800]),
            ..Tag::default()
        }))
    );
    assert_eq!(
        t.next_token(),
        Some(Token::Characters(Wtf8Buf::from_utf16(&[0xDC00])))
    );
    let codes: Vec<_> = t.take_errors().iter().map(|e| e.code).collect();
    assert_eq!(codes, [ErrorCode::SurrogateInInputStream; 2]);
}

#[test]
fn many_attributes_drop_duplicates() {
    let mut input = String::from("<a");
    for i in 0..100 {
        input.push_str(&format!(" a{i}={i}"));
    }
    input.push_str(" a7=dup a99=dup>");
    let (tokens, errors) = tokenize_chunks(&[&input], State::Data);
    let Token::StartTag(tag) = &tokens[0] else {
        panic!("{tokens:?}")
    };
    assert_eq!(tag.attributes.len(), 100);
    assert_eq!(tag.attributes[7].value, "7");
    assert_eq!(errors.len(), 2);
}

/// xorshift, so the randomized test is reproducible without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn random_input_never_panics_and_chunking_doesnt_matter() {
    const PIECES: &[&str] = &[
        "<",
        ">",
        "/",
        "!",
        "-",
        "--",
        "?",
        "&",
        "#",
        "x",
        "X",
        ";",
        "=",
        "\"",
        "'",
        "`",
        " ",
        "\t",
        "\n",
        "\r",
        "\r\n",
        "\0",
        "\u{1}",
        "\u{FFFF}",
        "\u{80}",
        "a",
        "Z",
        "9",
        "é",
        "\u{1F600}",
        "]",
        "]]",
        "[CDATA[",
        "DOCTYPE",
        "doctype",
        "PUBLIC",
        "SYSTEM",
        "script",
        "<script>",
        "</script>",
        "<!--",
        "-->",
        "amp",
        "notin",
        "&amp;",
        "&#x",
        "&#",
        "xml",
        "<?",
        "?>",
        "title",
        "</title>",
        "<a b=c>",
    ];
    const STATES: &[State] = &[
        State::Data,
        State::Rcdata,
        State::Rawtext,
        State::ScriptData,
        State::Plaintext,
        State::CdataSection,
    ];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..3000 {
        let mut input = String::new();
        for _ in 0..rng.below(40) {
            input.push_str(PIECES[rng.below(PIECES.len())]);
        }
        let state = STATES[rng.below(STATES.len())];
        let whole = tokenize_chunks(&[&input], state);
        let mut chunks = Vec::new();
        let mut rest = input.as_str();
        while !rest.is_empty() {
            let mut at = 1 + rng.below(rest.len());
            while !rest.is_char_boundary(at) {
                at += 1;
            }
            let (chunk, tail) = rest.split_at(at);
            chunks.push(chunk);
            rest = tail;
        }
        assert_eq!(
            tokenize_chunks(&chunks, state),
            whole,
            "{input:?} in {state:?} split as {chunks:?}"
        );
    }
}
