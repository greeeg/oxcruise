//! Line text and position encodings. The graph counts columns in bytes; clients usually want UTF-16.

use lsp_types::PositionEncodingKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

impl Encoding {
    pub fn kind(self) -> PositionEncodingKind {
        match self {
            Self::Utf8 => PositionEncodingKind::UTF8,
            Self::Utf16 => PositionEncodingKind::UTF16,
        }
    }

    /// Column in this encoding of byte offset `byte` in `line` (clamped to a character boundary).
    pub fn column(self, line: &str, byte: usize) -> u32 {
        let mut byte = byte.min(line.len());
        while !line.is_char_boundary(byte) {
            byte -= 1;
        }
        match self {
            Self::Utf8 => byte as u32,
            Self::Utf16 => line[..byte].encode_utf16().count() as u32,
        }
    }

    /// Inverse of [`Encoding::column`].
    pub fn byte_offset(self, line: &str, column: u32) -> usize {
        match self {
            Self::Utf8 => (column as usize).min(line.len()),
            Self::Utf16 => {
                let mut units = 0;
                for (i, c) in line.char_indices() {
                    if units >= column {
                        return i;
                    }
                    units += c.len_utf16() as u32;
                }
                line.len()
            }
        }
    }
}

/// The text of one file, as analysed (read from disk).
#[derive(Debug, Default, Clone)]
pub struct Source {
    text: Option<String>,
}

impl Source {
    pub fn read(path: &std::path::Path) -> Source {
        Source {
            text: std::fs::read_to_string(path).ok(),
        }
    }

    #[cfg(test)]
    pub fn from_text(text: impl Into<String>) -> Source {
        Source {
            text: Some(text.into()),
        }
    }

    pub fn is_readable(&self) -> bool {
        self.text.is_some()
    }

    /// 0-based line without its line ending.
    pub fn line(&self, index: u32) -> Option<&str> {
        let line = self.text.as_deref()?.split('\n').nth(index as usize)?;
        Some(line.strip_suffix('\r').unwrap_or(line))
    }

    /// Position of a 1-based `line`/byte `col` of the graph, as `(line, column)` of the client.
    /// Without the text the byte column is all there is.
    pub fn position(&self, line: u32, col: u32, enc: Encoding) -> (u32, u32) {
        let (line, byte) = (line.saturating_sub(1), col.saturating_sub(1));
        match self.line(line) {
            Some(text) => (line, enc.column(text, byte as usize)),
            None => (line, byte),
        }
    }

    /// End of `line` (0-based) in the client's encoding.
    pub fn line_end(&self, line: u32, enc: Encoding) -> u32 {
        self.line(line).map_or(0, |l| enc.column(l, l.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_in_utf16_count_units() {
        // `é` is 2 bytes and 1 unit; `😀` is 4 bytes and 2 units
        let line = "é😀x";
        assert_eq!(Encoding::Utf16.column(line, 0), 0);
        assert_eq!(Encoding::Utf16.column(line, 2), 1);
        assert_eq!(Encoding::Utf16.column(line, 6), 3);
        assert_eq!(Encoding::Utf8.column(line, 6), 6);
        // inside a character: floor to its start
        assert_eq!(Encoding::Utf16.column(line, 4), 1);
        assert_eq!(Encoding::Utf16.column(line, 99), 4);
    }

    #[test]
    fn byte_offset_inverts_column() {
        let line = "é😀x";
        for byte in [0, 2, 6, 7] {
            for enc in [Encoding::Utf8, Encoding::Utf16] {
                assert_eq!(enc.byte_offset(line, enc.column(line, byte)), byte);
            }
        }
    }

    #[test]
    fn lines_drop_carriage_returns() {
        let s = Source::from_text("a\r\nbb\n");
        assert_eq!(s.line(0), Some("a"));
        assert_eq!(s.line(1), Some("bb"));
        assert_eq!(s.line(2), Some(""));
        assert_eq!(s.line(3), None);
        assert_eq!(s.position(2, 2, Encoding::Utf16), (1, 1));
        assert_eq!(Source::default().position(2, 5, Encoding::Utf16), (1, 4));
    }
}
