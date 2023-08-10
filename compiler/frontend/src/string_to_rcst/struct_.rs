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
                    unparsable_input: "".to_string(),
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
                    unparsable_input: "".to_string(),
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
                unparsable_input: "".to_string(),
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
                unparsable_input: "".to_string(),
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
// #[test]
// fn test_struct() {
//     assert_eq!(struct_("hello", 0), None);
//     assert_eq!(
//         struct_("[]", 0),
//         Some((
//             "",
//             CstKind::Struct {
//                 opening_bracket: Box::new(CstKind::OpeningBracket.into()),
//                 fields: vec![],
//                 closing_bracket: Box::new(CstKind::ClosingBracket.into()),
//             }
//             .into(),
//         )),
//     );
//     assert_eq!(
//         struct_("[ ]", 0),
//         Some((
//             "",
//             CstKind::Struct {
//                 opening_bracket: Box::new(CstKind::OpeningBracket.with_trailing_space()),
//                 fields: vec![],
//                 closing_bracket: Box::new(CstKind::ClosingBracket.into()),
//             }
//             .into(),
//         )),
//     );
//     assert_eq!(
//         struct_("[foo:bar]", 0),
//         Some((
//             "",
//             CstKind::Struct {
//                 opening_bracket: Box::new(CstKind::OpeningBracket.into()),
//                 fields: vec![CstKind::StructField {
//                     key_and_colon: Some(Box::new((
//                         build_identifier("foo"),
//                         CstKind::Colon.into(),
//                     ))),
//                     value: Box::new(build_identifier("bar")),
//                     comma: None,
//                 }
//                 .into()],
//                 closing_bracket: Box::new(CstKind::ClosingBracket.into()),
//             }
//             .into(),
//         )),
//     );
//     assert_eq!(
//         struct_("[foo,bar:baz]", 0),
//         Some((
//             "",
//             CstKind::Struct {
//                 opening_bracket: Box::new(CstKind::OpeningBracket.into()),
//                 fields: vec![
//                     CstKind::StructField {
//                         key_and_colon: None,
//                         value: Box::new(build_identifier("foo")),
//                         comma: Some(Box::new(CstKind::Comma.into())),
//                     }
//                     .into(),
//                     CstKind::StructField {
//                         key_and_colon: Some(Box::new((
//                             build_identifier("bar"),
//                             CstKind::Colon.into(),
//                         ))),
//                         value: Box::new(build_identifier("baz")),
//                         comma: None,
//                     }
//                     .into(),
//                 ],
//                 closing_bracket: Box::new(CstKind::ClosingBracket.into()),
//             }
//             .into(),
//         )),
//     );
//     assert_eq!(
//         struct_("[foo := [foo]", 0),
//         Some((
//             ":= [foo]",
//             CstKind::Struct {
//                 opening_bracket: Box::new(CstKind::OpeningBracket.into()),
//                 fields: vec![CstKind::StructField {
//                     key_and_colon: None,
//                     value: Box::new(build_identifier("foo").with_trailing_space()),
//                     comma: None,
//                 }
//                 .into()],
//                 closing_bracket: Box::new(
//                     CstKind::Error {
//                         unparsable_input: "".to_string(),
//                         error: CstError::StructNotClosed,
//                     }
//                     .into()
//                 ),
//             }
//             .into(),
//         )),
//     );
//     // [
//     //   foo: bar,
//     //   4: "Hi",
//     // ]
//     assert_eq!(
//         struct_("[\n  foo: bar,\n  4: \"Hi\",\n]", 0),
//         Some((
//             "",
//             CstKind::Struct {
//                 opening_bracket: Box::new(CstKind::OpeningBracket.with_trailing_whitespace(
//                     vec![
//                         CstKind::Newline("\n".to_string()),
//                         CstKind::Whitespace("  ".to_string()),
//                     ],
//                 )),
//                 fields: vec![
//                     CstKind::StructField {
//                         key_and_colon: Some(Box::new((
//                             build_identifier("foo"),
//                             CstKind::Colon.with_trailing_space(),
//                         ))),
//                         value: Box::new(build_identifier("bar")),
//                         comma: Some(Box::new(CstKind::Comma.into())),
//                     }
//                     .with_trailing_whitespace(vec![
//                         CstKind::Newline("\n".to_string()),
//                         CstKind::Whitespace("  ".to_string()),
//                     ]),
//                     CstKind::StructField {
//                         key_and_colon: Some(Box::new((
//                             build_simple_int(4),
//                             CstKind::Colon.with_trailing_space(),
//                         ))),
//                         value: Box::new(build_simple_text("Hi")),
//                         comma: Some(Box::new(CstKind::Comma.into())),
//                     }
//                     .with_trailing_whitespace(vec![CstKind::Newline("\n".to_string())]),
//                 ],
//                 closing_bracket: Box::new(CstKind::ClosingBracket.into()),
//             }
//             .into(),
//         )),
//     );
// }
