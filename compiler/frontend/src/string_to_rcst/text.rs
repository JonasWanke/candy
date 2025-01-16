use super::{
    expression::{expression, ExpressionParsingOptions},
    literal::{closing_curly_brace, double_quote, newline, opening_curly_brace, single_quote},
    utils::parse_multiple,
    whitespace::whitespaces_and_newlines,
};
use crate::{
    cst::{CstError, CstKind},
    rcst::Rcst,
};
use itertools::Itertools;
use tracing::instrument;

// TODO: It might be a good idea to ignore text interpolations in patterns
#[instrument(level = "trace")]
pub fn text(input: &str, indentation: usize) -> Option<(&str, Rcst)> {
    let (input, opening_single_quotes) = parse_multiple(input, single_quote, None)?;
    let (mut input, opening_double_quote) = double_quote(input)?;

    let (new_input, mut opening_whitespace) =
        whitespaces_and_newlines(input, indentation + 1, false);

    // If the string does not contain any newlines, parse the whitespace in
    // front of the string as part of the string and not as trailing whitespace.
    // This fixes https://github.com/candy-lang/candy/issues/896.
    if opening_whitespace.iter().any(|it| it.is_newline()) {
        input = new_input;
    } else {
        opening_whitespace = vec![];
    }

    let (mut opening_whitespace, mut parts) = if let Some(second_newline_index) = opening_whitespace
        .iter()
        .enumerate()
        .filter(|(_, whitespace)| matches!(whitespace.kind, CstKind::Newline(_)))
        .map(|(i, _)| i)
        .nth(1)
    {
        let (first_whitespace, rest) = opening_whitespace.split_at(second_newline_index);
        (
            first_whitespace.to_vec(),
            convert_whitespace_into_text_newlines(rest.to_vec()),
        )
    } else {
        (opening_whitespace, vec![])
    };

    let closing = loop {
        // TODO Use higher indentation in multiline text
        if let Some((input_after_interpolation, interpolation)) =
            text_interpolation(input, indentation, opening_single_quotes.len() + 1)
        {
            input = input_after_interpolation;
            parts.push(interpolation);
        } else if let Some((input_after_part, part)) = text_part(input, opening_single_quotes.len())
        {
            input = input_after_part;
            parts.push(part);
        } else {
            let (input_after_whitespace, whitespace) =
                whitespaces_and_newlines(input, indentation + 1, false);
            input = input_after_whitespace;

            let mut whitespace_before_closing_quote = if let Some(last_newline_index) = whitespace
                .iter()
                .enumerate()
                .filter(|(_, whitespace)| matches!(whitespace.kind, CstKind::Newline(_)))
                .map(|(i, _)| i)
                .next_back()
            {
                let (whitespace, rest) = whitespace.split_at(last_newline_index);
                let mut newlines = convert_whitespace_into_text_newlines(whitespace.to_vec());
                parts.append(&mut newlines);
                rest.to_vec()
            } else {
                whitespace
            };

            // Allow closing quotes to have the same indentation level as the opening quotes
            let (input_after_whitespace, whitespace) = if newline(input).is_some() {
                whitespaces_and_newlines(input, indentation, false)
            } else {
                (input, Vec::new())
            };
            let closing_quote = if let Some((input_after_double_quote, closing_double_quote)) =
                double_quote(input_after_whitespace)
                && let Some((input_after_single_quotes, closing_single_quotes)) = parse_multiple(
                    input_after_double_quote,
                    single_quote,
                    Some((opening_single_quotes.len(), false)),
                ) {
                input = input_after_single_quotes;

                whitespace_before_closing_quote = if let Some(last_newline_index) = whitespace
                    .iter()
                    .enumerate()
                    .filter(|(_, whitespace)| matches!(whitespace.kind, CstKind::Newline(_)))
                    .map(|(i, _)| i)
                    .next_back()
                {
                    let (whitespace, rest) = whitespace.split_at(last_newline_index);
                    let mut newlines = convert_whitespace_into_text_newlines(whitespace.to_vec());
                    parts.append(&mut newlines);
                    rest.to_vec()
                } else {
                    let mut newlines =
                        convert_whitespace_into_text_newlines(whitespace_before_closing_quote);
                    parts.append(&mut newlines);
                    whitespace
                };

                Some(CstKind::ClosingText {
                    closing_double_quote: Box::new(closing_double_quote),
                    closing_single_quotes,
                })
            } else if !whitespace.is_empty() || newline(input).is_some() {
                Some(CstKind::Error {
                    unparsable_input: String::new(),
                    error: CstError::TextNotSufficientlyIndented,
                })
            } else if input.is_empty() {
                Some(CstKind::Error {
                    unparsable_input: String::new(),
                    error: CstError::TextNotClosed,
                })
            } else {
                None
            };

            if let Some(closing_quote) = closing_quote {
                if let Some(last) = parts.pop() {
                    parts.push(last.wrap_in_whitespace(whitespace_before_closing_quote));
                } else {
                    opening_whitespace.append(&mut whitespace_before_closing_quote);
                }
                break closing_quote;
            }
            let mut newlines =
                convert_whitespace_into_text_newlines(whitespace_before_closing_quote);
            parts.append(&mut newlines);
        }
    };

    Some((
        input,
        CstKind::Text {
            opening: Box::new(
                CstKind::OpeningText {
                    opening_single_quotes,
                    opening_double_quote: Box::new(opening_double_quote),
                }
                .wrap_in_whitespace(opening_whitespace),
            ),
            parts,
            closing: Box::new(closing.into()),
        }
        .into(),
    ))
}

