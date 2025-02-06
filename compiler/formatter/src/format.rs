use crate::{
    existing_parentheses::ExistingParentheses,
    existing_whitespace::{
        ExistingWhitespace, TrailingWhitespace, TrailingWithIndentationConfig,
        WhitespacePositionInBody,
    },
    format_collection::{
        apply_trailing_comma_condition, format_collection, TrailingCommaCondition,
    },
    formatted_cst::FormattedCst,
    text_edits::TextEdits,
    width::{Indentation, SinglelineWidth, StringWidth, Width},
};
use candy_frontend::{
    cst::{Cst, CstError, CstKind, IntRadix, UnwrapWhitespaceAndComment},
    position::Offset,
};
use extension_trait::extension_trait;
use itertools::Itertools;
use traversal::dft_post_rev;

#[derive(Clone, Default)]
pub struct FormattingInfo {
    pub indentation: Indentation,

    // The fields below apply only for direct descendants.
    pub trailing_comma_condition: Option<TrailingCommaCondition>,
    pub is_single_expression_in_assignment_body: bool,
}
impl FormattingInfo {
    pub const fn with_indent(&self) -> Self {
        Self {
            indentation: self.indentation.with_indent(),
            trailing_comma_condition: None,
            is_single_expression_in_assignment_body: false,
        }
    }
    pub const fn with_dedent(&self) -> Self {
        Self {
            indentation: self.indentation.with_dedent(),
            trailing_comma_condition: None,
            is_single_expression_in_assignment_body: false,
        }
    }
    pub const fn with_trailing_comma_condition(&self, condition: TrailingCommaCondition) -> Self {
        Self {
            indentation: self.indentation,
            trailing_comma_condition: Some(condition),
            is_single_expression_in_assignment_body: false,
        }
    }
    pub const fn for_single_expression_in_assignment_body(&self) -> Self {
        Self {
            indentation: self.indentation.with_indent(),
            trailing_comma_condition: None,
            is_single_expression_in_assignment_body: true,
        }
    }
    pub fn resolve_for_expression_with_indented_lines(
        &self,
        previous_width: Width,
        first_line_extra_width: Width,
    ) -> Self {
        Self {
            indentation: if self.is_single_expression_in_assignment_body
                && previous_width.last_line_fits(self.indentation, first_line_extra_width)
            {
                self.indentation.with_dedent()
            } else {
                self.indentation
            },
            trailing_comma_condition: None,
            is_single_expression_in_assignment_body: false,
        }
    }
}

pub fn format_csts<'a>(
    edits: &mut TextEdits,
    previous_width: Width,
    mut csts: &'a [Cst],
    fallback_offset: Offset,
    info: &FormattingInfo,
) -> FormattedCst<'a> {
    let mut offset = fallback_offset;
    let mut width = Width::default();
    let mut formatted =
        FormattedCst::new(Width::default(), ExistingWhitespace::empty(fallback_offset));
    let mut expression_count = 0;
    loop {
        let (new_whitespace, rest) = split_leading_whitespace(offset, csts);
        csts = rest;
        new_whitespace.into_empty_and_move_comments_to(edits, &mut formatted.whitespace);

        // Expression
        let Some((expression, rest)) = csts.split_first() else {
            break;
        };
        csts = rest;

        let is_at_start = offset == fallback_offset;
        width += if is_at_start {
            formatted.into_trailing_with_indentation_detailed(
                edits,
                &TrailingWithIndentationConfig::Body {
                    position: WhitespacePositionInBody::Start,
                    indentation: info.indentation,
                },
            )
        } else {
            formatted.into_trailing_with_indentation_detailed(
                edits,
                &TrailingWithIndentationConfig::Body {
                    position: WhitespacePositionInBody::Middle,
                    indentation: info.indentation,
                },
            )
        };

        formatted = format_cst(edits, previous_width + width, expression, info);
        offset = formatted.whitespace.end_offset();
        expression_count += 1;
    }

    width += formatted.child_width();
    if expression_count > 1 {
        width = width.without_first_line_width();
    }

    FormattedCst::new(width, formatted.whitespace)
}

fn split_leading_whitespace(start_offset: Offset, csts: &[Cst]) -> (ExistingWhitespace, &[Cst]) {
    let first_expression_index = csts.iter().position(|cst| {
        !matches!(
            cst.kind,
            CstKind::Whitespace(_)
                | CstKind::Error {
                    error: CstError::TooMuchWhitespace,
                    ..
                }
                | CstKind::Newline(_)
                | CstKind::Comment { .. },
        )
    });
    let (leading_whitespace, rest) = first_expression_index.map_or_else(
        || (csts, [].as_slice()),
        |first_expression_index| csts.split_at(first_expression_index),
    );
    let leading_whitespace = ExistingWhitespace::new(start_offset, leading_whitespace);
    (leading_whitespace, rest)
}

