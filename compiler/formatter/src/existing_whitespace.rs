use crate::{
    format::{format_cst, FormattingInfo},
    text_edits::TextEdits,
    width::{SinglelineWidth, Width},
    Indentation,
};
use candy_frontend::{
    cst::{Cst, CstError, CstKind},
    position::Offset,
};
use derive_more::From;
use itertools::Itertools;
use std::{borrow::Cow, num::NonZeroUsize};

#[derive(Clone, Copy, Debug, Eq, Hash, From, PartialEq)]
pub enum TrailingWhitespace {
    None,
    Space,
    Indentation(Indentation),
}

pub enum TrailingWithIndentationConfig {
    Body {
        position: WhitespacePositionInBody,
        indentation: Indentation,
    },
    Trailing {
        previous_width: Width,
        indentation: Indentation,
    },
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WhitespacePositionInBody {
    Start,
    Middle,
    End,
}

/// The maximum number of empty lines (i.e., containing no expression or comment) that may come
/// consecutively.
const MAX_CONSECUTIVE_EMPTY_LINES: usize = 2;
pub const SPACE: &str = " ";
pub const NEWLINE: &str = "\n";

/// Captures the existing trailing whitespace of CST nodes for later formatting.
///
/// The CST node ends at [`start_offset`], which is also where [`whitespace`] begins.
///
/// The three whitespace fields can contain singleline whitespace, linebreaks, and comments.
///
/// This struct also supports adoption: Whitespace can be “cut” from one place and “pasted” to
/// another. There are two use-cases for this:
///
/// - Move comments from an inner CST node to the parent CST node, where the actual whitespace stays
///   in the same place. E.g., the comma of a list item could contain trailing whitespace, which is
///   moved up and merged with potential trailing whitespace around the list item as a whole.
/// - Move comments to the other side of punctuation. E.g., a list item containing a comment between
///   value and comma (forcing the comma to be on a separate line) would move the trailing
///   whitespace of the value into trailing whitespace around the list item as a whole.
#[must_use]
#[derive(Clone, Debug)]
pub struct ExistingWhitespace<'a> {
    start_offset: Offset,
    adopted_whitespace_before: Cow<'a, [Cst]>,
    whitespace: Cow<'a, [Cst]>,
    adopted_whitespace_after: Cow<'a, [Cst]>,
}
impl<'a> ExistingWhitespace<'a> {
    pub const fn empty(start_offset: Offset) -> Self {
        Self {
            start_offset,
            adopted_whitespace_before: Cow::Borrowed(&[]),
            whitespace: Cow::Borrowed(&[]),
            adopted_whitespace_after: Cow::Borrowed(&[]),
        }
    }
    pub fn new(start_offset: Offset, whitespace: impl Into<Cow<'a, [Cst]>>) -> Self {
        let whitespace = whitespace.into();
        if whitespace.is_empty() {
            return Self::empty(start_offset);
        }

        Self {
            start_offset,
            adopted_whitespace_before: Cow::Borrowed(&[]),
            whitespace,
            adopted_whitespace_after: Cow::Borrowed(&[]),
        }
    }

    pub fn end_offset(&self) -> Offset {
        self.whitespace
            .as_ref()
            .last()
            .map_or(self.start_offset, |it| it.data.span.end)
    }
    pub fn is_empty(&self) -> bool {
        self.adopted_whitespace_before.is_empty()
            && self.whitespace.is_empty()
            && self.adopted_whitespace_after.is_empty()
    }
    pub fn whitespace_ref(&self) -> &[Cst] {
        self.whitespace.as_ref()
    }

