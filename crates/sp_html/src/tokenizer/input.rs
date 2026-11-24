// https://html.spec.whatwg.org/multipage/parsing.html#preprocessing-the-input-stream

use std::collections::VecDeque;

use crate::Wtf8Buf;

const LF: u32 = 0x0A;
const CR: u32 = 0x0D;

pub(crate) enum Next {
    Char(u32),
    Eof,
    /// no input is available yet, or the insertion point was reached.
    Suspend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Peeked {
    Char(u32),
    Eof,
    /// the lookahead ran past the input that's available so far.
    End,
}

/// The input stream: chunks of code points that haven't been consumed yet, in order.
/// Network data is appended at the end, `document.write()` inserts at the insertion
/// point.
#[derive(Default)]
pub(crate) struct InputStream {
    chunks: VecDeque<Wtf8Buf>,
    /// bytes of `chunks[0]` already consumed.
    offset: usize,
    /// the insertion point is just before `chunks[i]` (for i == 0, just before the next
    /// input character); i == chunks.len() is the end of the stream.
    insertion_point: Option<usize>,
    closed: bool,
    /// the last code point consumed was CR, so an LF right after it is ignored.
    after_cr: bool,
    line: u32,
    col: u32,
    after_lf: bool,
    /// utf-16 length of the current input character.
    last_width: u32,
    eof_consumed: bool,
}

impl InputStream {
    pub(crate) fn new() -> Self {
        InputStream {
            line: 1,
            last_width: 1,
            ..InputStream::default()
        }
    }

    pub(crate) fn append(&mut self, text: Wtf8Buf) {
        if !text.is_empty() {
            self.chunks.push_back(text);
        }
    }

    /// No more input will be appended; the end of the stream is the EOF character.
    pub(crate) fn close(&mut self) {
        self.closed = true;
    }

    pub(crate) fn set_insertion_point_before_next(&mut self) {
        self.trim_front();
        self.insertion_point = Some(0);
    }

    pub(crate) fn set_insertion_point_at_end(&mut self) {
        self.insertion_point = Some(self.chunks.len());
    }

    pub(crate) fn clear_insertion_point(&mut self) {
        self.insertion_point = None;
    }

    pub(crate) fn has_insertion_point(&self) -> bool {
        self.insertion_point.is_some()
    }

    /// Inserts text just before the insertion point, or at the end if it's undefined.
    pub(crate) fn insert(&mut self, text: Wtf8Buf) {
        if text.is_empty() {
            return;
        }
        match self.insertion_point {
            Some(ip) => {
                if ip == 0 {
                    self.trim_front();
                }
                self.chunks.insert(ip, text);
                self.insertion_point = Some(ip + 1);
            }
            None => self.chunks.push_back(text),
        }
    }

    /// Drops the consumed part of the first chunk so that text can go in front of the
    /// rest of it.
    fn trim_front(&mut self) {
        if self.offset > 0
            && let Some(front) = self.chunks.front_mut()
        {
            *front = Wtf8Buf::from_wtf8_bytes(&front.as_bytes()[self.offset..]);
            self.offset = 0;
        }
    }

    fn pop_front(&mut self) {
        self.chunks.pop_front();
        self.offset = 0;
        if let Some(ip) = &mut self.insertion_point {
            *ip = ip.saturating_sub(1);
        }
    }

    /// Consumes the next input character, normalizing newlines: CR LF and lone CR both
    /// become LF.
    pub(crate) fn consume(&mut self, stop_at_insertion_point: bool) -> Next {
        loop {
            if stop_at_insertion_point && self.insertion_point == Some(0) {
                return Next::Suspend;
            }
            let Some(front) = self.chunks.front() else {
                if !self.closed {
                    return Next::Suspend;
                }
                if !self.eof_consumed {
                    self.eof_consumed = true;
                    self.advance_position(0);
                }
                return Next::Eof;
            };
            let (cp, len) = front
                .code_point_at(self.offset)
                .expect("chunks are never empty and offset stays inside");
            self.offset += len;
            if self.offset == front.len() {
                self.pop_front();
            }
            if self.after_cr && cp == LF {
                self.after_cr = false;
                continue;
            }
            self.after_cr = cp == CR;
            let cp = if cp == CR { LF } else { cp };
            self.advance_position(cp);
            return Next::Char(cp);
        }
    }

