//! feeds pseudo-random strings through the parser, the setters and the idna
//! code. page content controls urls, so none of this may panic.

use sp_url::{Host, Url, api, domain_to_unicode, urlencoded};

/// xorshift64*, seeded so failures reproduce.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// pieces that steer the parser into its interesting states.
const PIECES: &[&str] = &[
    "http:",
    "https:",
    "file:",
    "ws:",
    "blob:",
    "data:",
    "sc:",
    "javascript:",
    "//",
    "/",
    "\\",
    "?",
    "#",
    "@",
    ":",
    "[",
    "]",
    "[::1]",
    "[::1.2.3.4]",
    "1.2.3.4",
    "0x",
    "09",
    ".",
    "..",
    "%2e",
    "%",
    "%zz",
    "%41",
    "%ff",
    "xn--",
    "xn--zca",
    "c:",
    "c|",
    "localhost",
    "80",
    "65536",
    "\t",
    "\n",
    " ",
    "\u{0}",
    "\u{7f}",
    "\u{ad}",
    "\u{df}",
    "\u{5d0}",
    "\u{660}",
    "\u{200c}",
    "\u{200d}",
    "\u{94d}",
    "\u{301}",
    "\u{fffd}",
    "\u{ff0e}",
    "\u{3002}",
    "\u{1f4a9}",
    "a",
    "Z",
    "0",
    "-",
];

fn random_string(rng: &mut Rng) -> String {
    let mut s = String::new();
    for _ in 0..rng.below(12) {
        if rng.below(3) == 0 {
            // any scalar value, weighted toward the low planes.
            let max = [0x80, 0x800, 0x1_0000, 0x11_0000][rng.below(4)];
            if let Some(c) = char::from_u32(rng.below(max) as u32) {
                s.push(c);
            }
        } else {
            s.push_str(PIECES[rng.below(PIECES.len())]);
        }
    }
    s
}

#[test]
fn random_inputs_do_not_panic() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let bases: Vec<Url> = [
        "http://h/a/b?q#f",
        "file:///c:/d",
        "sc:opaque",
        "sc://h/p",
        "about:blank",
    ]
    .iter()
    .map(|b| Url::parse(b, None).expect("base parses"))
    .collect();
    for _ in 0..20_000 {
        let input = random_string(&mut rng);
        let base = &bases[rng.below(bases.len())];
        let parsed = Url::parse(&input, None);
        let relative = Url::parse(&input, Some(base));
        for url in [parsed, relative].into_iter().flatten() {
            // serializing and reparsing gives the same url.
            let href = api::href(&url);
            let reparsed = Url::parse(&href, None).expect("serialization reparses");
            assert_eq!(api::href(&reparsed), href, "input {input:?}");
            let _ = url.origin().serialize();
        }

        let mut url = base.clone();
        let value = random_string(&mut rng);
        match rng.below(11) {
            0 => {
                let _ = api::set_href(&mut url, &value);
            }
            1 => api::set_protocol(&mut url, &value),
            2 => api::set_username(&mut url, &value),
            3 => api::set_password(&mut url, &value),
            4 => api::set_host(&mut url, &value),
            5 => api::set_hostname(&mut url, &value),
            6 => api::set_port(&mut url, &value),
            7 => api::set_pathname(&mut url, &value),
            8 => api::set_search(&mut url, &value),
            9 => api::set_hash(&mut url, &value),
            _ => {
                let _ = urlencoded::serialize(&urlencoded::parse_str(&value));
            }
        }
        let _ = api::href(&url);

        let _ = Host::parse(&input, false);
        let _ = Host::parse(&input, true);
        let _ = domain_to_unicode(&input);
    }
}