    pub fn move_into_outer(self, outer: &mut ExistingWhitespace<'a>) {
        assert!(self.adopted_whitespace_before.is_empty());
        assert!(self.adopted_whitespace_after.is_empty());
        assert!(outer.adopted_whitespace_before.is_empty());
        assert!(outer.adopted_whitespace_after.is_empty());
        assert_eq!(self.end_offset(), outer.start_offset);

        outer.start_offset = self.start_offset;
        prepend(self.whitespace, &mut outer.whitespace);
    }
    pub fn into_space_and_move_comments_to(
        mut self,
        edits: &mut TextEdits,
        other: &mut ExistingWhitespace<'a>,
    ) {
        if let Some(whitespace) = self.whitespace.first()
            && whitespace.kind.is_whitespace()
        {
            let span = match &mut self.whitespace {
                Cow::Borrowed(whitespace) => {
                    let (first, remaining) = whitespace.split_first().unwrap();
                    *whitespace = remaining;
                    first.data.span.clone()
                }
                Cow::Owned(whitespace) => whitespace.remove(0).data.span,
            };
            self.start_offset = span.end;
            edits.change(span, SPACE);
        } else {
            edits.insert(self.start_offset, SPACE);
        }
        self.into_empty_and_move_comments_to(edits, other);
    }
    pub fn into_empty_and_move_comments_to(
        self,
        edits: &mut TextEdits,
        other: &mut ExistingWhitespace<'a>,
    ) {
        if self.is_empty() {
            return;
        }

        let self_end_offset = self.end_offset();
        if self_end_offset <= other.start_offset {
            if self_end_offset == other.start_offset
                && self.adopted_whitespace_before.is_empty()
                && self.adopted_whitespace_after.is_empty()
                && other.adopted_whitespace_before.is_empty()
                && !edits.has_edit_at(self_end_offset)
            {
                // Simple case: The whitespace is adopted by directly following whitespace.
                other.start_offset = self.start_offset;
                prepend(self.whitespace, &mut other.whitespace);
                prepend(self.adopted_whitespace_before, &mut other.whitespace);
                return;
            }

            // Default case: We have to delete the whitespace here and re-insert the relevant parts
            // (comments) later.
            if let Some(other_adopted_first) = &other.adopted_whitespace_before.first() {
                let other_adopted_start_offset = other_adopted_first.data.span.start;
                assert!(self_end_offset <= other_adopted_start_offset);
            }
            prepend(
                self.adopted_whitespace_after,
                &mut other.adopted_whitespace_before,
            );
            prepend(self.whitespace, &mut other.adopted_whitespace_before);
            prepend(
                self.adopted_whitespace_before,
                &mut other.adopted_whitespace_before,
            );
        } else {
            let other_end_offset = other
                .whitespace
                .last()
                .map_or(other.start_offset, |it| it.data.span.end);
            if self.start_offset == other_end_offset
                && other.adopted_whitespace_after.is_empty()
                && self.adopted_whitespace_before.is_empty()
                && self.adopted_whitespace_after.is_empty()
                && !edits.has_edit_at(self.start_offset)
            {
                // Simple case: The whitespace is adopted by directly preceding whitespace.
                append(self.whitespace, &mut other.whitespace);
                append(self.adopted_whitespace_after, &mut other.whitespace);
                return;
            }

            // Default case (see above)
            if let Some(other_adopted_last) = &other.adopted_whitespace_after.last() {
                let other_adopted_end_offset = other_adopted_last.data.span.end;
                assert!(other_adopted_end_offset <= self.start_offset);
            }
            append(
                self.adopted_whitespace_before,
                &mut other.adopted_whitespace_after,
            );
            append(self.whitespace, &mut other.adopted_whitespace_after);
            append(
                self.adopted_whitespace_after,
                &mut other.adopted_whitespace_after,
            );
        }
        edits.delete(self.start_offset..self_end_offset);
    }

    pub fn has_comments(&self) -> bool {
        fn check(whitespace: &[Cst]) -> bool {
            whitespace.iter().any(|it| it.kind.is_comment())
        }

        check(&self.adopted_whitespace_before)
            || check(&self.whitespace)
            || check(&self.adopted_whitespace_after)
    }

    pub fn into_empty_trailing(self, edits: &mut TextEdits) -> SinglelineWidth {
        assert!(!self.has_comments());

        for whitespace in self.whitespace_ref() {
            edits.delete(whitespace.data.span.clone());
        }

        SinglelineWidth::default()
    }
    #[must_use]
    pub fn into_trailing_with_space(self, edits: &mut TextEdits) -> SinglelineWidth {
        assert!(!self.has_comments());

        if let Some((first, last)) = first_and_last(self.whitespace.as_ref()) {
            edits.change(first.data.span.start..last.data.span.end, SPACE);
        } else {
            edits.insert(self.start_offset, SPACE);
        }
        SinglelineWidth::SPACE
    }

