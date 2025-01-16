use super::{
    expression::{expression, ExpressionParsingOptions},
    literal::{closing_bracket, colon, colon_equals_sign, comma, opening_bracket},
    whitespace::whitespaces_and_newlines,
};
use crate::{
    cst::{CstError, CstKind, IsMultiline},
    rcst::Rcst,
};
use tracing::instrument;

#[instrument(level = "trace")]
pub fn struct_(input: &str, indentation: usize, allow_function: bool) -> Option<(&str, Rcst)> {
    let (mut outer_input, mut opening_bracket) = opening_bracket(input)?;

    let mut fields: Vec<Rcst> = vec![];
    let mut fields_indentation = indentation;
    loop {
        let input = outer_input;

        // Whitespace before key.
        let (input, whitespace) = whitespaces_and_newlines(input, indentation + 1, true);
        if whitespace.is_multiline() {
            fields_indentation = indentation + 1;
        }
        if fields.is_empty() {
            opening_bracket = opening_bracket.wrap_in_whitespace(whitespace);
        } else {
            let last = fields.pop().unwrap();
            fields.push(last.wrap_in_whitespace(whitespace));
        }
        outer_input = input;

        // The key if it's explicit or the value when using a shorthand.
        let (input, key_or_value) = match expression(
            input,
            fields_indentation,
            ExpressionParsingOptions {
                allow_assignment: false,
                allow_call: true,
                allow_bar: true,
                allow_function,
            },
        ) {
            Some((input, key)) => (input, Some(key)),
            None => (input, None),
        };

        // Whitespace between key/value and colon.
        let (input, key_or_value_whitespace) =
            whitespaces_and_newlines(input, fields_indentation + 1, true);
        if key_or_value_whitespace.is_multiline() {
            fields_indentation = indentation + 1;
        }

        // Colon.
        let (input, colon, has_colon) = match colon(input) {
            Some((new_input, colon)) if colon_equals_sign(input).is_none() => {
                (new_input, colon, true)
            }
            _ => (
                input,
                CstKind::Error {
                    unparsable_input: String::new(),
                    error: CstError::StructFieldMissesColon,
                }
                .into(),
                false,
            ),
        };

        // Whitespace between colon and value.
        let (input, whitespace) = whitespaces_and_newlines(input, fields_indentation + 1, true);
        if whitespace.is_multiline() {
            fields_indentation = indentation + 1;
        }
        let colon = colon.wrap_in_whitespace(whitespace);

        // Value.
        let (input, value, has_value) = match expression(
            input,
            fields_indentation + 1,
            ExpressionParsingOptions {
                allow_assignment: false,
                allow_call: true,
                allow_bar: true,
                allow_function,
            },
        ) {
            Some((input, value)) => (input, value, true),
            None => (
                input,
                CstKind::Error {
                    unparsable_input: String::new(),
                    error: CstError::StructFieldMissesValue,
                }
                .into(),
                false,
            ),
        };

        // Whitespace between value and comma.
        let (input, whitespace) = whitespaces_and_newlines(input, fields_indentation + 1, true);
        if whitespace.is_multiline() {
            fields_indentation = indentation + 1;
        }
        let value = value.wrap_in_whitespace(whitespace);

        // Comma.
        let (input, comma) = match comma(input) {
            Some((input, comma)) => (input, Some(comma)),
            None => (input, None),
        };

        if key_or_value.is_none() && !has_value && comma.is_none() {
            break;
        }

        let is_using_shorthand = key_or_value.is_some() && !has_colon && !has_value;
        let key_or_value = key_or_value.unwrap_or_else(|| {
            CstKind::Error {
                unparsable_input: String::new(),
                error: if is_using_shorthand {
                    CstError::StructFieldMissesValue
                } else {
                    CstError::StructFieldMissesKey
                },
            }
            .into()
        });
        let key_or_value = key_or_value.wrap_in_whitespace(key_or_value_whitespace);

        outer_input = input;
        let comma = comma.map(Box::new);
        let field = if is_using_shorthand {
            CstKind::StructField {
                key_and_colon: None,
                value: Box::new(key_or_value),
                comma,
            }
        } else {
            CstKind::StructField {
                key_and_colon: Some(Box::new((key_or_value, colon))),
                value: Box::new(value),
                comma,
            }
        };
        fields.push(field.into());
    }
    let input = outer_input;

    let (new_input, whitespace) = whitespaces_and_newlines(input, indentation, true);

    let (input, closing_bracket) = match closing_bracket(new_input) {
        Some((input, closing_bracket)) => {
            if fields.is_empty() {
                opening_bracket = opening_bracket.wrap_in_whitespace(whitespace);
            } else {
                let last = fields.pop().unwrap();
                fields.push(last.wrap_in_whitespace(whitespace));
            }
            (input, closing_bracket)
        }
        None => (
            input,
            CstKind::Error {
                unparsable_input: String::new(),
                error: CstError::StructNotClosed,
            }
            .into(),
        ),
    };

    Some((
        input,
        CstKind::Struct {
            opening_bracket: Box::new(opening_bracket),
            fields,
            closing_bracket: Box::new(closing_bracket),
        }
        .into(),
    ))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::string_to_rcst::utils::assert_rich_ir_snapshot;

    #[test]
    fn test_struct() {
        assert_rich_ir_snapshot!(struct_("hello", 0, true), @"Nothing was parsed");
        assert_rich_ir_snapshot!(
            struct_("[ ]", 0, true),
            @r###"
        Remaining input: ""
        Parsed: Struct:
          opening_bracket: TrailingWhitespace:
            child: OpeningBracket
            whitespace:
              Whitespace " "
          fields:
          closing_bracket: ClosingBracket
        "###
        );
        assert_rich_ir_snapshot!(struct_("[ ]", 0, true), @r###"
        Remaining input: ""
        Parsed: Struct:
          opening_bracket: TrailingWhitespace:
            child: OpeningBracket
            whitespace:
              Whitespace " "
          fields:
          closing_bracket: ClosingBracket
        "###);
        assert_rich_ir_snapshot!(struct_("[foo:bar]", 0, true), @r###"
        Remaining input: ""
        Parsed: Struct:
          opening_bracket: OpeningBracket
          fields:
            StructField:
              key_and_colon:
                key: Identifier "foo"
                colon: Colon
              value: Identifier "bar"
              comma: None
          closing_bracket: ClosingBracket
        "###);
        assert_rich_ir_snapshot!(struct_("[foo,bar:baz]", 0, true), @r###"
        Remaining input: ""
        Parsed: Struct:
          opening_bracket: OpeningBracket
          fields:
            StructField:
              key_and_colon: None
              value: Identifier "foo"
              comma: Comma
            StructField:
              key_and_colon:
                key: Identifier "bar"
                colon: Colon
              value: Identifier "baz"
              comma: None
          closing_bracket: ClosingBracket
        "###);
        assert_rich_ir_snapshot!(struct_("[foo := [foo]", 0, true), @r###"
        Remaining input: ":= [foo]"
        Parsed: Struct:
          opening_bracket: OpeningBracket
          fields:
            StructField:
              key_and_colon: None
              value: TrailingWhitespace:
                child: Identifier "foo"
                whitespace:
                  Whitespace " "
              comma: None
          closing_bracket: Error:
            unparsable_input: ""
            error: StructNotClosed
        "###);
        // [
        //   foo: bar,
        //   4: "Hi",
        // ]
        assert_rich_ir_snapshot!(struct_("[\n  foo: bar,\n  4: \"Hi\",\n]", 0, true), @r###"
        Remaining input: ""
        Parsed: Struct:
          opening_bracket: TrailingWhitespace:
            child: OpeningBracket
            whitespace:
              Newline "\n"
              Whitespace "  "
          fields:
            TrailingWhitespace:
              child: StructField:
                key_and_colon:
                  key: Identifier "foo"
                  colon: TrailingWhitespace:
                    child: Colon
                    whitespace:
                      Whitespace " "
                value: Identifier "bar"
                comma: Comma
              whitespace:
                Newline "\n"
                Whitespace "  "
            TrailingWhitespace:
              child: StructField:
                key_and_colon:
                  key: Int:
                    radix_prefix: None
                    value: 4
                    string: "4"
                  colon: TrailingWhitespace:
                    child: Colon
                    whitespace:
                      Whitespace " "
                value: Text:
                  opening: OpeningText:
                    opening_single_quotes:
                    opening_double_quote: DoubleQuote
                  parts:
                    TextPart "Hi"
                  closing: ClosingText:
                    closing_double_quote: DoubleQuote
                    closing_single_quotes:
                comma: Comma
              whitespace:
                Newline "\n"
          closing_bracket: ClosingBracket
        "###);
    }
}
