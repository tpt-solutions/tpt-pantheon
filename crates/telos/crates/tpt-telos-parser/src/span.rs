//! Source locations for AST nodes, used to report real line/column positions
//! in diagnostics (e.g. `telos-sdk`'s `VerificationReport`).
//!
//! The lexer (`lexer::lex`) tags each token with a pair of *char offsets*
//! (indices into the source's `Vec<char>`), not line/column. [`LineIndex`]
//! converts an offset into a 1-based `(line, col)` pair.

/// A 1-based source location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

/// Maps char offsets into a source string to 1-based `(line, col)` pairs.
pub struct LineIndex {
    /// Char offset of the first character of each line (line 0 is index 0).
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a line index by scanning `src` once for newlines.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_parser::span::LineIndex;
    ///
    /// let idx = LineIndex::new("ab\ncd\nef");
    /// assert_eq!(idx.line_col(0), (1, 1)); // 'a'
    /// assert_eq!(idx.line_col(3), (2, 1)); // 'c'
    /// assert_eq!(idx.line_col(6), (3, 1)); // 'e'
    /// ```
    pub fn new(src: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, c) in src.chars().enumerate() {
            if c == '\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex { line_starts }
    }

    /// Convert a char offset into a 1-based `(line, col)` pair.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        // Find the last line_start <= offset.
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        (line_idx + 1, offset - line_start + 1)
    }

    /// Convert a char offset into a [`Span`].
    pub fn span_at(&self, offset: usize) -> Span {
        let (line, col) = self.line_col(offset);
        Span { line, col }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line() {
        let idx = LineIndex::new("abc");
        assert_eq!(idx.span_at(0), Span { line: 1, col: 1 });
        assert_eq!(idx.span_at(2), Span { line: 1, col: 3 });
    }

    #[test]
    fn multi_line() {
        let idx = LineIndex::new("ab\ncd\nef");
        assert_eq!(idx.span_at(0), Span { line: 1, col: 1 });
        assert_eq!(idx.span_at(3), Span { line: 2, col: 1 });
        assert_eq!(idx.span_at(4), Span { line: 2, col: 2 });
        assert_eq!(idx.span_at(6), Span { line: 3, col: 1 });
    }

    #[test]
    fn offset_at_end_of_source() {
        let idx = LineIndex::new("ab\ncd");
        // Offset 5 is one past the last char ('d' at offset 4) -- the Eof
        // token in the lexer sits here.
        assert_eq!(idx.span_at(5), Span { line: 2, col: 3 });
    }
}
