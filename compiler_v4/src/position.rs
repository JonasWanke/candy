use derive_more::{Deref, DerefMut, From};
use extension_trait::extension_trait;
use serde::Serialize;
use std::{
    fmt::{self, Display, Formatter},
    ops::Range,
};
use unicode_segmentation::UnicodeSegmentation;

/// The offset of a character in a string as the number of bytes preceding it in
/// UTF-8 encoding.
#[derive(
    Clone, Copy, Debug, Default, Deref, DerefMut, Eq, From, Hash, Ord, PartialEq, PartialOrd,
)]
#[from(forward)]
pub struct Offset(pub usize);

impl Offset {
    pub fn to_position(mut self, source: &str) -> Position {
        if *self > source.len() {
            *self = source.len();
        }
        let line_start_offsets = line_start_offsets(source);

        let line = line_start_offsets
            .binary_search(&self)
            .unwrap_or_else(|i| i - 1);

        let character = source[*line_start_offsets[line]..*self]
            .graphemes(true)
            .count();
        Position { line, character }
    }
}

#[extension_trait]
pub impl RangeOfOffset for Range<Offset> {
    fn to_positions(&self, source: &str) -> Range<Position> {
        let start = self.start.to_position(source);
        let end = self.end.to_position(source);
        start..end
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Position {
    /// Zero-based line index (`\n`-separated)
    pub line: usize,
    /// Zero-based character index (counting grapheme clusters)
    pub character: usize,
}
impl Display for Position {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.line + 1, self.character + 1)
    }
}
#[extension_trait]
pub impl RangeOfPosition for Range<Position> {
    fn format(&self) -> String {
        format!("{} – {}", self.start, self.end)
    }
}

fn line_start_offsets<S: AsRef<str>>(text: S) -> Vec<Offset> {
    let mut offsets = vec![Offset(0)];
    offsets.extend(
        text.as_ref()
            .bytes()
            .enumerate()
            .filter(|(_, it)| it == &b'\n')
            .map(|(index, _)| Offset(index + 1)),
    );
    offsets
}
