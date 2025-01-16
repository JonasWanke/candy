use crate::{
    existing_whitespace::{ExistingWhitespace, TrailingWhitespace, TrailingWithIndentationConfig},
    text_edits::TextEdits,
    width::{Indentation, SinglelineWidth, Width},
};
use candy_frontend::cst::Cst;

pub struct UnformattedCst<'a> {
    pub child: &'a Cst,
    pub whitespace: ExistingWhitespace<'a>,
}

/// When a CST node is formatted, it returns a [`FormattedCst`] with its own width and whatever
/// trailing whitespace it contained.
///
/// The parent must later decide what to do with the trailing whitespace and call either of the
/// `into…` methods.
#[must_use]
pub struct FormattedCst<'a> {
    /// The minimum width that this CST node could take after formatting.
    ///
    /// If there are trailing comments, this is [Width::Multiline]. Otherwise, it's the child's own
    /// width.
    child_width: Width,

    /// Whether this CST node was formatted as a multiline sandwich-like.
    ///
    /// This means the previous width had sufficient space to fit the sandwich-like's opening
    /// character(s) (e.g., the opening parenthesis of a function call or the opening quote(s) of a
    /// text) and the rest of the sandwich-like expression is formatted over multiple lines.
    is_sandwich_like_multiline_formatting: bool,

    /// Whether this CST node is mostly singleline and ends with a CST node formatted as a multiline
    /// sandwich-like.
    ///
    /// For example, if the single expression of an assignment is a call with a trailing multiline
    /// list argument, it can start on the same line as the assignment but end on a new line.
    ends_with_sandwich_like_multiline_formatting: bool,

    pub whitespace: ExistingWhitespace<'a>,
}
impl<'a> FormattedCst<'a> {
    pub const fn new(child_width: Width, whitespace: ExistingWhitespace<'a>) -> Self {
        Self {
            child_width,
            is_sandwich_like_multiline_formatting: false,
            ends_with_sandwich_like_multiline_formatting: false,
            whitespace,
        }
    }
    pub const fn new_maybe_sandwich_like_multiline_formatting(
        child_width: Width,
        is_sandwich_like_multiline_formatting: bool,
        ends_with_sandwich_like_multiline_formatting: bool,
        whitespace: ExistingWhitespace<'a>,
    ) -> Self {
        Self {
            child_width,
            is_sandwich_like_multiline_formatting,
            ends_with_sandwich_like_multiline_formatting,
            whitespace,
        }
    }

    #[must_use]
    pub const fn child_width(&self) -> Width {
        self.child_width
    }
    #[must_use]
    pub fn min_width(&self, indentation: Indentation) -> Width {
        if self.whitespace.has_comments() {
            self.child_width + Width::multiline(SinglelineWidth::default(), indentation.width())
        } else {
            self.child_width
        }
    }

    #[must_use]
    pub const fn is_sandwich_like_multiline_formatting(&self) -> bool {
        self.is_sandwich_like_multiline_formatting
    }
    #[must_use]
    pub const fn ends_with_sandwich_like_multiline_formatting(&self) -> bool {
        self.ends_with_sandwich_like_multiline_formatting
    }

    pub fn split(self) -> (Width, ExistingWhitespace<'a>) {
        (self.child_width, self.whitespace)
    }

    #[must_use]
    pub fn into_space_and_move_comments_to(
        self,
        edits: &mut TextEdits,
        other: &mut ExistingWhitespace<'a>,
    ) -> Width {
        self.whitespace
            .into_space_and_move_comments_to(edits, other);
        self.child_width + SinglelineWidth::SPACE
    }
    #[must_use]
    pub fn into_empty_and_move_comments_to(
        self,
        edits: &mut TextEdits,
        other: &mut ExistingWhitespace<'a>,
    ) -> Width {
        self.whitespace
            .into_empty_and_move_comments_to(edits, other);
        self.child_width
    }

    #[must_use]
    pub fn into_trailing(
        self,
        edits: &mut TextEdits,
        trailing: impl Into<TrailingWhitespace>,
    ) -> Width {
        match trailing.into() {
            TrailingWhitespace::None => self.into_empty_trailing(edits),
            TrailingWhitespace::Space => self.into_trailing_with_space(edits),
            TrailingWhitespace::Indentation(indentation) => {
                self.into_trailing_with_indentation(edits, indentation)
            }
        }
    }
    #[must_use]
    pub fn into_empty_trailing(self, edits: &mut TextEdits) -> Width {
        self.child_width + self.whitespace.into_empty_trailing(edits)
    }
    #[must_use]
    pub fn into_trailing_with_space(self, edits: &mut TextEdits) -> Width {
        self.child_width + self.whitespace.into_trailing_with_space(edits)
    }
    #[must_use]
    pub fn into_trailing_with_indentation(
        self,
        edits: &mut TextEdits,
        indentation: Indentation,
    ) -> Width {
        self.into_trailing_with_indentation_detailed(
            edits,
            &TrailingWithIndentationConfig::Trailing {
                // TODO: Pass actual previous width
                previous_width: Width::default(),
                indentation,
            },
        )
    }
    #[must_use]
    pub fn into_trailing_with_indentation_detailed(
        self,
        edits: &mut TextEdits,
        config: &TrailingWithIndentationConfig,
    ) -> Width {
        let config = match config {
            TrailingWithIndentationConfig::Body {
                position,
                indentation,
            } => TrailingWithIndentationConfig::Body {
                position: *position,
                indentation: *indentation,
            },
            TrailingWithIndentationConfig::Trailing {
                previous_width,
                indentation,
            } => TrailingWithIndentationConfig::Trailing {
                previous_width: *previous_width + self.child_width,
                indentation: *indentation,
            },
        };
        let whitespace_width = self
            .whitespace
            .into_trailing_with_indentation(edits, &config);
        self.child_width + whitespace_width
    }
}