/// The non-trivial cases usually work in three steps, though these are often not clearly separated:
///
/// 0. Lay out children, giving us a [`FormattedCst`] containing the child's width and their
///    [`ExistingWhitespace`]. In many places (e.g., [`CstKind::BinaryBar`] and [`CstKind::Call`]), we lay
///    out the right side as if a line break was necessary since that's the worst case.
/// 1. Check whether we fit in one or multiple lines (based on the [`previous_width`], child widths,
///    and whether there are comments).
/// 2. Tell each [`ExistingWhitespace`] (often through [`FormattedCst`]) whether it should be empty,
///    become a single space, or become a newline with indentation.
///
/// See the case of [`CstKind::StructAccess`] for a simple example and [`CstKind::Function`] for the
/// opposite.
///
/// [`previous_width`] is relevant for the minimum width that is reserved on the first line: E.g.,
/// when formatting the call within `foo | bar baz`, [`previous_width`] would indicate that a width of
/// two is reserved in the first line (for the bar and the space that follows it).
pub fn format_cst<'a>(
    edits: &mut TextEdits,
    previous_width: Width,
    cst: &'a Cst,
    info: &FormattingInfo,
) -> FormattedCst<'a> {
    let width = match &cst.kind {
        CstKind::EqualsSign | CstKind::Comma | CstKind::Dot | CstKind::Colon => {
            SinglelineWidth::from(1).into()
        }
        CstKind::ColonEqualsSign => SinglelineWidth::from(2).into(),
        CstKind::Bar
        | CstKind::OpeningParenthesis
        | CstKind::ClosingParenthesis
        | CstKind::OpeningBracket
        | CstKind::ClosingBracket
        | CstKind::OpeningCurlyBrace
        | CstKind::ClosingCurlyBrace => SinglelineWidth::from(1).into(),
        CstKind::Arrow => SinglelineWidth::from(2).into(),
        CstKind::SingleQuote | CstKind::DoubleQuote | CstKind::Percent | CstKind::Octothorpe => {
            SinglelineWidth::from(1).into()
        }
        CstKind::Whitespace(_) | CstKind::Newline(_) => {
            panic!("Whitespace and newlines should be handled separately.")
        }
        CstKind::Comment {
            octothorpe,
            comment,
        } => {
            let formatted_octothorpe = format_cst(edits, previous_width, octothorpe, info);
            assert!(formatted_octothorpe
                .min_width(info.indentation)
                .is_singleline());

            let trimmed_comment = comment.trim_end();
            edits.change(octothorpe.data.span.end..cst.data.span.end, trimmed_comment);

            formatted_octothorpe.into_empty_trailing(edits) + trimmed_comment.width()
        }
        CstKind::TrailingWhitespace { child, whitespace } => {
            let mut whitespace = ExistingWhitespace::new(child.data.span.end, whitespace);
            let child = format_cst(edits, previous_width, child, info);
            let child_width = child.into_empty_and_move_comments_to(edits, &mut whitespace);
            return FormattedCst::new(child_width, whitespace);
        }
        CstKind::Identifier(string) | CstKind::Symbol(string) => string.width(),
        CstKind::Int {
            radix_prefix,
            string,
            ..
        } => {
            if let Some((radix, radix_string)) = radix_prefix {
                let span_end = Offset(cst.data.span.start.0 + radix_string.len());
                let span = cst.data.span.start..span_end;
                match radix {
                    IntRadix::Binary => edits.change(span, "0b"),
                    IntRadix::Hexadecimal => {
                        edits.change(span, "0x");
                        edits.change(span_end..cst.data.span.end, string.to_uppercase());
                    }
                }
            }

            radix_prefix
                .as_ref()
                .map(|(_, string)| string.width())
                .unwrap_or_default()
                + string.width()
        }
        CstKind::OpeningText {
            opening_single_quotes,
            opening_double_quote,
        } => {
            // TODO: Format text
            let mut width = Width::default();
            for opening_single_quote in opening_single_quotes {
                width += format_cst(edits, previous_width + width, opening_single_quote, info)
                    .min_width(info.indentation);
            }
            width += format_cst(edits, previous_width + width, opening_double_quote, info)
                .min_width(info.indentation);
            width
        }
        CstKind::ClosingText {
            closing_double_quote,
            closing_single_quotes,
        } => {
            // TODO: Format text
            let mut width = format_cst(edits, previous_width, closing_double_quote, info)
                .min_width(info.indentation);
            for closing_single_quote in closing_single_quotes {
                width += format_cst(edits, previous_width + width, closing_single_quote, info)
                    .min_width(info.indentation);
            }
            width
        }
        CstKind::Text {
            opening,
            parts,
            closing,
        } => {
            let info = info.resolve_for_expression_with_indented_lines(
                previous_width,
                SinglelineWidth::PARENTHESIS.into(),
            );

            let opening = format_cst(edits, previous_width, opening, &info);
            let closing = format_cst(
                edits,
                Width::multiline(None, info.indentation.width()),
                closing,
                &info,
            );

            let (closing_width, whitespace) = closing.split();
            let quotes_width =
                info.indentation.width() + opening.min_width(info.indentation) + closing_width;

            let Some((last_part, first_parts)) = parts.split_last() else {
                return FormattedCst::new(
                    opening.into_empty_trailing(edits) + closing_width,
                    whitespace,
                );
            };
            let previous_width_for_lines =
                Width::multiline(None, info.indentation.with_indent().width());
            let mut first_parts_width = Width::default();
            for part in first_parts {
                first_parts_width += format_cst(
                    edits,
                    previous_width_for_lines + first_parts_width,
                    part,
                    &info,
                )
                .min_width(info.indentation);
            }

            let last_part = format_cst(edits, previous_width + first_parts_width, last_part, &info);
            let total_parts_width = first_parts_width + last_part.min_width(info.indentation);
            return if total_parts_width.is_singleline()
                && (quotes_width + total_parts_width).fits(info.indentation)
            {
                FormattedCst::new(
                    opening.into_empty_trailing(edits)
                        + first_parts_width
                        + last_part.into_empty_trailing(edits)
                        + closing_width,
                    whitespace,
                )
            } else {
                FormattedCst::new(
                    opening.into_trailing(
                        edits,
                        TrailingWhitespace::Indentation(info.indentation.with_indent()),
                    ) + first_parts_width
                        + last_part.into_trailing(
                            edits,
                            TrailingWhitespace::Indentation(info.indentation),
                        )
                        + closing_width,
                    whitespace,
                )
            };
        }
        CstKind::TextNewline(_) => {
            let whitespace = vec![cst.clone()];
            let whitespace: ExistingWhitespace<'_> =
                ExistingWhitespace::new(cst.data.span.start, &whitespace);
            FormattedCst::new(Width::default(), whitespace)
                .into_trailing(edits, TrailingWhitespace::Indentation(info.indentation))
        }
        CstKind::TextPart(text) => text.width(),
        CstKind::TextInterpolation {
            opening_curly_braces,
            expression,
            closing_curly_braces,
        } => {
            // TODO: Format text
            let mut width = Width::default();
            for opening_curly_brace in opening_curly_braces {
                width += format_cst(edits, previous_width + width, opening_curly_brace, info)
                    .min_width(info.indentation);
            }
            width += format_cst(edits, previous_width + width, expression, info)
                .min_width(info.indentation);
            for closing_curly_brace in closing_curly_braces {
                width += format_cst(edits, previous_width + width, closing_curly_brace, info)
                    .min_width(info.indentation);
            }
            width
        }
        CstKind::BinaryBar { left, bar, right } => {
            // Left
            let mut left =
                format_receiver(edits, previous_width, left, info, ReceiverParent::BinaryBar);

            // Bar
            let width_for_right_side = Width::multiline(None, info.indentation.width());
            let bar_width = format_cst(edits, width_for_right_side, bar, info)
                .into_space_and_move_comments_to(edits, &mut left.whitespace);
            let left_min_width = left.min_width(info.indentation);

            // Right
            let (right_width, whitespace) = {
                let (right, right_parentheses) = ExistingParentheses::split_from(edits, right);
                // Depending on the precedence of `right` and whether there's an opening parenthesis
                // with a comment, we might be able to remove the parentheses. However, we won't insert
                // any by ourselves.
                let right_needs_parentheses = match right.precedence() {
                    Some(PrecedenceCategory::High) => {
                        right_parentheses.are_required_due_to_comments()
                    }
                    Some(PrecedenceCategory::Low) | None => right_parentheses.is_some(),
                };
                let (previous_width_for_right, info_for_right) = if right_needs_parentheses {
                    (
                        width_for_right_side
                            + bar_width
                            + SinglelineWidth::PARENTHESIS
                            + SinglelineWidth::PARENTHESIS,
                        info.with_indent(),
                    )
                } else {
                    (width_for_right_side + bar_width, info.clone())
                };
                let right = format_cst(edits, previous_width_for_right, right, &info_for_right);
                if right_needs_parentheses {
                    assert!(right_parentheses.is_some());
                    right_parentheses.into_some(
                        edits,
                        previous_width
                            + left_min_width
                            + SinglelineWidth::SPACE
                            + bar_width
                            + SinglelineWidth::SPACE,
                        right,
                        info,
                    )
                } else {
                    right_parentheses.into_none(edits, right)
                }
                .split()
            };

            let left_width = if let Some(right_first_line_width) = right_width.first_line_width()
                && (left_min_width + SinglelineWidth::SPACE + bar_width + right_first_line_width)
                    .fits(info.indentation)
            {
                left.into_trailing_with_space(edits)
            } else {
                left.into_trailing_with_indentation(edits, info.indentation)
            };

            return FormattedCst::new(left_width + bar_width + right_width, whitespace);
        }
        CstKind::Parenthesized { .. } => {
            // Whenever parentheses are necessary, they are handled by the parent. Hence, we try to
            // remove them here.
            let (child, parentheses) = ExistingParentheses::split_from(edits, cst);
            assert!(parentheses.is_some());

            return if parentheses.are_required_due_to_comments() {
                let child = format_cst(
                    edits,
                    Width::multiline(None, info.indentation.with_indent().width()),
                    child,
                    &info.with_indent(),
                );
                parentheses.into_some(edits, previous_width, child, info)
            } else {
                let child = format_cst(edits, previous_width, child, info);
                parentheses.into_none(edits, child)
            };
        }
        CstKind::Call {
            receiver,
            arguments,
        } => {
            let receiver =
                format_receiver(edits, previous_width, receiver, info, ReceiverParent::Call);
            if arguments.is_empty() {
                return receiver;
            }

            // Arguments
            let previous_width_for_arguments = Width::multiline(None, info.indentation.width());
            let last_argument_index = arguments.len() - 1;
            let mut arguments = arguments
                .iter()
                .enumerate()
                .map(|(index, argument)| {
                    Argument::new(
                        edits,
                        previous_width_for_arguments,
                        argument,
                        &info.with_indent(),
                        index == last_argument_index,
                    )
                })
                .collect_vec();

            let min_width = receiver.min_width(info.indentation)
                + arguments
                    .iter()
                    .map(|it| SinglelineWidth::SPACE + it.min_singleline_width())
                    .sum::<Width>();
            let (is_singleline, argument_info, trailing) =
                if previous_width.last_line_fits(info.indentation, min_width) {
                    (true, info.clone(), TrailingWhitespace::Space)
                } else {
                    (
                        false,
                        info.with_indent(),
                        TrailingWhitespace::Indentation(info.indentation.with_indent()),
                    )
                };

            let width = receiver.into_trailing(edits, trailing);

            let last_argument = arguments.pop().unwrap();
            let width = arguments.into_iter().fold(width, |old_width, argument| {
                let argument = argument.format(
                    edits,
                    previous_width + old_width,
                    &argument_info,
                    is_singleline,
                );
                let width = if is_singleline {
                    argument.into_trailing_with_space(edits)
                } else {
                    argument.into_trailing_with_indentation(edits, argument_info.indentation)
                };
                old_width + width
            });
            let last_argument_is_sandwich_like = matches!(
                &last_argument.argument,
                MaybeSandwichLikeArgument::SandwichLike(_)
            );
            let info_for_last_argument =
                if info.is_single_expression_in_assignment_body && is_singleline {
                    argument_info.with_dedent()
                } else {
                    argument_info
                };
            let (last_argument_width, whitespace) = last_argument
                .format(
                    edits,
                    previous_width + width,
                    &info_for_last_argument,
                    is_singleline,
                )
                .split();

            let mut width = width + last_argument_width;
            if !is_singleline && !last_argument_is_sandwich_like {
                width = width.without_first_line_width();
            }

            return FormattedCst::new(width, whitespace);
        }
        CstKind::List {
            opening_parenthesis,
            items,
            closing_parenthesis,
        } => {
            return format_collection(
                edits,
                previous_width,
                opening_parenthesis,
                items,
                closing_parenthesis,
                true,
                info,
            );
        }
        CstKind::ListItem { value, comma } => {
            let value_end = value.data.span.end;
            let value = format_cst(edits, previous_width, value, info);

            let (comma_width, mut whitespace) = apply_trailing_comma_condition(
                edits,
                previous_width + value.child_width(),
                comma.as_deref(),
                value_end,
                info,
                value.min_width(info.indentation),
            );

            return FormattedCst::new(
                value.into_empty_and_move_comments_to(edits, &mut whitespace) + comma_width,
                whitespace,
            );
        }
        CstKind::Struct {
            opening_bracket,
            fields,
            closing_bracket,
        } => {
            return format_collection(
                edits,
                previous_width,
                opening_bracket,
                fields,
                closing_bracket,
                false,
                info,
            );
        }
        CstKind::StructField {
            key_and_colon,
            value,
            comma,
        } => {
            let key_width_and_colon = key_and_colon.as_deref().map(|(key, colon)| {
                let key = format_cst(edits, previous_width, key, &info.with_indent());
                let mut colon = format_cst(
                    edits,
                    previous_width + key.child_width(),
                    colon,
                    &info.with_indent(),
                );
                (
                    key.into_empty_and_move_comments_to(edits, &mut colon.whitespace),
                    colon,
                )
            });

            let value_end = value.data.span.end;
            let previous_width_for_value = if key_and_colon.is_some() {
                Width::multiline(None, info.indentation.with_indent().width())
            } else {
                previous_width
            };
            let value = format_cst(edits, previous_width_for_value, value, &info.with_indent());

            let key_and_colon_min_width = key_width_and_colon
                .as_ref()
                .map(|(key_width, colon)| *key_width + colon.min_width(info.indentation))
                .unwrap_or_default();
            let (comma_width, mut whitespace) = apply_trailing_comma_condition(
                edits,
                previous_width_for_value + value.child_width(),
                comma.as_deref(),
                value_end,
                info,
                key_and_colon_min_width + value.min_width(info.indentation),
            );
            let value_width = value.into_empty_and_move_comments_to(edits, &mut whitespace);
            let min_width = key_and_colon_min_width + value_width + comma_width;

            return FormattedCst::new(
                key_width_and_colon
                    .map(|(key_width, colon)| {
                        let colon_trailing = if min_width.fits(info.indentation) {
                            TrailingWhitespace::Space
                        } else {
                            TrailingWhitespace::Indentation(info.indentation.with_indent())
                        };
                        key_width + colon.into_trailing(edits, colon_trailing)
                    })
                    .unwrap_or_default()
                    + value_width
                    + comma_width,
                whitespace,
            );
        }
        CstKind::StructAccess { struct_, dot, key } => {
            let (struct_, struct_parentheses) = ExistingParentheses::split_from(edits, struct_);
            let struct_needs_parentheses = match struct_.precedence() {
                Some(PrecedenceCategory::High) => struct_parentheses.are_required_due_to_comments(),
                Some(PrecedenceCategory::Low) => true,
                None => struct_parentheses.is_some(),
            };
            let (previous_width_for_struct, info_for_struct) = if struct_needs_parentheses {
                let previous_width_for_struct = if struct_parentheses.are_required_due_to_comments()
                {
                    Width::multiline(None, info.indentation.with_indent().width())
                } else {
                    previous_width + SinglelineWidth::PARENTHESIS
                };
                (previous_width_for_struct, info.with_indent())
            } else {
                (previous_width, info.clone())
            };
            let struct_ = format_cst(edits, previous_width_for_struct, struct_, &info_for_struct);
            let mut struct_ = if struct_needs_parentheses {
                struct_parentheses.into_some(edits, previous_width, struct_, info)
            } else {
                struct_parentheses.into_none(edits, struct_)
            };

            let previous_width_for_dot =
                Width::multiline(None, info.indentation.with_indent().width());
            let dot_width = format_cst(edits, previous_width_for_dot, dot, &info.with_indent())
                .into_empty_and_move_comments_to(edits, &mut struct_.whitespace);

            let key = format_cst(
                edits,
                previous_width_for_dot + dot_width,
                key,
                &info.with_indent(),
            );

            let min_width =
                struct_.min_width(info.indentation) + dot_width + key.min_width(info.indentation);
            let struct_trailing = if min_width.fits(info.indentation) {
                TrailingWhitespace::None
            } else {
                TrailingWhitespace::Indentation(info.indentation.with_indent())
            };

            let (key_width, whitespace) = key.split();
            return FormattedCst::new(
                struct_.into_trailing(edits, struct_trailing) + dot_width + key_width,
                whitespace,
            );
        }
        CstKind::Match {
            expression,
            percent,
            cases,
        } => {
            let expression = format_cst(edits, previous_width, expression, info);

            let previous_width_for_indented =
                Width::multiline(None, info.indentation.with_indent().width());
            let mut percent = format_cst(edits, previous_width_for_indented, percent, info);
            let expression_width =
                expression.into_space_and_move_comments_to(edits, &mut percent.whitespace);

            let only_has_empty_error_case = matches!(
                cases.as_slice(),
                [Cst {
                    kind: CstKind::Error {
                        unparsable_input,
                        error: CstError::MatchMissesCases,
                    },
                    ..
                }] if unparsable_input.is_empty(),
            );
            let (cases, last_case) =
                if !only_has_empty_error_case && let [cases @ .., last_case] = cases.as_slice() {
                    (cases, last_case)
                } else {
                    let (percent_width, whitespace) = percent.split();
                    return FormattedCst::new(expression_width + percent_width, whitespace);
                };

            let case_info = info
                .resolve_for_expression_with_indented_lines(
                    previous_width,
                    expression_width + SinglelineWidth::PERCENT,
                )
                .with_indent();
            let percent_width =
                percent.into_trailing_with_indentation(edits, case_info.indentation);

            let (last_case_width, whitespace) =
                format_cst(edits, previous_width_for_indented, last_case, &case_info).split();
            return FormattedCst::new(
                expression_width
                    + percent_width
                    + cases
                        .iter()
                        .map(|it| {
                            format_cst(edits, previous_width_for_indented, it, &case_info)
                                .into_trailing_with_indentation(edits, case_info.indentation)
                        })
                        .sum::<Width>()
                    + last_case_width,
                whitespace,
            );
        }
        CstKind::MatchCase {
            pattern,
            condition: _, // TODO: format match case conditions
            arrow,
            body,
        } => {
            let pattern = format_cst(edits, previous_width, pattern, info);

            let previous_width_for_arrow =
                Width::multiline(None, info.indentation.with_indent().width());
            let mut arrow = format_cst(edits, previous_width_for_arrow, arrow, info);
            let pattern_width =
                pattern.into_space_and_move_comments_to(edits, &mut arrow.whitespace);

            let (body_width, whitespace) = format_csts(
                edits,
                previous_width_for_arrow
                    + SinglelineWidth::SPACE
                    + arrow.min_width(info.indentation.with_indent()),
                body,
                arrow.whitespace.end_offset(),
                &info.with_indent(),
            )
            .split();

            let arrow_trailing = if pattern_width.last_line_fits(
                info.indentation,
                arrow.min_width(info.indentation) + SinglelineWidth::SPACE + body_width,
            ) {
                TrailingWhitespace::Space
            } else {
                TrailingWhitespace::Indentation(info.indentation.with_indent())
            };

            return FormattedCst::new(
                pattern_width + arrow.into_trailing(edits, arrow_trailing) + body_width,
                whitespace,
            );
        }
        CstKind::Function {
            opening_curly_brace,
            parameters_and_arrow,
            body,
            closing_curly_brace,
        } => {
            let info = info.resolve_for_expression_with_indented_lines(
                previous_width,
                SinglelineWidth::PARENTHESIS.into(),
            );

            let opening_curly_brace = format_cst(edits, previous_width, opening_curly_brace, &info);

            let previous_width_for_inner =
                Width::multiline(None, info.indentation.with_indent().width());
            let parameters_width_and_arrow =
                parameters_and_arrow.as_ref().map(|(parameters, arrow)| {
                    let mut parameters = parameters
                        .iter()
                        .map(|it| {
                            format_cst(edits, previous_width_for_inner, it, &info.with_indent())
                        })
                        .collect_vec();
                    let arrow =
                        format_cst(edits, previous_width_for_inner, arrow, &info.with_indent());

                    let parameters_trailing = if (opening_curly_brace.min_width(info.indentation)
                        + SinglelineWidth::SPACE
                        + parameters
                            .iter()
                            .map(|it| it.min_width(info.indentation) + SinglelineWidth::SPACE)
                            .sum::<Width>()
                        + arrow.min_width(info.indentation))
                    .fits(info.indentation)
                    {
                        TrailingWhitespace::Space
                    } else {
                        TrailingWhitespace::Indentation(info.indentation.with_indent())
                    };
                    let last_parameter = parameters.pop();
                    let parameters_width = parameters
                        .into_iter()
                        .map(|it| it.into_trailing(edits, parameters_trailing))
                        .sum::<Width>();

                    let last_parameter_width = last_parameter
                        .map(|it| {
                            // The arrow's comment can flow to the next line.
                            let trailing = if parameters_width.last_line_fits(
                                info.indentation,
                                it.min_width(info.indentation)
                                    + SinglelineWidth::SPACE
                                    + arrow.child_width(),
                            ) {
                                TrailingWhitespace::Space
                            } else {
                                TrailingWhitespace::Indentation(info.indentation.with_indent())
                            };
                            it.into_trailing(edits, trailing)
                        })
                        .unwrap_or_default();

                    (parameters_width + last_parameter_width, arrow)
                });

            let body_fallback_offset = parameters_width_and_arrow.as_ref().map_or_else(
                || opening_curly_brace.whitespace.end_offset(),
                |(_, arrow)| arrow.whitespace.end_offset(),
            );
            let body = format_csts(
                edits,
                previous_width_for_inner,
                body,
                body_fallback_offset,
                &info.with_indent(),
            );
            let (closing_curly_brace_width, whitespace) = format_cst(
                edits,
                Width::multiline(None, info.indentation.width()),
                closing_curly_brace,
                &info,
            )
            .split();

            let (parameters_and_arrow_min_width, arrow_has_comments) = parameters_width_and_arrow
                .as_ref()
                .map(|(parameters_width, arrow)| {
                    (
                        *parameters_width + arrow.child_width(),
                        arrow.whitespace.has_comments(),
                    )
                })
                .unwrap_or_default();
            let body_min_width = body.min_width(info.indentation);
            let width_until_arrow = opening_curly_brace.min_width(info.indentation)
                + SinglelineWidth::SPACE
                + parameters_and_arrow_min_width;

            // Opening curly brace
            let width_for_first_line = if parameters_and_arrow.is_some() {
                #[allow(clippy::redundant_clone)] // False positive
                width_until_arrow
            } else {
                width_until_arrow
                    + body_min_width
                    + SinglelineWidth::SPACE
                    + closing_curly_brace_width
            };
            let opening_curly_brace_trailing =
                if previous_width.last_line_fits(info.indentation, width_for_first_line) {
                    TrailingWhitespace::Space
                } else if body_min_width.is_empty() {
                    TrailingWhitespace::Indentation(info.indentation)
                } else {
                    TrailingWhitespace::Indentation(info.indentation.with_indent())
                };

            // Body
            let space_if_parameters = if parameters_width_and_arrow.is_some() {
                SinglelineWidth::SPACE
            } else {
                SinglelineWidth::default()
            };
            let space_if_body_not_empty = if body_min_width.is_empty() {
                SinglelineWidth::default()
            } else {
                SinglelineWidth::SPACE
            };
            let width_from_body =
                body_min_width + space_if_body_not_empty + closing_curly_brace_width;
            let body_trailing = if body.child_width().is_empty() {
                TrailingWhitespace::None
            } else if !arrow_has_comments
                && previous_width.last_line_fits(
                    info.indentation,
                    width_until_arrow + space_if_parameters + width_from_body,
                )
            {
                TrailingWhitespace::Space
            } else {
                TrailingWhitespace::Indentation(info.indentation)
            };

            // Parameters and arrow
            let parameters_and_arrow_width = parameters_width_and_arrow
                .map(|(parameters_width, arrow)| {
                    let arrow_trailing = if !arrow.whitespace.has_comments()
                        && width_until_arrow
                            .last_line_fits(info.indentation, space_if_parameters + width_from_body)
                    {
                        TrailingWhitespace::Space
                    } else {
                        TrailingWhitespace::Indentation(info.indentation.with_indent())
                    };
                    parameters_width + arrow.into_trailing(edits, arrow_trailing)
                })
                .unwrap_or_default();

            return FormattedCst::new(
                opening_curly_brace.into_trailing(edits, opening_curly_brace_trailing)
                    + parameters_and_arrow_width
                    + body.into_trailing(edits, body_trailing)
                    + closing_curly_brace_width,
                whitespace,
            );
        }
        CstKind::Assignment {
            left,
            assignment_sign,
            body,
        } => {
            let left_width =
                format_cst(edits, previous_width, left, info).into_trailing_with_space(edits);
            // TODO: move assignment sign to next line if it doesn't fit

            let previous_width_for_assignment_sign = previous_width + left_width;
            let assignment_sign = format_cst(
                edits,
                previous_width_for_assignment_sign,
                assignment_sign,
                &info.with_indent(),
            );

            let body_info = if body.len() == 1 {
                // Avoid double indentation for bodies/items/entries in trailing functions/lists/
                // structs.
                info.for_single_expression_in_assignment_body()
            } else {
                info.with_indent()
            };
            let (body_width, body_whitespace) = format_csts(
                edits,
                previous_width_for_assignment_sign
                    + assignment_sign.min_width(info.indentation)
                    + SinglelineWidth::SPACE,
                body,
                assignment_sign.whitespace.end_offset(),
                &body_info,
            )
            .split();
            let body_whitespace_has_comments = body_whitespace.has_comments();
            let body_whitespace_width = body_whitespace.into_trailing_with_indentation(
                edits,
                &TrailingWithIndentationConfig::Body {
                    position: WhitespacePositionInBody::End,
                    indentation: info.indentation.with_indent(),
                },
            );

            let contains_single_assignment = body.len() == 1
                && matches!(
                    body.first().unwrap().unwrap_whitespace_and_comment().kind,
                    CstKind::Assignment { .. },
                );
            let assignment_sign_trailing = if !contains_single_assignment
                && left_width.last_line_fits(
                    info.indentation,
                    assignment_sign.min_width(info.indentation)
                        + SinglelineWidth::SPACE
                        + body_width
                        + body_whitespace_width,
                ) {
                TrailingWhitespace::Space
            } else if !contains_single_assignment
                && !body_whitespace_has_comments
                && let Some(body_first_line_width) = body_width.first_line_width()
                && left_width.last_line_fits(
                    info.indentation,
                    assignment_sign.min_width(info.indentation)
                        + SinglelineWidth::SPACE
                        + body_first_line_width,
                )
            {
                TrailingWhitespace::Space
            } else {
                TrailingWhitespace::Indentation(info.indentation.with_indent())
            };

            left_width
                + assignment_sign.into_trailing(edits, assignment_sign_trailing)
                + body_width
                + body_whitespace_width
        }
        CstKind::Error {
            unparsable_input, ..
        } => unparsable_input.width(),
    };
    FormattedCst::new(width, ExistingWhitespace::empty(cst.data.span.end))
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ReceiverParent {
    BinaryBar,
    Call,
}
fn format_receiver<'a>(
    edits: &mut TextEdits,
    previous_width: Width,
    receiver: &'a Cst,
    info: &FormattingInfo,
    parent: ReceiverParent,
) -> FormattedCst<'a> {
    let (receiver, receiver_parentheses) = ExistingParentheses::split_from(edits, receiver);
    let receiver_needs_parentheses = match receiver.precedence() {
        Some(PrecedenceCategory::High) => receiver_parentheses.are_required_due_to_comments(),
        Some(PrecedenceCategory::Low) => match parent {
            ReceiverParent::BinaryBar => receiver_parentheses.are_required_due_to_comments(),
            ReceiverParent::Call => true,
        },
        None => receiver_parentheses.is_some(),
    };
    let previous_width_for_receiver = if receiver_needs_parentheses {
        previous_width + SinglelineWidth::PARENTHESIS + SinglelineWidth::PARENTHESIS
    } else {
        previous_width
    };
    let receiver = format_cst(edits, previous_width_for_receiver, receiver, info);

    if receiver_needs_parentheses {
        receiver_parentheses.into_some(edits, previous_width, receiver, info)
    } else {
        receiver_parentheses.into_none(edits, receiver)
    }
}