    #[must_use]
    pub fn into_trailing_with_indentation(
        self,
        edits: &mut TextEdits,
        config: &TrailingWithIndentationConfig,
    ) -> Width {
        fn iter_whitespace(
            whitespace: &[Cst],
            offset_override: impl Into<Option<Offset>>,
        ) -> impl Iterator<Item = (&Cst, Option<Offset>)> {
            let offset_override = offset_override.into();
            whitespace.iter().map(move |it| (it, offset_override))
        }

        // For adopted items, we need an offset override: The position where adopted comments will
        // be inserted.
        let whitespace = iter_whitespace(&self.adopted_whitespace_before, self.start_offset)
            .chain(iter_whitespace(&self.whitespace, None))
            .chain(iter_whitespace(
                &self.adopted_whitespace_after,
                self.end_offset(),
            ))
            .collect_vec();
        // `.chain(…)` doesn't produce an `ExactSizeIterator`, so it's easier to collect everything
        // into a `Vec` first.
        let last_comment_index = whitespace.iter().rposition(|(it, _)| it.kind.is_comment());
        let split_index = last_comment_index.map(|it| it + 1).unwrap_or_default();
        let (comments_and_whitespace, final_whitespace) = whitespace.split_at(split_index);

        let comments_width = Self::format_trailing_comments(edits, comments_and_whitespace, config);

        let owned_final_whitespace = final_whitespace
            .iter()
            .filter(|(_, offset_override)| offset_override.is_none())
            .map(|(it, _)| it);
        let trailing_range = if let Some((first, last)) = first_and_last(owned_final_whitespace) {
            first.data.span.start..last.data.span.end
        } else {
            let offset = self.end_offset();
            offset..offset
        };
        let (indentation, trailing_newline_count) = match config {
            TrailingWithIndentationConfig::Body {
                position: WhitespacePositionInBody::Start,
                ..
            } if comments_width.is_empty() => {
                edits.delete(trailing_range);
                return comments_width;
            }
            TrailingWithIndentationConfig::Body {
                position: WhitespacePositionInBody::End,
                indentation,
            } if indentation.is_indented() => {
                edits.delete(trailing_range);
                return comments_width;
            }
            TrailingWithIndentationConfig::Body {
                position: WhitespacePositionInBody::Start | WhitespacePositionInBody::Middle,
                indentation,
            } => {
                let trailing_newline_count = final_whitespace
                    .iter()
                    .filter(|(it, _)| it.kind.is_newline())
                    .count()
                    .clamp(1, 1 + MAX_CONSECUTIVE_EMPTY_LINES);
                (indentation, trailing_newline_count)
            }
            TrailingWithIndentationConfig::Trailing { indentation, .. }
            | TrailingWithIndentationConfig::Body { indentation, .. } => (indentation, 1),
        };
        edits.change(
            trailing_range,
            format!("{}{indentation}", NEWLINE.repeat(trailing_newline_count)),
        );
        comments_width + Width::NEWLINE + indentation.width()
    }
    fn format_trailing_comments(
        edits: &mut TextEdits,
        comments_and_whitespace: &[(&Cst, Option<Offset>)],
        config: &TrailingWithIndentationConfig,
    ) -> Width {
        enum NewlineCount {
            NoneOrAdopted,
            Owned(NonZeroUsize),
        }
        enum CommentPosition {
            FirstLine,
            NextLine(NewlineCount),
        }

        let (previous_width, indentation, ensure_space_before_first_comment, inner_newline_limit) =
            match config {
                TrailingWithIndentationConfig::Body {
                    indentation,
                    position,
                } => (
                    Width::Singleline(indentation.width()),
                    *indentation,
                    matches!(
                        position,
                        WhitespacePositionInBody::Middle | WhitespacePositionInBody::End,
                    ),
                    MAX_CONSECUTIVE_EMPTY_LINES,
                ),
                TrailingWithIndentationConfig::Trailing {
                    previous_width,
                    indentation,
                } => (*previous_width, *indentation, true, 1),
            };

        let mut width = Width::default();
        let mut comment_position = CommentPosition::FirstLine;
        let mut last_reusable_whitespace_range = None;

        for (item, offset_override) in comments_and_whitespace {
            let is_adopted = offset_override.is_some();
            match &item.kind {
                CstKind::Whitespace(_)
                | CstKind::Error {
                    error: CstError::TooMuchWhitespace,
                    ..
                } => {
                    if !is_adopted {
                        if let Some(range) = last_reusable_whitespace_range {
                            edits.delete(range);
                        }
                        last_reusable_whitespace_range = Some(item.data.span.clone());
                    }
                }
                CstKind::Newline(_) => match &mut comment_position {
                    CommentPosition::FirstLine => {
                        if let Some(range) = last_reusable_whitespace_range {
                            // Delete trailing spaces in the previous line.
                            edits.delete(range);
                            last_reusable_whitespace_range = None;
                        }

                        let newline_count = if is_adopted {
                            NewlineCount::NoneOrAdopted
                        } else {
                            edits.change(item.data.span.clone(), NEWLINE);
                            NewlineCount::Owned(NonZeroUsize::new(1).unwrap())
                        };

                        comment_position = CommentPosition::NextLine(newline_count);
                        width += Width::NEWLINE;
                    }
                    CommentPosition::NextLine(_) if is_adopted => {
                        // We already encountered a newline (owned or adopted) and the new
                        // one is adopted. Hence, we can't reuse it and there's nothing to
                        // do for us.
                    }
                    CommentPosition::NextLine(NewlineCount::NoneOrAdopted) => {
                        // The old newline was adopted or we didn't have one yet, but we now
                        // have one to reuse.
                        if let Some(range) = last_reusable_whitespace_range {
                            // Delete old reusable whitespace since the new one has to come
                            // after this newline.
                            edits.delete(range);
                            last_reusable_whitespace_range = None;
                        }

                        comment_position = CommentPosition::NextLine(NewlineCount::Owned(
                            NonZeroUsize::new(1).unwrap(),
                        ));
                    }
                    CommentPosition::NextLine(NewlineCount::Owned(count)) => {
                        // We already encountered and kept at least one newline.
                        if count.get() >= inner_newline_limit {
                            edits.delete(item.data.span.clone());
                        } else {
                            *count = count.checked_add(1).unwrap();
                            width += Width::NEWLINE;
                        }
                    }
                },
                CstKind::Comment { comment, .. } => {
                    let (comment_width, comment_whitespace) = format_cst(
                        edits,
                        previous_width,
                        item,
                        &FormattingInfo {
                            indentation,
                            trailing_comma_condition: None,
                            supports_sandwich_like_formatting: false,
                        },
                    )
                    .split();
                    assert!(comment_whitespace.is_empty());
                    _ = comment_whitespace;

                    let space = match comment_position {
                        CommentPosition::FirstLine => {
                            let (space, space_width) = if ensure_space_before_first_comment {
                                (Cow::Borrowed(SPACE), SinglelineWidth::SPACE)
                            } else {
                                (Cow::Borrowed(""), SinglelineWidth::default())
                            };
                            if previous_width
                                .last_line_fits(indentation, space_width + comment_width)
                                || matches!(
                                    config,
                                    TrailingWithIndentationConfig::Body {
                                        position: WhitespacePositionInBody::Start,
                                        ..
                                    },
                                )
                            {
                                width += Width::from(space_width);
                                space
                            } else {
                                width += Width::NEWLINE + indentation.width();
                                Cow::Owned(format!("{NEWLINE}{indentation}"))
                            }
                        }
                        CommentPosition::NextLine(newline_count) => {
                            match newline_count {
                                NewlineCount::NoneOrAdopted => {
                                    edits.insert(
                                        last_reusable_whitespace_range
                                            .as_ref()
                                            .map(|it| it.start)
                                            .or(*offset_override)
                                            .unwrap_or(item.data.span.start),
                                        NEWLINE,
                                    );
                                    width += Width::NEWLINE + indentation.width();
                                }
                                NewlineCount::Owned(_) => width += indentation.width(),
                            }
                            Cow::Owned(indentation.to_string())
                        }
                    };
                    if let Some(range) = last_reusable_whitespace_range {
                        edits.change(range, space);
                    } else {
                        edits.insert(offset_override.unwrap_or(item.data.span.start), space);
                    }

                    if let Some(offset_override) = offset_override {
                        edits.insert(*offset_override, format!("#{comment}"));
                    }

                    width += comment_width;
                    comment_position = CommentPosition::NextLine(NewlineCount::NoneOrAdopted);
                    last_reusable_whitespace_range = None;
                    // TODO: Handle multiple comments on the same line.
                }
                _ => unreachable!(),
            }
        }
        assert!(
            last_reusable_whitespace_range.is_none(),
            "The last CST must be a comment, so we should have consumed all whitespace.",
        );
        width
    }
}