    /// Looks at the characters after the current one without consuming them.
    pub(crate) fn peek(&self, stop_at_insertion_point: bool) -> Peek<'_> {
        Peek {
            input: self,
            chunk: 0,
            offset: self.offset,
            at_chunk_start: true,
            after_cr: self.after_cr,
            stop_at_insertion_point,
        }
    }

    // columns count utf-16 code units, like javascript string offsets and the positions
    // in html5lib-tests.
    fn advance_position(&mut self, cp: u32) {
        if self.after_lf {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += self.last_width;
        }
        self.after_lf = cp == LF;
        self.last_width = if cp > 0xFFFF { 2 } else { 1 };
    }

    /// Line and column of the current input character.
    pub(crate) fn position(&self) -> (u32, u32) {
        (self.line, self.col)
    }

    /// Line and column the next input character will have.
    pub(crate) fn next_position(&self) -> (u32, u32) {
        if self.after_lf {
            (self.line + 1, 1)
        } else {
            (self.line, self.col + self.last_width)
        }
    }
}

pub(crate) struct Peek<'a> {
    input: &'a InputStream,
    chunk: usize,
    offset: usize,
    at_chunk_start: bool,
    after_cr: bool,
    stop_at_insertion_point: bool,
}

impl Iterator for Peek<'_> {
    type Item = Peeked;

    fn next(&mut self) -> Option<Peeked> {
        loop {
            if self.at_chunk_start
                && self.stop_at_insertion_point
                && self.input.insertion_point == Some(self.chunk)
            {
                return Some(Peeked::End);
            }
            let Some(buf) = self.input.chunks.get(self.chunk) else {
                return Some(if self.input.closed {
                    Peeked::Eof
                } else {
                    Peeked::End
                });
            };
            let (cp, len) = buf.code_point_at(self.offset)?;
            self.offset += len;
            self.at_chunk_start = false;
            if self.offset == buf.len() {
                self.chunk += 1;
                self.offset = 0;
                self.at_chunk_start = true;
            }
            if self.after_cr && cp == LF {
                self.after_cr = false;
                continue;
            }
            self.after_cr = cp == CR;
            return Some(Peeked::Char(if cp == CR { LF } else { cp }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(input: &mut InputStream, stop: bool) -> (String, bool) {
        let mut out = String::new();
        loop {
            match input.consume(stop) {
                Next::Char(cp) => out.push(char::from_u32(cp).unwrap()),
                Next::Eof => return (out, true),
                Next::Suspend => return (out, false),
            }
        }
    }

    #[test]
    fn newlines_normalize_across_chunks() {
        let mut input = InputStream::new();
        input.append("a\r".into());
        input.append("\nb\r\rc\r".into());
        assert_eq!(drain(&mut input, false), ("a\nb\n\nc\n".into(), false));
        input.append("\n".into());
        input.close();
        assert_eq!(drain(&mut input, false), (String::new(), true));
        assert_eq!(input.position(), (5, 1));
    }

    #[test]
    fn insertion_point_orders_writes() {
        let mut input = InputStream::new();
        input.append("<script></script>rest".into());
        for _ in 0.."<script></script>".len() {
            input.consume(false);
        }
        input.set_insertion_point_before_next();
        input.insert("one ".into());
        input.insert("two ".into());
        assert_eq!(drain(&mut input, true), ("one two ".into(), false));
        input.insert("three ".into());
        assert_eq!(drain(&mut input, true), ("three ".into(), false));
        input.clear_insertion_point();
        input.close();
        assert_eq!(drain(&mut input, true), ("rest".into(), true));
    }

    #[test]
    fn peek_matches_consume() {
        let mut input = InputStream::new();
        input.append("x\r".into());
        input.append("\nyz".into());
        let peeked: Vec<Peeked> = input.peek(false).take(4).collect();
        assert_eq!(
            peeked,
            [
                Peeked::Char('x'.into()),
                Peeked::Char(LF),
                Peeked::Char('y'.into()),
                Peeked::Char('z'.into()),
            ]
        );
        input.consume(false);
        input.consume(false);
        assert!(input.peek(false).next() == Some(Peeked::Char('y'.into())));
        input.consume(false);
        input.consume(false);
        assert!(input.peek(false).next() == Some(Peeked::End));
        input.close();
        assert!(input.peek(false).next() == Some(Peeked::Eof));
    }
}