struct Argument<'a> {
    #[allow(clippy::struct_field_names)]
    argument: MaybeSandwichLikeArgument<'a>,
    precedence: Option<PrecedenceCategory>,
    parentheses: ExistingParentheses<'a>,
}
impl<'a> Argument<'a> {
    fn new(
        edits: &mut TextEdits,
        previous_width: Width,
        cst: &'a Cst,
        info: &FormattingInfo,
        is_last: bool,
    ) -> Self {
        let (argument, parentheses) = ExistingParentheses::split_from(edits, cst);
        let precedence = argument.precedence();

        let argument = if parentheses.are_required_due_to_comments() {
            let argument = format_cst(
                edits,
                previous_width,
                argument,
                &info.with_indent().with_indent(),
            );
            MaybeSandwichLikeArgument::Other {
                argument,
                min_singleline_width: Width::multiline(None, None),
            }
        } else if is_last && argument.is_sandwich_like() {
            MaybeSandwichLikeArgument::SandwichLike(argument)
        } else {
            let argument = format_cst(edits, previous_width, argument, info);
            let mut min_singleline_width = argument.min_width(info.indentation.with_indent());
            let parentheses_width = SinglelineWidth::PARENTHESIS + SinglelineWidth::PARENTHESIS;
            match precedence {
                Some(PrecedenceCategory::High) => {}
                Some(PrecedenceCategory::Low) => min_singleline_width += parentheses_width,
                None if parentheses.is_some() => min_singleline_width += parentheses_width,
                None => {}
            }
            MaybeSandwichLikeArgument::Other {
                argument,
                min_singleline_width,
            }
        };
        Argument {
            argument,
            precedence,
            parentheses,
        }
    }

