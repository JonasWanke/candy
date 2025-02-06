use super::{
    expression::{body, expression},
    list::list_of,
    literal::{
        builtin_keyword, closing_bracket, closing_curly_brace, closing_parenthesis, colon, comma,
        enum_keyword, equals_sign, fun_keyword, impl_keyword, let_keyword, opening_bracket,
        opening_curly_brace, opening_parenthesis, struct_keyword, trait_keyword,
    },
    parser::{OptionOfParser, OptionOfParserWithResult, OptionOfParserWithValue, Parser},
    type_::type_,
    whitespace::{
        AndTrailingWhitespace, OptionAndTrailingWhitespace, OptionWithValueAndTrailingWhitespace,
        ValueAndTrailingWhitespace,
    },
    word::raw_identifier,
};
use crate::ast::{
    AstAssignment, AstDeclaration, AstEnum, AstEnumVariant, AstError, AstFunction, AstImpl,
    AstParameter, AstParameters, AstString, AstStruct, AstStructField, AstStructKind, AstTrait,
    AstTypeParameter, AstTypeParameters,
};
use tracing::instrument;

#[instrument(level = "trace")]
pub fn declaration<'a>(parser: Parser) -> Option<(Parser, AstDeclaration)> {
    None.or_else(|| struct_(parser).map(|(parser, it)| (parser, AstDeclaration::Struct(it))))
        .or_else(|| enum_(parser).map(|(parser, it)| (parser, AstDeclaration::Enum(it))))
        .or_else(|| trait_(parser).map(|(parser, it)| (parser, AstDeclaration::Trait(it))))
        .or_else(|| impl_(parser).map(|(parser, it)| (parser, AstDeclaration::Impl(it))))
        .or_else(|| assignment(parser).map(|(parser, it)| (parser, AstDeclaration::Assignment(it))))
        .or_else(|| function(parser).map(|(parser, it)| (parser, AstDeclaration::Function(it))))
}

