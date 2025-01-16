use super::{
    literal::{closing_bracket, closing_parenthesis, comma, opening_bracket, opening_parenthesis},
    parser::{OptionOfParser, OptionOfParserWithValue, Parser},
    whitespace::{AndTrailingWhitespace, ValueAndTrailingWhitespace},
    word::raw_identifier,
};
use crate::ast::{
    AstError, AstFunctionType, AstFunctionTypeParameterType, AstNamedType, AstString, AstType,
    AstTypeArgument, AstTypeArguments,
};
use tracing::instrument;

#[instrument(level = "trace")]
pub fn type_(parser: Parser) -> Option<(Parser, AstType)> {
    None.or_else(|| named_type(parser).map(|(parser, it)| (parser, AstType::Named(it))))
        .or_else(|| function_type(parser).map(|(parser, it)| (parser, AstType::Function(it))))
}

#[instrument(level = "trace")]
pub fn named_type(parser: Parser) -> Option<(Parser, AstNamedType)> {
    let (parser, name) = raw_identifier(parser)?.and_trailing_whitespace();
    let (parser, type_arguments) = type_arguments(parser).optional(parser);
    Some((
        parser,
        AstNamedType {
            name,
            type_arguments,
        },
    ))
}

#[instrument(level = "trace")]
pub fn function_type(parser: Parser) -> Option<(Parser, AstFunctionType)> {
    let start_offset = parser.offset();
    let mut parser = opening_parenthesis(parser)?.and_trailing_whitespace();

    // TODO: error on duplicate type parameter names
    let mut parameter_types: Vec<AstFunctionTypeParameterType> = vec![];
    let mut parser_for_missing_comma_error: Option<Parser> = None;
    while let Some((new_parser, parameter_type, new_parser_for_missing_comma_error)) =
        function_type_parameter_type(parser.and_trailing_whitespace())
    {
        if let Some(parser_for_missing_comma_error) = parser_for_missing_comma_error {
            parameter_types.last_mut().unwrap().comma_error = Some(
                parser_for_missing_comma_error
                    .error_at_current_offset("This parameter type is missing a comma."),
            );
        }

        parser = new_parser;
        parameter_types.push(parameter_type);
        parser_for_missing_comma_error = new_parser_for_missing_comma_error;
    }
    let parser = parser.and_trailing_whitespace();

    let (parser, closing_parenthesis_error) = closing_parenthesis(parser)
        .unwrap_or_ast_error(
            parser,
            "These parameter types are missing a closing parenthesis.",
        )
        .and_trailing_whitespace();

    let (parser, return_type) =
        type_(parser).unwrap_or_ast_error(parser, "This function type is missing a return type.");

    Some((
        parser,
        AstFunctionType {
            parameter_types,
            closing_parenthesis_error,
            return_type: return_type.map(Box::new),
            span: start_offset..parser.offset(),
        },
    ))
}
#[instrument(level = "trace")]
fn function_type_parameter_type<'a>(
    parser: Parser,
) -> Option<(Parser, AstFunctionTypeParameterType, Option<Parser>)> {
    let (parser, type_) = type_(parser)?.and_trailing_whitespace();

    let (parser, parser_for_missing_comma_error) =
        comma(parser).map_or((parser, Some(parser)), |parser| (parser, None));

    Some((
        parser,
        AstFunctionTypeParameterType {
            type_: Box::new(type_),
            comma_error: None,
        },
        parser_for_missing_comma_error,
    ))
}

#[instrument(level = "trace")]
pub fn type_arguments<'s>(parser: Parser<'s>) -> Option<(Parser<'s>, AstTypeArguments)> {
    let start_offset = parser.offset();
    let mut parser = opening_bracket(parser)?.and_trailing_whitespace();

    let mut arguments: Vec<AstTypeArgument> = vec![];
    let mut parser_for_missing_comma_error: Option<Parser> = None;
    while let Some((new_parser, argument, new_parser_for_missing_comma_error)) =
        type_argument(parser.and_trailing_whitespace())
    {
        if let Some(parser_for_missing_comma_error) = parser_for_missing_comma_error {
            arguments.last_mut().unwrap().comma_error = Some(
                parser_for_missing_comma_error
                    .error_at_current_offset("This type argument is missing a comma."),
            );
        }

        parser = new_parser;
        arguments.push(argument);
        parser_for_missing_comma_error = new_parser_for_missing_comma_error;
    }
    let parser = parser.and_trailing_whitespace();

    let (parser, closing_bracket_error) = closing_bracket(parser).unwrap_or_ast_error(
        parser,
        "These type arguments are missing a closing bracket.",
    );

    let span = start_offset..parser.offset();
    let empty_arguments_error = if arguments.is_empty() {
        Some(AstError {
            unparsable_input: AstString {
                string: parser.source()[*start_offset..*parser.offset()].into(),
                file: parser.file().to_path_buf(),
                span: span.clone(),
            },
            error: "Type argument brackets must not be empty.".into(),
        })
    } else {
        None
    };

    Some((
        parser,
        AstTypeArguments {
            span,
            arguments,
            empty_arguments_error,
            closing_bracket_error,
        },
    ))
}
#[instrument(level = "trace")]
fn type_argument<'a>(parser: Parser) -> Option<(Parser, AstTypeArgument, Option<Parser>)> {
    let (parser, type_) = type_(parser)?.and_trailing_whitespace();

    let (parser, parser_for_missing_comma_error) =
        comma(parser).map_or((parser, Some(parser)), |parser| (parser, None));

    Some((
        parser,
        AstTypeArgument {
            type_,
            comma_error: None,
        },
        parser_for_missing_comma_error,
    ))
}