    /// Width of the opening parenthesis / bracket / curly brace
    const SANDWICH_LIKE_MIN_SINGLELINE_WIDTH: SinglelineWidth = SinglelineWidth::PARENTHESIS;
    fn min_singleline_width(&self) -> Width {
        match &self.argument {
            MaybeSandwichLikeArgument::SandwichLike(_) => {
                Self::SANDWICH_LIKE_MIN_SINGLELINE_WIDTH.into()
            }
            MaybeSandwichLikeArgument::Other {
                min_singleline_width,
                ..
            } => *min_singleline_width,
        }
    }
    fn format(
        self,
        edits: &mut TextEdits,
        previous_width: Width,
        info: &FormattingInfo,
        is_singleline: bool,
    ) -> FormattedCst<'a> {
        let argument = match self.argument {
            MaybeSandwichLikeArgument::SandwichLike(it) => {
                format_cst(edits, previous_width, it, info)
            }
            MaybeSandwichLikeArgument::Other { argument, .. } => argument,
        };

        let are_parentheses_necessary_due_to_precedence = match self.precedence {
            Some(PrecedenceCategory::High) => false,
            Some(PrecedenceCategory::Low) | None => true,
        };
        if self.parentheses.is_some() {
            // We already have parentheses …
            if is_singleline && are_parentheses_necessary_due_to_precedence
                || self.parentheses.are_required_due_to_comments()
            {
                // … and we actually need them.
                self.parentheses
                    .into_some(edits, previous_width, argument, info)
            } else {
                // … but we don't need them.
                self.parentheses.into_none(edits, argument)
            }
        } else {
            // We don't have parentheses …
            if is_singleline && are_parentheses_necessary_due_to_precedence {
                // … but we need them.
                self.parentheses
                    .into_some(edits, previous_width, argument, info)
            } else {
                // … and we don't need them.
                self.parentheses.into_none(edits, argument)
            }
        }
    }
}
enum MaybeSandwichLikeArgument<'a> {
    SandwichLike(&'a Cst),
    Other {
        argument: FormattedCst<'a>,
        min_singleline_width: Width,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrecedenceCategory {
    /// Literals, parenthesized, struct access, etc.
    High,

    /// Binary bar, call, and match
    Low,
}

#[extension_trait]
pub impl<D> CstExtension for Cst<D> {
    fn has_comments(&self) -> bool {
        dft_post_rev(self, |it| it.children().into_iter())
            .any(|(_, it)| matches!(it.kind, CstKind::Comment { .. }))
    }

    fn is_sandwich_like(&self) -> bool {
        matches!(
            &self.kind,
            CstKind::List { .. } | CstKind::Struct { .. } | CstKind::Function { .. },
        )
    }

    /// Used by the parent to determine whether parentheses are necessary around this expression.
    ///
    /// Returns `None` if the child isn't a full expression on its own (e.g., [CstKind::Dot]) or is
    /// an error. In these cases, parenthesized expressions should be kept parenthesized and vice
    /// versa.
    fn precedence(&self) -> Option<PrecedenceCategory> {
        match &self.kind {
            CstKind::EqualsSign
            | CstKind::Comma
            | CstKind::Dot
            | CstKind::Colon
            | CstKind::ColonEqualsSign
            | CstKind::Bar
            | CstKind::OpeningParenthesis
            | CstKind::ClosingParenthesis
            | CstKind::OpeningBracket
            | CstKind::ClosingBracket
            | CstKind::OpeningCurlyBrace
            | CstKind::ClosingCurlyBrace
            | CstKind::Arrow
            | CstKind::SingleQuote
            | CstKind::DoubleQuote
            | CstKind::Percent
            | CstKind::Octothorpe
            | CstKind::Whitespace(_)
            | CstKind::Newline(_)
            | CstKind::Comment { .. } => None,
            CstKind::TrailingWhitespace { child, .. } => child.precedence(),
            CstKind::Identifier(_) | CstKind::Symbol(_) | CstKind::Int { .. } => {
                Some(PrecedenceCategory::High)
            }
            CstKind::OpeningText { .. } | CstKind::ClosingText { .. } => None,
            CstKind::Text { .. } => Some(PrecedenceCategory::High),
            CstKind::TextNewline(_) | CstKind::TextPart(_) | CstKind::TextInterpolation { .. } => {
                None
            }
            CstKind::BinaryBar { .. } => Some(PrecedenceCategory::Low),
            CstKind::Parenthesized { .. } => Some(PrecedenceCategory::High),
            CstKind::Call { .. } => Some(PrecedenceCategory::Low),
            CstKind::List { .. } => Some(PrecedenceCategory::High),
            CstKind::ListItem { .. } => None,
            CstKind::Struct { .. } => Some(PrecedenceCategory::High),
            CstKind::StructField { .. } => None,
            CstKind::StructAccess { .. } => Some(PrecedenceCategory::High),
            CstKind::Match { .. } => Some(PrecedenceCategory::Low),
            CstKind::MatchCase { .. } => None,
            CstKind::Function { .. } => Some(PrecedenceCategory::High),
            CstKind::Assignment { .. } | CstKind::Error { .. } => None,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::Formatter;
    use candy_frontend::{rcst_to_cst::RcstsToCstsExt, string_to_rcst::parse_rcst};
    use itertools::Itertools;

    // Comments with code snippets display the formatted/expected version of the subsequent test,
    // excluding a trailing newline.
    //
    // They're present for multiline expressions for better readability. When multiple source
    // expressions have the same formatted version, there's only a comment in front of the first
    // test case.

    #[test]
    fn test_csts() {
        test(" ", "");
        test("foo", "foo\n");
        test("foo\n", "foo\n");
        test("'\x04\n", "'\x04\n");

        // Consecutive newlines

        // foo
        // bar
        test("foo\nbar", "foo\nbar\n");
        // foo
        //
        // bar
        test("foo\n\nbar", "foo\n\nbar\n");
        // foo
        //
        //
        // bar
        test("foo\n\n\nbar", "foo\n\n\nbar\n");
        test("foo\n\n\n\nbar", "foo\n\n\nbar\n");
        test("foo\n\n\n\n\nbar", "foo\n\n\nbar\n");
        // foo = bar
        //
        // baz
        test("foo =\n  bar\n\nbaz", "foo = bar\n\nbaz\n");
        // foo = bar
        //
        // # abc
        test("foo =\n  bar\n\n# abc", "foo = bar\n\n# abc\n");

        // Consecutive expressions

        // foo
        // bar
        // baz
        test("foo\nbar\nbaz", "foo\nbar\nbaz\n");
        // foo
        // bar
        test("foo\n bar", "foo\nbar\n");
        // foo
        //
        // bar
        test("foo\n \nbar", "foo\n\nbar\n");
        // foo
        test("foo ", "foo\n");

        // Leading newlines

        test(" \nfoo", "foo\n");
        test("  \nfoo", "foo\n");
        test(" \n  \n foo", "foo\n");

        // Trailing newlines

        test("foo\n ", "foo\n");
        test("foo\n  ", "foo\n");
        test("foo \n  ", "foo\n");
        test("foo\n\n", "foo\n");
        test("foo\n \n ", "foo\n");

        // Comments

        // # abc
        //
        // foo
        test("# abc\n\nfoo", "# abc\n\nfoo\n");
        // # abc
        // foo
        test("# abc\nfoo", "# abc\nfoo\n");
        // foo # abc
        test("foo# abc", "foo # abc\n");
        test("foo # abc", "foo # abc\n");
        test("foo # abc ", "foo # abc\n");
        // foo
        // # abc
        test("foo\n# abc", "foo\n# abc\n");
        test("foo\n # abc", "foo\n# abc\n");
        // # abc
        // # def
        test("# abc\n# def\n", "# abc\n# def\n");
        // # abc
        //
        // # def
        test("# abc\n\n# def\n", "# abc\n\n# def\n");
    }
    #[test]
    fn test_int() {
        // Binary
        test("0b10", "0b10\n");
        test("0B10100101", "0b10100101\n");

        // Decimal
        test("1", "1\n");
        test("123", "123\n");

        // Hexadecimal
        test("0x123", "0x123\n");
        test("0XDEADc0de", "0xDEADC0DE\n");
    }
    #[test]
    fn test_text() {
        // Empty
        test("\"\"", "\"\"\n");
        test("\"\n\"", "\"\"\n");
        test("\"\n  \n\"", "\"\"\n");

        // Single line
        test("\"\n  foo\"", "\"foo\"\n");
        test("\"foo{0}bar\"", "\"foo{0}bar\"\n");
        test(
            "\"loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooong Text\"", 
            "\"\n  loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooong Text\n\"\n"
        );
        test(
            "\"\n  loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooong Text\"", 
            "\"\n  loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooong Text\n\"\n"
        );
        test(
            "\"\n  loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooong Text\n\"", 
            "\"\n  loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooong Text\n\"\n"
        );

        // Multiple lines
        test("\"\n  foo\n  bar\"", "\"\n  foo\n  bar\n\"\n");
        test("\"\n  foo\n  bar\n\"", "\"\n  foo\n  bar\n\"\n");
        test("\"foo\n  bar\"", "\"\n  foo\n  bar\n\"\n");
        test("\"foo\n  bar\n\"", "\"\n  foo\n  bar\n\"\n");
        test("\"foo\n  {0}\n  bar\n\"", "\"\n  foo\n  {0}\n  bar\n\"\n");
    }
    #[test]
    fn test_binary_bar() {
        test("foo | bar", "foo | bar\n");
        test("foo|bar", "foo | bar\n");
        test("foo  |  bar", "foo | bar\n");
        test("foo\n\n|   bar", "foo | bar\n");
        test("foo | (bar)", "foo | bar\n");
        test("foo | (\n  bar\n)", "foo | bar\n");
        test("foo | (bar baz)", "foo | (bar baz)\n");
        test("foo | (bar | baz)", "foo | (bar | baz)\n");
        test("(foo bar) | baz", "foo bar | baz\n");
        test("(foo | bar) | baz", "foo | bar | baz\n");
        test(
            "looooooooooooooooooooooooooooooooongReceiver | (looooooooooooooooooooooooooooooooooooooooongFunction)",
            "looooooooooooooooooooooooooooooooongReceiver | looooooooooooooooooooooooooooooooooooooooongFunction\n",
        );
        // looooooooooooooooooooooooooooooooooooooooongReceiver
        // | looooooooooooooooooooooooooooooooooooooooongFunction
        test(
            "looooooooooooooooooooooooooooooooooooooooongReceiver | looooooooooooooooooooooooooooooooooooooooongFunction",
            "looooooooooooooooooooooooooooooooooooooooongReceiver\n| looooooooooooooooooooooooooooooooooooooooongFunction\n",
        );
        // foo
        // | looooooooooooooooooooooooooooooooooooooooongFunction0 looooooooooooooooooooooooooooongArgument0
        test(
            "foo | looooooooooooooooooooooooooooooooooooooooongFunction0 looooooooooooooooooooooooooooongArgument0",
            "foo\n| looooooooooooooooooooooooooooooooooooooooongFunction0 looooooooooooooooooooooooooooongArgument0\n",
        );
        // looooooooooooooooooooooooooooooooongReceiver
        // | looooooooooooooooooooooooooooooooongFunction longArgument
        test(
            "looooooooooooooooooooooooooooooooongReceiver | looooooooooooooooooooooooooooooooongFunction longArgument",
            "looooooooooooooooooooooooooooooooongReceiver\n| looooooooooooooooooooooooooooooooongFunction longArgument\n",
        );
        // looooooooooooooooooooooooooooooooongReceiver | looooooooooooooooooooooooooooooooongFunction0
        // | looooooooooooooooooooooooooooooooongFunction1
        test(
            "looooooooooooooooooooooooooooooooongReceiver | looooooooooooooooooooooooooooooooongFunction0 | looooooooooooooooooooooooooooooooongFunction1",
            "looooooooooooooooooooooooooooooooongReceiver | looooooooooooooooooooooooooooooooongFunction0\n| looooooooooooooooooooooooooooooooongFunction1\n",
        );
        // looooooooooooooooooooooooooooooooongReceiver
        // | looooooooooooooooooooooooooooooooongFunction0 longArgument0
        // | looooooooooooooooooooooooooooooooongFunction1 longArgument1 longArgument2
        test(
            "looooooooooooooooooooooooooooooooongReceiver | looooooooooooooooooooooooooooooooongFunction0 longArgument0 | looooooooooooooooooooooooooooooooongFunction1 longArgument1 longArgument2",
            "looooooooooooooooooooooooooooooooongReceiver\n| looooooooooooooooooooooooooooooooongFunction0 longArgument0\n| looooooooooooooooooooooooooooooooongFunction1 longArgument1 longArgument2\n",
        );
        // looooooooooooooooooooooooooooooooongReceiver
        // | looooooooooooooooooooooooooooooooongFunction
        //   longArgument0
        //   longArgument1
        //   longArgument2
        //   longArgument3
        test(
            "looooooooooooooooooooooooooooooooongReceiver | looooooooooooooooooooooooooooooooongFunction longArgument0 longArgument1 longArgument2 longArgument3",
            "looooooooooooooooooooooooooooooooongReceiver\n| looooooooooooooooooooooooooooooooongFunction\n  longArgument0\n  longArgument1\n  longArgument2\n  longArgument3\n",
        );
        // foo
        // | looooooooooooooooooooooooooooooooooooooooongFunction0 looooooooooooooooooooooooooooongArgument0
        // | function1
        test(
            "foo | looooooooooooooooooooooooooooooooooooooooongFunction0 looooooooooooooooooooooooooooongArgument0 | function1",
            "foo\n| looooooooooooooooooooooooooooooooooooooooongFunction0 looooooooooooooooooooooooooooongArgument0\n| function1\n",
        );
        // foo | bar {
        //   baz
        //   blub
        // }
        test(
            "foo | bar {\n  baz\n  blub\n}",
            "foo | bar {\n  baz\n  blub\n}\n",
        );
        // loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongReceiver longArgument
        // | function
        test(
            "loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongReceiver longArgument\n| function\n",
            "loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongReceiver longArgument\n| function\n",
        );

        // Comments

        test("foo | bar # abc", "foo | bar # abc\n");
        // foo # abc
        // | bar
        test("foo | # abc\n  bar", "foo # abc\n| bar\n");
        test("foo # abc\n| bar", "foo # abc\n| bar\n");
    }
    #[test]
    fn test_parenthesized() {
        test("(foo)", "foo\n");
        test(" ( foo ) ", "foo\n");
        test("(\n  foo)", "foo\n");
        test("(\n  foo\n)", "foo\n");
        test("( ( foo ) ) ", "foo\n");
        test(
            "(looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongItem)",
            "looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongItem\n",
        );
        test(
            "(\n  looooooooooooooooooooooooooooooooongReceiver loooooooooooooooooooooooooooooooooooooooooongArgument)",
            "looooooooooooooooooooooooooooooooongReceiver loooooooooooooooooooooooooooooooooooooooooongArgument\n",
        );
        test(
            "(looooooooooooooooooooooooooooooooongReceiver looooooooooooooooooooooooooooooooooooooooooongArgument)",
            "looooooooooooooooooooooooooooooooongReceiver looooooooooooooooooooooooooooooooooooooooooongArgument\n",
        );

        // Comments

        test("(foo) # abc", "foo # abc\n");
        test("(foo)# abc", "foo # abc\n");
        test("(foo# abc\n)", "foo # abc\n");
        test("(foo # abc\n)", "foo # abc\n");
        // ( # abc
        //   foo
        // )
        test("(# abc\n  foo)", "( # abc\n  foo\n)\n");
        test("(((# abc\n  foo)))", "( # abc\n  foo\n)\n");
        // ( # abc
        //   # def
        //   foo
        // )
        test(
            "(# abc\n  (# def\n    foo))",
            "( # abc\n  # def\n  foo\n)\n",
        );
    }
    #[test]
    fn test_call() {
        test("foo bar Baz", "foo bar Baz\n");
        test("foo   bar Baz ", "foo bar Baz\n");
        test("foo   bar Baz ", "foo bar Baz\n");
        // foo
        //   firstlooooooooooooooooooooooooooooooooongArgument
        //   secondlooooooooooooooooooooooooooooooooongArgument
        test(
            "foo firstlooooooooooooooooooooooooooooooooongArgument secondlooooooooooooooooooooooooooooooooongArgument",
            "foo\n  firstlooooooooooooooooooooooooooooooooongArgument\n  secondlooooooooooooooooooooooooooooooooongArgument\n",
        );
        // foo
        //   {
        //     bar
        //     baz
        //   }
        //   blub
        test(
            "foo { bar\n  baz\n} blub",
            "foo\n  {\n    bar\n    baz\n  }\n  blub\n",
        );

        // Parentheses

        test("foo (bar)", "foo bar\n");
        test("foo (bar baz)", "foo (bar baz)\n");
        test("foo\n  bar baz", "foo (bar baz)\n");
        // foo
        //   firstlooooooooooooooooooooooooooooooooongArgument secondlooooooooooooooooooooooooooooongArgument
        test(
            "foo (firstlooooooooooooooooooooooooooooooooongArgument secondlooooooooooooooooooooooooooooongArgument)",
            "foo\n  firstlooooooooooooooooooooooooooooooooongArgument secondlooooooooooooooooooooooooooooongArgument\n",
        );
        // foo
        //   ( # abc
        //     bar
        //   )
        test("foo (# abc\n  bar\n)", "foo\n  ( # abc\n    bar\n  )\n");
        test("needs (is foo) \"message\"", "needs (is foo) \"message\"\n");
        test("(foo bar) baz", "(foo bar) baz\n");
        test("(foo | bar) baz", "(foo | bar) baz\n");

        // Trailing sandwich-like

        test("foo{bar}", "foo { bar }\n");
        test("foo(bar,)", "foo (bar,)\n");
        test("foo[bar]", "foo [bar]\n");
        test("foo [\n  bar\n]", "foo [bar]\n");
        // foo {
        //   looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression
        // }
        test(
            "foo { looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression }",
            "foo {\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression\n}\n",
        );
        // looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression {
        //   foo
        // }
        test(
            "looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression { foo }",
            "looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression {\n  foo\n}\n",
        );
        // foo { bar ->
        //   looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression
        // }
        test(
            "foo { bar -> looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression }",
            "foo { bar ->\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression\n}\n",
        );
        // foo (
        //   looooooooooooooooooooooooongItem0,
        //   looooooooooooooooooooooooongItem1,
        //   looooooooooooooooooooooooongItem2,
        // )
        test(
            "foo (looooooooooooooooooooooooongItem0, looooooooooooooooooooooooongItem1, looooooooooooooooooooooooongItem2)",
            "foo (\n  looooooooooooooooooooooooongItem0,\n  looooooooooooooooooooooooongItem1,\n  looooooooooooooooooooooooongItem2,\n)\n",
        );
        // foo ( # abc
        //   item,
        // )
        test("foo (# abc\n  item,)", "foo ( # abc\n  item,\n)\n");
        // foo (
        //   # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment
        //   item,
        // )
        test(
            "foo (# looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment\n  item,)",
            "foo (\n  # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment\n  item,\n)\n",
        );

        // Comments

        // foo # abc
        //   bar
        //   Baz
        test("foo # abc\n  bar\n  Baz", "foo # abc\n  bar\n  Baz\n");
        // foo
        //   # abc
        //   bar
        //   Baz
        test("foo\n  # abc\n  bar\n  Baz", "foo\n  # abc\n  bar\n  Baz\n");
        // foo
        //   bar # abc
        //   Baz
        test("foo\n  bar # abc\n  Baz", "foo\n  bar # abc\n  Baz\n");
    }
    #[test]
    fn test_list() {
        // Empty

        test("(,)", "(,)\n");
        test(" ( , ) ", "(,)\n");
        test("(\n  , ) ", "(,)\n");
        test("(\n  ,\n) ", "(,)\n");

        // Single item

        test("(foo,)", "(foo,)\n");
        test("(foo,)\n", "(foo,)\n");
        test("(foo, )\n", "(foo,)\n");
        test("(foo ,)\n", "(foo,)\n");
        test("( foo, )\n", "(foo,)\n");
        test("(foo,)\n", "(foo,)\n");
        test("(\n  foo,\n)\n", "(foo,)\n");
        test("(\n  foo,\n)\n", "(foo,)\n");
        test(" ( foo , ) \n", "(foo,)\n");
        test(
            "(looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonoooooooongItem,)",
            "(looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonoooooooongItem,)\n",
        );
        // (
        //   looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonooooooooongItem,
        // )
        test(
            "(looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonooooooooongItem,)",
            "(\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonooooooooongItem,\n)\n",
        );

        // Multiple items

        test("(foo, bar)", "(foo, bar)\n");
        test("(foo, bar,)", "(foo, bar)\n");
        test("(foo, bar, baz)", "(foo, bar, baz)\n");
        test("(foo, bar, baz,)", "(foo, bar, baz)\n");
        test("( foo ,bar ,baz , )", "(foo, bar, baz)\n");
        test("(\n  foo,\n  bar,\n  baz,\n)", "(foo, bar, baz)\n");
        // (
        //   firstLooooooooooooooooooooooooooooooooooooongItem,
        //   secondLooooooooooooooooooooooooooooooooooooongItem,
        // )
        test(
            "(firstLooooooooooooooooooooooooooooooooooooongItem, secondLooooooooooooooooooooooooooooooooooooongItem)",
            "(\n  firstLooooooooooooooooooooooooooooooooooooongItem,\n  secondLooooooooooooooooooooooooooooooooooooongItem,\n)\n",
        );

        // Comments

        test("(foo,) # abc", "(foo,) # abc\n");
        test("(foo,)# abc", "(foo,) # abc\n");
        // (
        //   foo, # abc
        // )
        test("(foo,# abc\n)", "(\n  foo, # abc\n)\n");
        test("(foo, # abc\n)", "(\n  foo, # abc\n)\n");
        // ( # abc
        //   foo,
        // )
        test("(# abc\n  foo,)", "( # abc\n  foo,\n)\n");
        // (
        //   foo, # abc
        //   bar,
        // )
        test("(foo# abc\n  , bar,)", "(\n  foo, # abc\n  bar,\n)\n");
    }
    #[test]
    fn test_struct() {
        // Empty

        test("[]", "[]\n");
        test("[ ]", "[]\n");
        test("[\n]", "[]\n");

        // Single item

        test("[foo]", "[foo]\n");
        test("[foo ]", "[foo]\n");
        test("[\n  foo]", "[foo]\n");
        test("[\n  foo\n]", "[foo]\n");
        test("[foo: bar]", "[foo: bar]\n");
        test("[ foo :bar ] ", "[foo: bar]\n");
        test("[\n  foo:\n    bar,\n]", "[foo: bar]\n");
        test(
            "[looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonooooooooongItem]",
            "[looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonooooooooongItem]\n",
        );
        // [
        //   looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonoooooooooongItem,
        // ]
        test(
            "[looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonoooooooooongItem]",
            "[\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonoooooooooongItem,\n]\n",
        );
        test(
            "[\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonooongKey: value\n]",
            "[looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooonooongKey: value]\n",
        );
        // [
        //   looooooooooooooooooooooooonooooooooooooooooooongKey:
        //     looooooooooooooooooooooooonooooooooooooooooooongValue,
        // ]
        test(
            "[looooooooooooooooooooooooonooooooooooooooooooongKey: looooooooooooooooooooooooonooooooooooooooooooongValue]",
            "[\n  looooooooooooooooooooooooonooooooooooooooooooongKey:\n    looooooooooooooooooooooooonooooooooooooooooooongValue,\n]\n",
        );

        // Multiple items

        test("[foo: bar, baz]", "[foo: bar, baz]\n");
        test("[foo: bar, baz,]", "[foo: bar, baz]\n");
        test("[foo: bar, baz: blub,]", "[foo: bar, baz: blub]\n");
        test("[ foo :bar ,baz , ]", "[foo: bar, baz]\n");
        test("[\n  foo :\n    bar ,\n  baz ,\n]", "[foo: bar, baz]\n");
        test(
            "[item1, looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongKey: value]",
            "[\n  item1,\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongKey: value,\n]\n",
        );

        // Comments

        test("[foo] # abc", "[foo] # abc\n");
        test("[foo: bar] # abc", "[foo: bar] # abc\n");
        // [
        //   foo: bar, # abc
        // ]
        test("[foo: bar # abc\n]", "[\n  foo: bar, # abc\n]\n");
        // [
        //   foo: # abc
        //     bar,
        // ]
        test("[foo: # abc\n  bar\n]", "[\n  foo: # abc\n    bar,\n]\n");
        // [ # abc
        //   foo: bar,
        // ]
        test("[# abc\n  foo: bar]", "[ # abc\n  foo: bar,\n]\n");
        // [
        //   foo: bar, # abc
        //   baz,
        // ]
        test(
            "[foo: bar # abc\n  , baz]",
            "[\n  foo: bar, # abc\n  baz,\n]\n",
        );
    }
    #[test]
    fn test_struct_access() {
        test("foo.bar", "foo.bar\n");
        test("foo.bar.baz", "foo.bar.baz\n");
        test("foo . bar. baz .blub ", "foo.bar.baz.blub\n");
        // foo.firstlooooooooooooooooooooooooooooooooongArgument
        //   .secondlooooooooooooooooooooooooooooooooongArgument
        test(
            "foo.firstlooooooooooooooooooooooooooooooooongArgument.secondlooooooooooooooooooooooooooooooooongArgument",
            "foo.firstlooooooooooooooooooooooooooooooooongArgument\n  .secondlooooooooooooooooooooooooooooooooongArgument\n",
        );
        // foo
        //   .firstlooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongArgument
        //   .secondlooooooooooooooooooooooooooooooooongArgument
        test(
            "foo.firstlooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongArgument.secondlooooooooooooooooooooooooooooooooongArgument",
            "foo\n  .firstlooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongArgument\n  .secondlooooooooooooooooooooooooooooooooongArgument\n",
        );
        test("(use \"Foo\").bar", "(use \"Foo\").bar\n");

        // Comments

        // foo # abc
        //   .bar
        test("foo# abc\n  .bar", "foo # abc\n  .bar\n");
        test("foo # abc\n  .bar", "foo # abc\n  .bar\n");
        test("foo  # abc\n  .bar", "foo # abc\n  .bar\n");
        test("foo .# abc\n  bar", "foo # abc\n  .bar\n");
        test("foo . # abc\n  bar", "foo # abc\n  .bar\n");
        test("foo .bar# abc", "foo.bar # abc\n");
        test("foo .bar # abc", "foo.bar # abc\n");
    }
    #[test]
    fn test_match() {
        test("foo % ", "foo %\n");
        // foo %
        //   Foo -> Foo
        //   Bar -> Bar
        test(
            "foo %\n  Foo -> Foo\n  Bar -> Bar",
            "foo %\n  Foo -> Foo\n  Bar -> Bar\n",
        );
        test(
            "foo%\n  Foo->Foo\n\n  Bar  ->  Bar",
            "foo %\n  Foo -> Foo\n  Bar -> Bar\n",
        );
        // foo := bar %
        //   Baz -> Blub
        test(
            "foo := bar %\n  Baz -> Blub\n",
            "foo := bar %\n  Baz -> Blub\n",
        );

        // Comments
        // foo % # abc
        //   Bar -> Baz
        test("foo%# abc\n  Bar -> Baz", "foo % # abc\n  Bar -> Baz\n");
        // foo %
        //   Bar -> # abc
        //     Baz
        test(
            "foo %\n  Bar # abc\n  -> Baz",
            "foo %\n  Bar -> # abc\n    Baz\n",
        );
    }
    #[test]
    fn test_function() {
        // No parameters

        test("{}", "{ }\n");
        test("{ }", "{ }\n");
        test("{ foo }", "{ foo }\n");
        test("{\n  foo\n}", "{ foo }\n");
        // {
        //   foo
        //   bar
        // }
        test("{\n  foo\n  bar\n}", "{\n  foo\n  bar\n}\n");
        // {
        //   foo
        //
        //   bar
        // }
        test("{\n  foo\n \n  bar\n}", "{\n  foo\n\n  bar\n}\n");
        // {
        //   loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongBody
        // }
        test(
            "{ loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongBody }",
            "{\n  loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongBody\n}\n",
        );

        // Parameters

        test("{ foo -> }", "{ foo -> }\n");
        test("{ foo -> bar }", "{ foo -> bar }\n");
        // { parameter looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter ->
        //   foo
        // }
        test(
            "{ parameter looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter -> foo }",
            "{ parameter looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter ->\n  foo\n}\n",
        );
        // {
        //   parameter
        //   loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter -> foo
        // }
        test(
            "{ parameter loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter -> foo }",
            "{\n  parameter\n  loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter -> foo\n}\n",
        );
        // {
        //   parameter
        //   looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter ->
        //   foo
        // }
        test(
            "{ parameter looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter -> foo }",
            "{\n  parameter\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter ->\n  foo\n}\n",
        );
        // {
        //   parameter
        //   looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter
        //   -> foo
        // }
        test(
            "{ parameter looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter -> foo }",
            "{\n  parameter\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongParameter\n  -> foo\n}\n",
        );
        // { parameter ->
        //   looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongBody
        // }
        test(
            "{ parameter -> looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongBody\n}\n",
            "{ parameter ->\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongBody\n}\n",
        );

        // Comments

        test("{ # abc\n}", "{ # abc\n}\n");
        // {
        //   foo # abc
        // }
        test("{ foo # abc\n}", "{\n  foo # abc\n}\n");
        // { foo ->
        //   bar # abc
        // }
        test("{ foo -> bar # abc\n}", "{ foo ->\n  bar # abc\n}\n");
        // { foo -> # abc
        //   bar
        // }
        test("{ foo -> # abc\n  bar\n}", "{ foo -> # abc\n  bar\n}\n");
        // {
        //   foo # abc
        //   -> bar
        // }
        test(
            "{ foo# abc\n  ->\n  bar\n}",
            "{\n  foo # abc\n  -> bar\n}\n",
        );
        // { # abc
        //   foo ->
        //   bar
        // }
        test("{ # abc\n  foo ->\n  bar\n}", "{ # abc\n  foo -> bar\n}\n");
    }
    #[test]
    fn test_assignment() {
        // Simple assignment

        test("foo = bar", "foo = bar\n");
        test("foo=bar", "foo = bar\n");
        test("foo = bar", "foo = bar\n");
        test("foo =\n  bar ", "foo = bar\n");
        test("foo := bar", "foo := bar\n");
        // foo =
        //   bar
        //   baz
        test("foo =\n  bar\n  baz", "foo =\n  bar\n  baz\n");
        test(
            "foo = looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression",
            "foo = looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression\n",
        );
        // foo =
        //   looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression
        test(
            "foo = looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression",
            "foo =\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression\n",
        );
        // foo =
        //   bar = baz
        test("foo =\n  bar = baz", "foo =\n  bar = baz\n");
        // foo := {
        //   bar
        //   baz
        // }
        test("foo := {\n  bar\n  baz\n}", "foo := {\n  bar\n  baz\n}\n");
        // foo = bar {
        //   baz
        //   blub
        //
        test(
            "foo = bar {\n  baz\n  blub\n}",
            "foo = bar {\n  baz\n  blub\n}\n",
        );
        // looooooooooooooooongIdentifier =
        //   function
        //     loooooooooooooooooooooooooooooooooooooooooooooooongArgument
        test(
            "looooooooooooooooongIdentifier = function loooooooooooooooooooooooooooooooooooooooooooooooongArgument",
            "looooooooooooooooongIdentifier =\n  function\n    loooooooooooooooooooooooooooooooooooooooooooooooongArgument\n",
        );

        // Function definition

        test("foo bar=baz ", "foo bar = baz\n");
        test("foo\n  bar=baz ", "foo bar = baz\n");
        test("foo\n  bar\n  =\n  baz ", "foo bar = baz\n");
        // foo
        //   firstlooooooooooooooooooooooooooooooooongArgument
        //   secondlooooooooooooooooooooooooooooooooongArgument
        test(
            "foo firstlooooooooooooooooooooooooooooooooongArgument secondlooooooooooooooooooooooooooooooooongArgument = bar",
            "foo\n  firstlooooooooooooooooooooooooooooooooongArgument\n  secondlooooooooooooooooooooooooooooooooongArgument = bar\n",
        );
        // foo
        //   firstlooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongArgument =
        //   bar
        test(
            "foo firstlooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongArgument = bar",
            "foo\n  firstlooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongArgument =\n  bar\n",
        );
        // foo argument =
        //   looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression
        test(
            "foo argument = looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression\n",
            "foo argument =\n  looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongExpression\n",
        );

        // Comments

        test("foo = bar # abc\n", "foo = bar # abc\n");
        test("foo=bar# abc\n", "foo = bar # abc\n");
        // foo =
        //   bar # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment
        test(
            "foo = bar # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment\n",
            "foo =\n  bar # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment\n",
        );
        // foo =
        //   bar
        //   # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment
        test(
            "foo = bar # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment\n",
            "foo =\n  bar\n  # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment\n",
        );
        // foo :=
        //   # loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment
        //   Foo
        //
        //   Bar
        test(
            "foo :=\n  # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment\n  Foo\n\n  Bar\n",
            "foo :=\n  # looooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooongComment\n  Foo\n\n  Bar\n",
        );
    }

    #[track_caller]
    fn test(source: &str, expected: &str) {
        let csts = parse_rcst(source).to_csts();
        assert_eq!(source, csts.iter().join(""));

        let formatted = csts.as_slice().format_to_string();
        assert_eq!(formatted, expected);
    }
}
