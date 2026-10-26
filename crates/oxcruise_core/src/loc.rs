use serde::{Deserialize, Serialize};

/// Byte range in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

/// 1-based line and column, as printed in `file:line:col`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Loc {
    pub line: u32,
    pub col: u32,
}

/// Maps byte offsets to [`Loc`] lazily via binary search over line starts.
#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<u32>,
    source_len: u32,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        Self {
            line_starts,
            source_len: source.len() as u32,
        }
    }

    /// Column is counted in bytes, which matches editors for ASCII and is
    /// good enough for CLI output.
    pub fn loc(&self, offset: u32) -> Loc {
        let offset = offset.min(self.source_len);
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        Loc {
            line: line as u32 + 1,
            col: offset - self.line_starts[line] + 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_offsets() {
        let idx = LineIndex::new("ab\ncd\n\nef");
        assert_eq!(idx.loc(0), Loc { line: 1, col: 1 });
        assert_eq!(idx.loc(1), Loc { line: 1, col: 2 });
        assert_eq!(idx.loc(3), Loc { line: 2, col: 1 });
        assert_eq!(idx.loc(7), Loc { line: 4, col: 1 });
        assert_eq!(idx.loc(100), Loc { line: 4, col: 3 });
    }
}