fn append<'a>(source: Cow<'a, [Cst]>, target: &mut Cow<'a, [Cst]>) {
    if source.is_empty() {
        return;
    }

    if target.is_empty() {
        *target = source;
    } else {
        match source {
            Cow::Borrowed(source) => target.to_mut().extend_from_slice(source),
            Cow::Owned(mut source) => target.to_mut().append(&mut source),
        }
    }
}
fn prepend<'a>(source: Cow<'a, [Cst]>, target: &mut Cow<'a, [Cst]>) {
    if source.is_empty() {
        return;
    }

    if target.is_empty() {
        *target = source;
    } else {
        target
            .to_mut()
            .splice(0..0, source.as_ref().iter().cloned());
    }
}
fn first_and_last<I: IntoIterator>(
    iterator: I,
) -> Option<(<I as IntoIterator>::Item, <I as IntoIterator>::Item)>
where
    <I as IntoIterator>::Item: Copy,
{
    let mut result = None;
    for item in iterator {
        let first = result.map_or(item, |(first, _)| first);
        result = Some((first, item));
    }
    result
}

#[cfg(test)]
mod test {
    use super::TrailingWhitespace;
    use crate::{
        existing_whitespace::TrailingWithIndentationConfig,
        format::{format_cst, FormattingInfo},
        text_edits::TextEdits,
        width::{Indentation, Width},
    };
    use candy_frontend::{cst::CstKind, rcst_to_cst::RcstsToCstsExt, string_to_rcst::parse_rcst};

