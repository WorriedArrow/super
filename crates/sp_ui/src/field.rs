//! single-line text editing state for the url bar: text, caret and
//! selection. positions are byte offsets on char boundaries.

#[derive(Default, Debug, Clone)]
pub(crate) struct TextField {
    text: String,
    /// where the caret is; the moving end of the selection.
    caret: usize,
    /// the fixed end of the selection. equal to `caret` when nothing is
    /// selected.
    anchor: usize,
}

impl TextField {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_owned();
        self.caret = self.text.len();
        self.anchor = self.caret;
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    /// moves the caret to `pos`, keeping the anchor if `extend`.
    pub fn set_caret(&mut self, pos: usize, extend: bool) {
        let pos = self.floor_boundary(pos);
        self.caret = pos;
        if !extend {
            self.anchor = pos;
        }
    }

    /// replaces the selection with `s`. control characters are dropped since
    /// a url bar holds one line.
    pub fn insert(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        let (start, end) = self.selection();
        self.text.replace_range(start..end, &clean);
        self.caret = start + clean.len();
        self.anchor = self.caret;
    }

    pub fn backspace(&mut self) {
        if !self.has_selection() {
            self.anchor = self.prev_boundary(self.caret);
        }
        self.insert("");
    }

    pub fn delete(&mut self) {
        if !self.has_selection() {
            self.anchor = self.next_boundary(self.caret);
        }
        self.insert("");
    }

    pub fn left(&mut self, extend: bool) {
        if self.has_selection() && !extend {
            let (start, _) = self.selection();
            self.set_caret(start, false);
        } else {
            self.set_caret(self.prev_boundary(self.caret), extend);
        }
    }

    pub fn right(&mut self, extend: bool) {
        if self.has_selection() && !extend {
            let (_, end) = self.selection();
            self.set_caret(end, false);
        } else {
            self.set_caret(self.next_boundary(self.caret), extend);
        }
    }

    pub fn home(&mut self, extend: bool) {
        self.set_caret(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        self.set_caret(self.text.len(), extend);
    }

    fn prev_boundary(&self, pos: usize) -> usize {
        self.text[..pos]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, pos: usize) -> usize {
        self.text[pos..]
            .chars()
            .next()
            .map_or(pos, |c| pos + c.len_utf8())
    }

    fn floor_boundary(&self, pos: usize) -> usize {
        let mut pos = pos.min(self.text.len());
        while !self.text.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(text: &str) -> TextField {
        let mut f = TextField::default();
        f.set_text(text);
        f
    }

    #[test]
    fn typing_inserts_at_the_caret() {
        let mut f = field("ac");
        f.left(false);
        f.insert("b");
        assert_eq!(f.text(), "abc");
        assert_eq!(f.caret(), 2);
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut f = field("hello world");
        f.select_all();
        f.insert("x");
        assert_eq!(f.text(), "x");
        assert!(!f.has_selection());
    }

    #[test]
    fn backspace_and_delete_respect_char_boundaries() {
        let mut f = field("aé日");
        f.backspace();
        assert_eq!(f.text(), "aé");
        f.home(false);
        f.delete();
        assert_eq!(f.text(), "é");
        f.delete();
        assert_eq!(f.text(), "");
        f.delete();
        f.backspace();
        assert_eq!(f.text(), "");
    }

    #[test]
    fn shift_arrows_extend_and_plain_arrows_collapse() {
        let mut f = field("abcd");
        f.left(true);
        f.left(true);
        assert_eq!(f.selection(), (2, 4));
        f.left(false);
        assert_eq!(f.selection(), (2, 2));
        f.end(true);
        f.right(false);
        assert_eq!(f.caret(), 4);
        f.home(true);
        assert_eq!(f.selection(), (0, 4));
    }

    #[test]
    fn control_characters_are_dropped() {
        let mut f = field("");
        f.insert("a\nb\r\tc");
        assert_eq!(f.text(), "abc");
    }

    #[test]
    fn set_caret_snaps_inside_a_char() {
        let mut f = field("日本");
        f.set_caret(1, false);
        assert_eq!(f.caret(), 0);
        f.set_caret(100, false);
        assert_eq!(f.caret(), "日本".len());
    }
}
