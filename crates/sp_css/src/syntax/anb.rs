//! https://drafts.csswg.org/css-syntax-3/#anb-microsyntax

use super::{ComponentValue, Numeric, Token};

/// the a and b of an <a-n-plus-b>. values beyond i32 saturate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnB {
    pub a: i32,
    pub b: i32,
}

fn integer(value: &Numeric) -> i32 {
    // `as` saturates, which is the clamping selectors want.
    value.value as i32
}

/// "n-" followed by one or more digits: returns b, the negated digits.
fn ndashdigit(name: &str) -> Option<i32> {
    let digits = name.strip_prefix("n-")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(-(digits.parse::<f64>().ok()? as i32))
}

/// https://drafts.csswg.org/css-syntax-3/#the-anb-type
/// matches the whole of `values`, with whitespace allowed around it.
pub fn parse_a_n_plus_b(values: &[ComponentValue]) -> Option<AnB> {
    // the non-whitespace tokens, each with whether whitespace came before it. functions and
    // blocks never match.
    let mut tokens = Vec::new();
    let mut after_whitespace = false;
    for value in values {
        match value {
            ComponentValue::Token(Token::Whitespace, _) => after_whitespace = true,
            ComponentValue::Token(token, _) => {
                tokens.push((token, after_whitespace));
                after_whitespace = false;
            }
            _ => return None,
        }
    }
    let (first, rest) = tokens.split_first()?;
    // a, and the rest of the "n" part (lowercased, starting at the "n") with what follows it.
    let (a, n_part, rest) = match first.0 {
        Token::Ident(ident) => {
            let ident = ident.to_ascii_lowercase();
            match ident.as_str() {
                "odd" | "even" if rest.is_empty() => {
                    return Some(AnB {
                        a: 2,
                        b: i32::from(ident == "odd"),
                    });
                }
                _ => match ident.strip_prefix('-') {
                    Some(n_part) => (-1, n_part.to_owned(), rest),
                    None => (1, ident, rest),
                },
            }
        }
        Token::Number(value) if value.is_integer && rest.is_empty() => {
            return Some(AnB {
                a: 0,
                b: integer(value),
            });
        }
        Token::Dimension { value, unit } if value.is_integer => {
            (integer(value), unit.to_ascii_lowercase(), rest)
        }
        // †: '+' directly followed by an ident that starts with "n".
        Token::Delim('+') => match rest.split_first()? {
            ((Token::Ident(ident), false), rest) if !ident.starts_with('-') => {
                (1, ident.to_ascii_lowercase(), rest)
            }
            _ => return None,
        },
        _ => return None,
    };
    let b = match (n_part.as_str(), rest) {
        // <n-dimension>, '+'? n, -n
        ("n", []) => 0,
        // <n-dimension> <signed-integer>, '+'? n <signed-integer>, -n <signed-integer>
        ("n", [(Token::Number(value), _)]) if value.is_integer && value.sign.is_some() => {
            integer(value)
        }
        // <n-dimension> ['+' | '-'] <signless-integer>, and the same after n and -n
        (
            "n",
            [
                (Token::Delim(sign @ ('+' | '-')), _),
                (Token::Number(value), _),
            ],
        ) if value.is_integer && value.sign.is_none() => {
            if *sign == '-' {
                integer(value).saturating_neg()
            } else {
                integer(value)
            }
        }
        // <ndash-dimension> <signless-integer>, '+'? n- <signless-integer>, -n- <signless-integer>
        ("n-", [(Token::Number(value), _)]) if value.is_integer && value.sign.is_none() => {
            integer(value).saturating_neg()
        }
        // <ndashdigit-dimension>, '+'? <ndashdigit-ident>, <dashndashdigit-ident>
        (n_part, []) => ndashdigit(n_part)?,
        _ => return None,
    };
    Some(AnB { a, b })
}