#[instrument(level = "trace")]
fn text_interpolation(
    input: &str,
    indentation: usize,
    curly_brace_count: usize,
) -> Option<(&str, Rcst)> {
    let (input, mut opening_curly_braces) =
        parse_multiple(input, opening_curly_brace, Some((curly_brace_count, true)))?;

    let (input, whitespace) = whitespaces_and_newlines(input, indentation + 1, false);
    let last = opening_curly_braces.pop().unwrap();
    opening_curly_braces.push(last.wrap_in_whitespace(whitespace));

    let (input, mut expression) = expression(
        input,
        indentation + 1,
        ExpressionParsingOptions {
            allow_assignment: false,
            allow_call: true,
            allow_bar: true,
            allow_function: true,
        },
    )
    .unwrap_or((
        input,
        CstKind::Error {
            unparsable_input: String::new(),
            error: CstError::TextInterpolationMissesExpression,
        }
        .into(),
    ));

    let (input, whitespace) = whitespaces_and_newlines(input, indentation + 1, false);
    expression = expression.wrap_in_whitespace(whitespace);

    let (input, closing_curly_braces) =
        parse_multiple(input, closing_curly_brace, Some((curly_brace_count, false))).unwrap_or((
            input,
            vec![CstKind::Error {
                unparsable_input: String::new(),
                error: CstError::TextInterpolationNotClosed,
            }
            .into()],
        ));

    Some((
        input,
        CstKind::TextInterpolation {
            opening_curly_braces,
            expression: Box::new(expression),
            closing_curly_braces,
        }
        .into(),
    ))
}

#[instrument(level = "trace")]
fn text_part(mut input: &str, single_quotes_count: usize) -> Option<(&str, Rcst)> {
    let mut text_part = vec![];
    loop {
        let next_char = input.chars().next();
        // TODO Optimize this somehow
        if next_char.is_none()
            || newline(input).is_some()
            || parse_multiple(
                input,
                opening_curly_brace,
                Some((single_quotes_count + 1, true)),
            )
            .is_some()
            || double_quote(input)
                .and_then(|(input_after_double_quote, _)| {
                    parse_multiple(
                        input_after_double_quote,
                        single_quote,
                        Some((single_quotes_count, false)),
                    )
                })
                .is_some()
        {
            let text_part = text_part.iter().join("");
            break if text_part.is_empty() {
                None
            } else {
                Some((input, CstKind::TextPart(text_part).into()))
            };
        }

        let next_char = next_char.unwrap();
        input = &input[next_char.len_utf8()..];
        text_part.push(next_char);
    }
}

