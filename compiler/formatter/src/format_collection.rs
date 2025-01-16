use crate::{
    existing_whitespace::{ExistingWhitespace, TrailingWhitespace},
    format::{format_cst, CstExtension, FormattingInfo},
    formatted_cst::FormattedCst,
    text_edits::TextEdits,
    width::{SinglelineWidth, Width},
};
use candy_frontend::{cst::Cst, position::Offset};
use itertools::Itertools;

pub fn format_collection<'a>(
    edits: &mut TextEdits,
    previous_width: Width,
    opening_punctuation: &Cst,
    items: &[Cst],
    closing_punctuation: &'a Cst,
    is_comma_required_for_single_item: bool,
    info: &FormattingInfo,
) -> FormattedCst<'a> {
    let (info, uses_sandwich_like_multiline_formatting) =
        info.resolve_for_sandwich_like(previous_width, SinglelineWidth::PARENTHESIS.into());

    let opening_punctuation = format_cst(edits, previous_width, opening_punctuation, &info);
    let closing_punctuation = format_cst(
        edits,
        Width::multiline(None, info.indentation.width()),
        closing_punctuation,
        &info,
    );

    let mut min_width = info.indentation.width()
        + opening_punctuation.min_width(info.indentation)
        + closing_punctuation.min_width(info.indentation);
    let previous_width_for_items = Width::multiline(None, info.indentation.with_indent().width());
    let item_info = info
        .with_indent()
        .with_trailing_comma_condition(TrailingCommaCondition::Always);
    let items = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let is_single_item = items.len() == 1;
            let is_last_item = index == items.len() - 1;

            let is_comma_required_due_to_single_item =
                is_single_item && is_comma_required_for_single_item;
            let is_comma_required =
                is_comma_required_due_to_single_item || !is_last_item || item.has_comments();
            let info = if !is_comma_required && let Width::Singleline(min_width) = min_width {
                // We're looking at the last item and everything might fit in one line.
                let max_width = Width::MAX - min_width;
                assert!(!max_width.is_empty());

                item_info
                    .with_trailing_comma_condition(TrailingCommaCondition::UnlessFitsIn(max_width))
            } else {
                item_info.clone()
            };
            let item = format_cst(edits, previous_width_for_items, item, &info);

            if let Width::Singleline(old_min_width) = min_width
                && let Width::Singleline(item_min_width) = item.min_width(info.indentation)
            {
                let (item_min_width, max_width) = if is_last_item {
                    (item_min_width, Width::MAX)
                } else {
                    // We need an additional column for the trailing space after the comma.
                    let item_min_width = item_min_width + SinglelineWidth::from(1);

                    // The last item needs at least one column of space.
                    let max_width = Width::MAX - SinglelineWidth::from(1);

                    (item_min_width, max_width)
                };
                min_width = Width::from_width_and_max(old_min_width + item_min_width, max_width);
            } else {
                min_width = Width::multiline(None, None);
            }

            item
        })
        .collect_vec();

    let (opening_punctuation_trailing, item_trailing, last_item_trailing) =
        if min_width.is_singleline() {
            (
                TrailingWhitespace::None,
                TrailingWhitespace::Space,
                TrailingWhitespace::None,
            )
        } else {
            (
                TrailingWhitespace::Indentation(info.indentation.with_indent()),
                TrailingWhitespace::Indentation(info.indentation.with_indent()),
                TrailingWhitespace::Indentation(info.indentation),
            )
        };

    let last_item_index = items.len().checked_sub(1);
    let (closing_punctuation_width, whitespace) = closing_punctuation.split();
    FormattedCst::new_maybe_sandwich_like_multiline_formatting(
        opening_punctuation.into_trailing(edits, opening_punctuation_trailing)
            + items
                .into_iter()
                .enumerate()
                .map(|(index, item)| {
                    item.into_trailing(
                        edits,
                        if last_item_index == Some(index) {
                            last_item_trailing
                        } else {
                            item_trailing
                        },
                    )
                })
                .sum::<Width>()
            + closing_punctuation_width,
        uses_sandwich_like_multiline_formatting,
        uses_sandwich_like_multiline_formatting,
        whitespace,
    )
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TrailingCommaCondition {
    Always,

    /// Add a trailing comma if the element fits in a single line and is at most
    /// this wide.
    UnlessFitsIn(SinglelineWidth),
}

pub fn apply_trailing_comma_condition<'a>(
    edits: &mut TextEdits,
    previous_width: Width,
    comma: Option<&'a Cst>,
    fallback_offset: Offset,
    info: &FormattingInfo,
    min_width_except_comma: Width,
) -> (Width, ExistingWhitespace<'a>) {
    let should_have_comma = match info.trailing_comma_condition {
        Some(TrailingCommaCondition::Always) => true,
        Some(TrailingCommaCondition::UnlessFitsIn(max_width)) => {
            !min_width_except_comma.fits_in(max_width)
        }
        None => comma.is_some(),
    };
    if should_have_comma {
        let whitespace = if let Some(comma) = comma {
            let comma = format_cst(edits, previous_width, comma, info);
            assert_eq!(comma.child_width(), SinglelineWidth::COMMA.into());
            comma.whitespace
        } else {
            edits.insert(fallback_offset, ",");
            ExistingWhitespace::empty(fallback_offset)
        };
        (SinglelineWidth::COMMA.into(), whitespace)
    } else if let Some(comma) = comma {
        if comma.has_comments() {
            // This last item can't fit on one line, so we do have to keep the comma.
            format_cst(edits, previous_width, comma, info).split()
        } else {
            edits.delete(comma.data.span.clone());
            (
                Width::default(),
                ExistingWhitespace::empty(comma.data.span.end),
            )
        }
    } else {
        (Width::default(), ExistingWhitespace::empty(fallback_offset))
    }
}

impl SinglelineWidth {
    const COMMA: Self = Self::new_const(1);
}
