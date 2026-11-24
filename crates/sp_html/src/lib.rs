//! html parsing: https://html.spec.whatwg.org/multipage/parsing.html

mod error;
pub mod tokenizer;
mod wtf8;

pub use error::{ErrorCode, ParseError};
pub use wtf8::{CodePoints, Wtf8Buf};