    #[test]
    fn test_empty_trailing() {
        test("foo End", TrailingWhitespace::None, "foo");
        test("foo  End", TrailingWhitespace::None, "foo");
    }

    #[test]
    fn test_trailing_with_space() {
        test("foo End", TrailingWhitespace::Space, "foo ");
        test("foo  End", TrailingWhitespace::Space, "foo ");
    }

    #[test]
    fn test_trailing_with_indentation() {
        test("foo\n  End", Indentation::from(1), "foo\n  ");
        test("foo \n  End", Indentation::from(1), "foo\n  ");
        test("foo End", Indentation::from(2), "foo\n    ");
        test("foo \n  End", Indentation::from(2), "foo\n    ");

        // Comments
        test("foo# abc\n  End", Indentation::from(1), "foo # abc\n  ");
        test("foo # abc\n  End", Indentation::from(1), "foo # abc\n  ");
        test("foo  # abc\n  End", Indentation::from(1), "foo # abc\n  ");
        test(
            "foo\n  # abc\n  End",
            Indentation::from(1),
            "foo\n  # abc\n  ",
        );
        test("foo # abc \n  End", Indentation::from(1), "foo # abc\n  ");
    }

    #[track_caller]
    fn test(source: &str, trailing: impl Into<TrailingWhitespace>, expected: &str) {
        let mut csts = parse_rcst(source).to_csts();
        assert_eq!(csts.len(), 1);

        let CstKind::Call { receiver: cst, .. } = csts.pop().unwrap().kind else {
            panic!("Expected a call");
        };
        let reduced_source = cst.to_string();

        let mut edits = TextEdits::new(reduced_source);
        let (child_width, whitespace) = format_cst(
            &mut edits,
            Width::default(),
            &cst,
            &FormattingInfo::default(),
        )
        .split();
        match trailing.into() {
            TrailingWhitespace::None => _ = whitespace.into_empty_trailing(&mut edits),
            TrailingWhitespace::Space => _ = whitespace.into_trailing_with_space(&mut edits),
            TrailingWhitespace::Indentation(indentation) => {
                _ = whitespace.into_trailing_with_indentation(
                    &mut edits,
                    &TrailingWithIndentationConfig::Trailing {
                        previous_width: child_width,
                        indentation,
                    },
                );
            }
        };
        assert_eq!(edits.apply(), expected);
    }
}