#[instrument(level = "trace")]
fn convert_whitespace_into_text_newlines(whitespace: Vec<Rcst>) -> Vec<Rcst> {
    let mut last_newline: Option<Rcst> = None;
    let mut whitespace_after_last_newline: Vec<Rcst> = vec![];
    let mut parts: Vec<Rcst> = vec![];
    for whitespace in whitespace
        .iter()
        .chain(std::iter::once(&CstKind::Newline("\n".to_string()).into()))
    {
        if let CstKind::Newline(newline) = whitespace.kind.clone() {
            if let Some(last_newline) = last_newline {
                parts.push(last_newline.wrap_in_whitespace(whitespace_after_last_newline));
                whitespace_after_last_newline = vec![];
            }
            last_newline = Some(CstKind::TextNewline(newline).into());
        } else {
            whitespace_after_last_newline.push(whitespace.clone());
        }
    }
    parts
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::string_to_rcst::utils::assert_rich_ir_snapshot;

    #[test]
    fn test_text() {
        assert_rich_ir_snapshot!(text("foo", 0), @"Nothing was parsed");
        assert_rich_ir_snapshot!(text("\" foobar \"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart " foobar "
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        // https://github.com/candy-lang/candy/issues/1016
        assert_rich_ir_snapshot!(text("\"\n  foo\n    bar\n\"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: TrailingWhitespace:
            child: OpeningText:
              opening_single_quotes:
              opening_double_quote: DoubleQuote
            whitespace:
              Newline "\n"
              Whitespace "  "
          parts:
            TextPart "foo"
            TrailingWhitespace:
              child: TextNewline "\n"
              whitespace:
                Whitespace "  "
            TrailingWhitespace:
              child: TextPart "  bar"
              whitespace:
                Newline "\n"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        // issue: https://github.com/candy-lang/candy/issues/1016
        assert_rich_ir_snapshot!(text("\"\n    text\n\"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: TrailingWhitespace:
            child: OpeningText:
              opening_single_quotes:
              opening_double_quote: DoubleQuote
            whitespace:
              Newline "\n"
              Whitespace "  "
          parts:
            TrailingWhitespace:
              child: TextPart "  text"
              whitespace:
                Newline "\n"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        assert_rich_ir_snapshot!(text("\"  foobar  \"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "  foobar  "
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        assert_rich_ir_snapshot!(text("\"  \n  foobar  \"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: TrailingWhitespace:
            child: OpeningText:
              opening_single_quotes:
              opening_double_quote: DoubleQuote
            whitespace:
              Whitespace "  "
              Newline "\n"
              Whitespace "  "
          parts:
            TextPart "foobar  "
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        assert_rich_ir_snapshot!(text(r#""foo" bar"#, 0), @r###"
        Remaining input: " bar"
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        // "foo
        //   bar"2
        assert_rich_ir_snapshot!(text("\"foo\n  bar\"2", 0), @r###"
        Remaining input: "2"
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo"
            TrailingWhitespace:
              child: TextNewline "\n"
              whitespace:
                Whitespace "  "
            TextPart "bar"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        // "
        //   foo
        // "
        assert_rich_ir_snapshot!(text("\"\n  foo\n\"2", 0), @r###"
        Remaining input: "2"
        Parsed: Text:
          opening: TrailingWhitespace:
            child: OpeningText:
              opening_single_quotes:
              opening_double_quote: DoubleQuote
            whitespace:
              Newline "\n"
              Whitespace "  "
          parts:
            TrailingWhitespace:
              child: TextPart "foo"
              whitespace:
                Newline "\n"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        //   "foo
        //   bar"
        assert_rich_ir_snapshot!(text("\"foo\n  bar\"2", 1), @r###"
        Remaining input: "
          bar"2"
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo"
          closing: Error:
            unparsable_input: ""
            error: TextNotSufficientlyIndented
        "###);
        assert_rich_ir_snapshot!(text("\"foo", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo"
          closing: Error:
            unparsable_input: ""
            error: TextNotClosed
        "###);
        assert_rich_ir_snapshot!(text("''\"foo\"'bar\"'' baz", 0), @r###"
        Remaining input: " baz"
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
              SingleQuote
              SingleQuote
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo"'bar"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
              SingleQuote
              SingleQuote
        "###);
        assert_rich_ir_snapshot!(text("\"foo {\"bar\"} baz\"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo "
            TextInterpolation:
              opening_curly_braces:
                OpeningCurlyBrace
              expression: Text:
                opening: OpeningText:
                  opening_single_quotes:
                  opening_double_quote: DoubleQuote
                parts:
                  TextPart "bar"
                closing: ClosingText:
                  closing_double_quote: DoubleQuote
                  closing_single_quotes:
              closing_curly_braces:
                ClosingCurlyBrace
            TextPart " baz"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        assert_rich_ir_snapshot!(text("'\"foo {\"bar\"} baz\"'", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
              SingleQuote
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo {"bar"} baz"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
              SingleQuote
        "###);
        assert_rich_ir_snapshot!(text("\"foo {  \"bar\" } baz\"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo "
            TextInterpolation:
              opening_curly_braces:
                TrailingWhitespace:
                  child: OpeningCurlyBrace
                  whitespace:
                    Whitespace "  "
              expression: TrailingWhitespace:
                child: Text:
                  opening: OpeningText:
                    opening_single_quotes:
                    opening_double_quote: DoubleQuote
                  parts:
                    TextPart "bar"
                  closing: ClosingText:
                    closing_double_quote: DoubleQuote
                    closing_single_quotes:
                whitespace:
                  Whitespace " "
              closing_curly_braces:
                ClosingCurlyBrace
            TextPart " baz"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        assert_rich_ir_snapshot!(
            text(
                "\"Some text with {'\"an interpolation containing {{\"an interpolation\"}}\"'}\"",
                0,
            ),
            @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "Some text with "
            TextInterpolation:
              opening_curly_braces:
                OpeningCurlyBrace
              expression: Text:
                opening: OpeningText:
                  opening_single_quotes:
                    SingleQuote
                  opening_double_quote: DoubleQuote
                parts:
                  TextPart "an interpolation containing "
                  TextInterpolation:
                    opening_curly_braces:
                      OpeningCurlyBrace
                      OpeningCurlyBrace
                    expression: Text:
                      opening: OpeningText:
                        opening_single_quotes:
                        opening_double_quote: DoubleQuote
                      parts:
                        TextPart "an interpolation"
                      closing: ClosingText:
                        closing_double_quote: DoubleQuote
                        closing_single_quotes:
                    closing_curly_braces:
                      ClosingCurlyBrace
                      ClosingCurlyBrace
                closing: ClosingText:
                  closing_double_quote: DoubleQuote
                  closing_single_quotes:
                    SingleQuote
              closing_curly_braces:
                ClosingCurlyBrace
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        assert_rich_ir_snapshot!(text("\"{ {2} }\"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextInterpolation:
              opening_curly_braces:
                TrailingWhitespace:
                  child: OpeningCurlyBrace
                  whitespace:
                    Whitespace " "
              expression: TrailingWhitespace:
                child: Function:
                  opening_curly_brace: OpeningCurlyBrace
                  parameters_and_arrow: None
                  body:
                    Int:
                      radix_prefix: None
                      value: 2
                      string: "2"
                  closing_curly_brace: ClosingCurlyBrace
                whitespace:
                  Whitespace " "
              closing_curly_braces:
                ClosingCurlyBrace
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        assert_rich_ir_snapshot!(text("\"{{2}}\"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "{"
            TextInterpolation:
              opening_curly_braces:
                OpeningCurlyBrace
              expression: Int:
                radix_prefix: None
                value: 2
                string: "2"
              closing_curly_braces:
                ClosingCurlyBrace
            TextPart "}"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        assert_rich_ir_snapshot!(text("\"foo {} baz\"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo "
            TextInterpolation:
              opening_curly_braces:
                OpeningCurlyBrace
              expression: Error:
                unparsable_input: ""
                error: TextInterpolationMissesExpression
              closing_curly_braces:
                ClosingCurlyBrace
            TextPart " baz"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
        assert_rich_ir_snapshot!(text("\"foo {\"bar\" baz\"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo "
            TextInterpolation:
              opening_curly_braces:
                OpeningCurlyBrace
              expression: Call:
                receiver: TrailingWhitespace:
                  child: Text:
                    opening: OpeningText:
                      opening_single_quotes:
                      opening_double_quote: DoubleQuote
                    parts:
                      TextPart "bar"
                    closing: ClosingText:
                      closing_double_quote: DoubleQuote
                      closing_single_quotes:
                  whitespace:
                    Whitespace " "
                arguments:
                  Identifier "baz"
                  Text:
                    opening: OpeningText:
                      opening_single_quotes:
                      opening_double_quote: DoubleQuote
                    parts:
                    closing: Error:
                      unparsable_input: ""
                      error: TextNotClosed
              closing_curly_braces:
                Error:
                  unparsable_input: ""
                  error: TextInterpolationNotClosed
          closing: Error:
            unparsable_input: ""
            error: TextNotClosed
        "###);
        assert_rich_ir_snapshot!(text("\"foo {\"bar\" \"a\"} baz\"", 0), @r###"
        Remaining input: ""
        Parsed: Text:
          opening: OpeningText:
            opening_single_quotes:
            opening_double_quote: DoubleQuote
          parts:
            TextPart "foo "
            TextInterpolation:
              opening_curly_braces:
                OpeningCurlyBrace
              expression: Call:
                receiver: TrailingWhitespace:
                  child: Text:
                    opening: OpeningText:
                      opening_single_quotes:
                      opening_double_quote: DoubleQuote
                    parts:
                      TextPart "bar"
                    closing: ClosingText:
                      closing_double_quote: DoubleQuote
                      closing_single_quotes:
                  whitespace:
                    Whitespace " "
                arguments:
                  Text:
                    opening: OpeningText:
                      opening_single_quotes:
                      opening_double_quote: DoubleQuote
                    parts:
                      TextPart "a"
                    closing: ClosingText:
                      closing_double_quote: DoubleQuote
                      closing_single_quotes:
              closing_curly_braces:
                ClosingCurlyBrace
            TextPart " baz"
          closing: ClosingText:
            closing_double_quote: DoubleQuote
            closing_single_quotes:
        "###);
    }
}