#[instrument(level = "trace")]
fn struct_<'a>(parser: Parser) -> Option<(Parser, AstStruct)> {
    let parser = struct_keyword(parser)?.and_trailing_whitespace();

    let (parser, name) = raw_identifier(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error_result(parser, "This struct is missing a name.");

    let (parser, type_parameters) = type_parameters(parser)
        .optional(parser)
        .and_trailing_whitespace();

    let (parser, kind) =
        struct_kind_builtin(parser).unwrap_or_else(|| struct_kind_user_defined(parser));

    Some((
        parser,
        AstStruct {
            name,
            type_parameters,
            kind,
        },
    ))
}
#[instrument(level = "trace")]
fn struct_kind_builtin<'a>(parser: Parser) -> Option<(Parser, AstStructKind)> {
    let parser = equals_sign(parser)?.and_trailing_whitespace();

    let (parser, builtin_keyword_error) = builtin_keyword(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This struct is missing the builtin keyword.");

    Some((
        parser,
        AstStructKind::Builtin {
            builtin_keyword_error,
        },
    ))
}
#[instrument(level = "trace")]
fn struct_kind_user_defined<'a>(parser: Parser) -> (Parser, AstStructKind) {
    let (mut parser, opening_curly_brace_error) = opening_curly_brace(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This struct is missing an opening curly brace.");

    let mut fields: Vec<AstStructField> = vec![];
    let mut parser_for_missing_comma_error: Option<Parser> = None;
    while let Some((new_parser, field, new_parser_for_missing_comma_error)) = struct_field(parser) {
        if let Some(parser_for_missing_comma_error) = parser_for_missing_comma_error {
            fields.last_mut().unwrap().comma_error = Some(
                parser_for_missing_comma_error
                    .error_at_current_offset("This struct field is missing a comma."),
            );
        }

        parser = new_parser.and_trailing_whitespace();
        fields.push(field);
        parser_for_missing_comma_error = new_parser_for_missing_comma_error;
    }

    let (parser, closing_curly_brace_error) = closing_curly_brace(parser)
        .unwrap_or_ast_error(parser, "This struct is missing a closing curly brace.");

    (
        parser,
        AstStructKind::UserDefined {
            opening_curly_brace_error,
            fields,
            closing_curly_brace_error,
        },
    )
}
#[instrument(level = "trace")]
fn struct_field<'a>(parser: Parser) -> Option<(Parser, AstStructField, Option<Parser>)> {
    let (parser, name) = raw_identifier(parser)?.and_trailing_whitespace();

    let (parser, colon_error) = colon(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This struct field is missing a colon.");

    let (parser, type_) = type_(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This struct field is missing a type.");

    let (parser, parser_for_missing_comma_error) =
        comma(parser).map_or((parser, Some(parser)), |parser| (parser, None));

    Some((
        parser,
        AstStructField {
            name,
            colon_error,
            type_,
            comma_error: None,
        },
        parser_for_missing_comma_error,
    ))
}

#[instrument(level = "trace")]
fn enum_<'a>(parser: Parser) -> Option<(Parser, AstEnum)> {
    let parser = enum_keyword(parser)?.and_trailing_whitespace();

    let (parser, name) = raw_identifier(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error_result(parser, "This enum is missing a name.");

    let (parser, type_parameters) = type_parameters(parser)
        .optional(parser)
        .and_trailing_whitespace();

    let (mut parser, opening_curly_brace_error) = opening_curly_brace(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This enum is missing an opening curly brace.");

    let mut variants: Vec<AstEnumVariant> = vec![];
    let mut parser_for_missing_comma_error: Option<Parser> = None;
    while let Some((new_parser, variant, new_parser_for_missing_comma_error)) = enum_variant(parser)
    {
        if let Some(parser_for_missing_comma_error) = parser_for_missing_comma_error {
            variants.last_mut().unwrap().comma_error = Some(
                parser_for_missing_comma_error
                    .error_at_current_offset("This enum variant is missing a comma."),
            );
        }

        parser = new_parser.and_trailing_whitespace();
        variants.push(variant);
        parser_for_missing_comma_error = new_parser_for_missing_comma_error;
    }

    let (parser, closing_curly_brace_error) = closing_curly_brace(parser)
        .unwrap_or_ast_error(parser, "This enum is missing a closing curly brace.");

    Some((
        parser,
        AstEnum {
            name,
            type_parameters,
            opening_curly_brace_error,
            variants,
            closing_curly_brace_error,
        },
    ))
}
#[instrument(level = "trace")]
fn enum_variant<'a>(parser: Parser) -> Option<(Parser, AstEnumVariant, Option<Parser>)> {
    let (parser, name) = raw_identifier(parser)?.and_trailing_whitespace();

    let (parser, type_) =
        colon(parser)
            .and_trailing_whitespace()
            .map_or((parser, None), |parser| {
                let (parser, type_) = type_(parser)
                    .and_trailing_whitespace()
                    .unwrap_or_ast_error(parser, "This enum variant is missing a type.");
                (parser, Some(type_))
            });

    let (parser, parser_for_missing_comma_error) =
        comma(parser).map_or((parser, Some(parser)), |parser| (parser, None));

    Some((
        parser,
        AstEnumVariant {
            name,
            type_,
            comma_error: None,
        },
        parser_for_missing_comma_error,
    ))
}

#[instrument(level = "trace")]
fn trait_<'a>(parser: Parser) -> Option<(Parser, AstTrait)> {
    let parser = trait_keyword(parser)?.and_trailing_whitespace();

    let (parser, name) = raw_identifier(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error_result(parser, "This trait is missing a name.");

    let (parser, type_parameters) = type_parameters(parser)
        .optional(parser)
        .and_trailing_whitespace();

    let (parser, opening_curly_brace_error) = opening_curly_brace(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This trait is missing an opening curly brace.");

    let (parser, functions) = list_of(parser, function);

    let (parser, closing_curly_brace_error) = closing_curly_brace(parser)
        .unwrap_or_ast_error(parser, "This trait is missing a closing curly brace.");

    Some((
        parser,
        AstTrait {
            name,
            type_parameters,
            opening_curly_brace_error,
            functions,
            closing_curly_brace_error,
        },
    ))
}

#[instrument(level = "trace")]
fn impl_<'a>(parser: Parser) -> Option<(Parser, AstImpl)> {
    let start_offset = parser.offset();
    let parser = impl_keyword(parser)?.and_trailing_whitespace();
    let impl_keyword_span = start_offset..parser.offset();

    let (parser, type_parameters) = type_parameters(parser)
        .optional(parser)
        .and_trailing_whitespace();

    let (parser, base_type) = type_(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This impl is missing a type.");

    let (parser, colon_error) = colon(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This impl is missing a colon after the type.");

    let (parser, trait_) = type_(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This impl is missing a trait.");

    let (parser, opening_curly_brace_error) = opening_curly_brace(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This impl is missing an opening curly brace.");

    let (parser, functions) = list_of(parser, function);

    let (parser, closing_curly_brace_error) = closing_curly_brace(parser)
        .unwrap_or_ast_error(parser, "This impl is missing a closing curly brace.");

    Some((
        parser,
        AstImpl {
            impl_keyword_span,
            type_parameters,
            type_: base_type,
            colon_error,
            trait_,
            opening_curly_brace_error,
            functions,
            closing_curly_brace_error,
        },
    ))
}

#[instrument(level = "trace")]
pub fn assignment<'a>(parser: Parser) -> Option<(Parser, AstAssignment)> {
    let let_keyword_start = parser.offset();
    let parser = let_keyword(parser)?.and_trailing_whitespace();
    let let_keyword_span = let_keyword_start..parser.offset();

    let (parser, name) = raw_identifier(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error_result(parser, "This assignment is missing a name.");
    let display_span = name.value().map_or(let_keyword_span, |it| it.span.clone());

    let (parser, type_) =
        colon(parser)
            .and_trailing_whitespace()
            .map_or((parser, None), |parser| {
                let (parser, type_) = type_(parser).and_trailing_whitespace().unwrap_or_ast_error(
                    parser,
                    "This assignment is missing a type after the colon.",
                );
                (parser, Some(type_))
            });

    let (parser, equals_sign_error) = equals_sign(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This assignment is missing an equals sign.");

    let (parser, value) =
        expression(parser).unwrap_or_ast_error(parser, "This assignment is missing a value.");

    Some((
        parser,
        AstAssignment {
            display_span,
            name,
            type_,
            equals_sign_error,
            value,
        },
    ))
}

#[instrument(level = "trace")]
fn function<'a>(parser: Parser) -> Option<(Parser, AstFunction)> {
    let fun_keyword_start = parser.offset();
    let parser = fun_keyword(parser)?;
    let fun_keyword_span = fun_keyword_start..parser.offset();
    let parser = parser.and_trailing_whitespace();

    let (parser, name) = raw_identifier(parser)
        .unwrap_or_ast_error_result(parser, "This function is missing a name.")
        .and_trailing_whitespace();
    let display_span = name.value().map_or(fun_keyword_span, |it| it.span.clone());

    let (parser, type_parameters) = type_parameters(parser).optional(parser);
    let (parser, parameters) =
        parameters(parser).unwrap_or_ast_error(parser, "Expected parameters");
    let (parser, return_type) = type_(parser).optional(parser).and_trailing_whitespace();
    let (parser, body) = body(parser).optional(parser);

    Some((
        parser,
        AstFunction {
            display_span,
            name,
            type_parameters,
            parameters,
            return_type,
            body,
        },
    ))
}
#[instrument(level = "trace")]
pub fn parameters<'a>(parser: Parser) -> Option<(Parser, AstParameters)> {
    let mut parser = opening_parenthesis(parser)?.and_trailing_whitespace();

    let mut parameters: Vec<AstParameter> = vec![];
    // TODO: error on duplicate parameter names
    let mut parser_for_missing_comma_error: Option<Parser> = None;
    while let Some((new_parser, parameter, new_parser_for_missing_comma_error)) = parameter(parser)
    {
        if let Some(parser_for_missing_comma_error) = parser_for_missing_comma_error {
            parameters.last_mut().unwrap().comma_error = Some(
                parser_for_missing_comma_error
                    .error_at_current_offset("This parameter is missing a comma."),
            );
        }

        parser = new_parser.and_trailing_whitespace();
        parameters.push(parameter);
        parser_for_missing_comma_error = new_parser_for_missing_comma_error;
    }

    let (parser, closing_parenthesis_error) = closing_parenthesis(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(
            parser,
            "This parameter list is missing a closing parenthesis.",
        );

    Some((
        parser,
        AstParameters {
            parameters,
            closing_parenthesis_error,
        },
    ))
}
#[instrument(level = "trace")]
fn parameter<'a>(parser: Parser) -> Option<(Parser, AstParameter, Option<Parser>)> {
    let (parser, name) = raw_identifier(parser)?.and_trailing_whitespace();

    let (parser, colon_error) = colon(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This parameter is missing a colon.");

    let (parser, type_) = type_(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This parameter is missing a type.");

    let (parser, parser_for_missing_comma_error) =
        comma(parser).map_or((parser, Some(parser)), |parser| (parser, None));

    Some((
        parser,
        AstParameter {
            name,
            colon_error,
            type_,
            comma_error: None,
        },
        parser_for_missing_comma_error,
    ))
}

#[instrument(level = "trace")]
fn type_parameters(parser: Parser) -> Option<(Parser, AstTypeParameters)> {
    let start_offset = parser.offset();
    let mut parser = opening_bracket(parser)?.and_trailing_whitespace();

    // TODO: error on duplicate type parameter names
    let mut parameters: Vec<AstTypeParameter> = vec![];
    let mut parser_for_missing_comma_error: Option<Parser> = None;
    while let Some((new_parser, parameter, new_parser_for_missing_comma_error)) =
        type_parameter(parser.and_trailing_whitespace())
    {
        if let Some(parser_for_missing_comma_error) = parser_for_missing_comma_error {
            parameters.last_mut().unwrap().comma_error = Some(
                parser_for_missing_comma_error
                    .error_at_current_offset("This parameter is missing a comma."),
            );
        }

        parser = new_parser;
        parameters.push(parameter);
        parser_for_missing_comma_error = new_parser_for_missing_comma_error;
    }
    let parser = parser.and_trailing_whitespace();

    let (parser, closing_bracket_error) = closing_bracket(parser).unwrap_or_ast_error(
        parser,
        "These type parameters are missing a closing bracket.",
    );

    let empty_parameters_error = if parameters.is_empty() {
        Some(AstError {
            unparsable_input: AstString {
                string: parser.source()[*start_offset..*parser.offset()].into(),
                file: parser.file().to_path_buf(),
                span: start_offset..parser.offset(),
            },
            error: "Type parameter brackets must not be empty.".into(),
        })
    } else {
        None
    };

    Some((
        parser,
        AstTypeParameters {
            parameters,
            empty_parameters_error,
            closing_bracket_error,
        },
    ))
}
#[instrument(level = "trace")]
fn type_parameter<'a>(parser: Parser) -> Option<(Parser, AstTypeParameter, Option<Parser>)> {
    let (parser, name) = raw_identifier(parser)?.and_trailing_whitespace();

    let (parser, upper_bound) =
        colon(parser)
            .and_trailing_whitespace()
            .map_or((parser, None), |parser| {
                let (parser, upper_bound) =
                    type_(parser).and_trailing_whitespace().unwrap_or_ast_error(
                        parser,
                        "This type parameter is missing an upper bound after the colon.",
                    );
                (parser, Some(upper_bound))
            });

    let (parser, parser_for_missing_comma_error) =
        comma(parser).map_or((parser, Some(parser)), |parser| (parser, None));

    Some((
        parser,
        AstTypeParameter {
            name,
            upper_bound,
            comma_error: None,
        },
        parser_for_missing_comma_error,
    ))
}
