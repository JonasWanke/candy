use super::{
    declarations::{assignment, parameters},
    list::list_of,
    literal::{
        arrow, closing_curly_brace, closing_parenthesis, comma, dot, opening_curly_brace,
        opening_parenthesis, switch_keyword,
    },
    parser::{OptionOfParser, OptionOfParserWithResult, OptionOfParserWithValue, Parser},
    text::text,
    type_::type_arguments,
    whitespace::{
        whitespace, AndTrailingWhitespace, OptionAndTrailingWhitespace,
        OptionWithValueAndTrailingWhitespace, ValueAndTrailingWhitespace,
    },
    word::{raw_identifier, word},
};
use crate::ast::{
    AstArgument, AstArguments, AstBody, AstCall, AstError, AstExpression, AstExpressionKind,
    AstIdentifier, AstInt, AstLambda, AstNavigation, AstParenthesized, AstResult, AstStatement,
    AstSwitch, AstSwitchCase,
};
use replace_with::replace_with_or_abort;
use tracing::instrument;

#[instrument(level = "trace")]
pub fn expression(parser: Parser) -> Option<(Parser, AstExpression)> {
    // If we start the call list with `if … else …`, the formatting looks weird.
    // Hence, we start with a single `None`.
    let start_offset = parser.offset();
    let (mut parser, kind) = None
        .or_else(|| {
            raw_identifier(parser).map(|(parser, identifier)| {
                (
                    parser,
                    AstExpressionKind::Identifier(AstIdentifier { identifier }),
                )
            })
        })
        .or_else(|| int(parser).map(|(parser, it)| (parser, AstExpressionKind::Int(it))))
        .or_else(|| text(parser).map(|(parser, it)| (parser, AstExpressionKind::Text(it))))
        .or_else(|| lambda(parser).map(|(parser, it)| (parser, AstExpressionKind::Lambda(it))))
        .or_else(|| {
            parenthesized(parser).map(|(parser, it)| (parser, AstExpressionKind::Parenthesized(it)))
        })
        .or_else(|| body(parser).map(|(parser, it)| (parser, AstExpressionKind::Body(it))))
        .or_else(|| switch(parser).map(|(parser, it)| (parser, AstExpressionKind::Switch(it))))?;
    let span = start_offset..parser.offset();
    let mut result = AstExpression { span, kind };

    loop {
        fn parse_suffix<'a>(
            parser: &mut Parser<'a>,
            result: &mut AstExpression,
            parse: fn(Parser<'a>, &mut AstExpression) -> Option<Parser<'a>>,
        ) -> bool {
            parse(*parser, result).is_some_and(|new_parser| {
                *parser = new_parser;
                true
            })
        }

        let mut did_make_progress = false;
        did_make_progress |= parse_suffix(&mut parser, &mut result, expression_suffix_navigation);
        did_make_progress |= parse_suffix(&mut parser, &mut result, expression_suffix_call);
        if !did_make_progress {
            break;
        }
    }
    Some((parser, result))
}

#[instrument(level = "trace")]
pub fn int(parser: Parser) -> Option<(Parser, AstInt)> {
    let (parser, string) = word(parser)?;
    if !string.string.chars().next().unwrap().is_ascii_digit() {
        return None;
    }

    let value = if string.string.chars().all(|c| c.is_ascii_digit()) {
        AstResult::ok(str::parse(&string.string).expect("Couldn't parse int."))
    } else {
        AstResult::error(
            None,
            AstError {
                unparsable_input: string.clone(),
                error: "This integer contains characters that are not digits.".to_string(),
            },
        )
    };
    Some((parser, AstInt { value, string }))
}

#[instrument(level = "trace")]
fn parenthesized<'a>(parser: Parser) -> Option<(Parser, AstParenthesized)> {
    let parser = opening_parenthesis(parser)?.and_trailing_whitespace();

    let (parser, inner) = expression(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This parenthesized expression is missing a value.");

    let (parser, closing_parenthesis_error) = closing_parenthesis(parser).unwrap_or_ast_error(
        parser,
        "This parenthesized expression is missing a closing parenthesis.",
    );

    Some((
        parser,
        AstParenthesized {
            inner: inner.map(Box::new),
            closing_parenthesis_error,
        },
    ))
}
#[instrument(level = "trace")]
fn lambda<'a>(parser: Parser) -> Option<(Parser, AstLambda)> {
    let (parser, parameters) = parameters(parser)?.and_trailing_whitespace();
    let (parser, body) = body(parser)?;

    Some((parser, AstLambda { parameters, body }))
}
#[instrument(level = "trace")]
pub fn body<'a>(parser: Parser) -> Option<(Parser, AstBody)> {
    let parser = opening_curly_brace(parser)?.and_trailing_whitespace();

    let (parser, statements) = list_of(parser, statement);

    let (parser, closing_curly_brace_error) = closing_curly_brace(parser)
        .unwrap_or_ast_error(parser, "This body is missing a closing curly brace.");

    Some((
        parser,
        AstBody {
            statements,
            closing_curly_brace_error,
        },
    ))
}
#[instrument(level = "trace")]
pub fn statement(parser: Parser) -> Option<(Parser, AstStatement)> {
    assignment(parser)
        .map(|(parser, it)| (parser, AstStatement::Assignment(it)))
        .or_else(|| expression(parser).map(|(parser, it)| (parser, AstStatement::Expression(it))))
}

#[instrument(level = "trace")]
fn switch<'a>(parser: Parser) -> Option<(Parser, AstSwitch)> {
    let parser = switch_keyword(parser)?.and_trailing_whitespace();

    let (parser, value) = expression(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(
            parser,
            "This switch is missing an expression to switch over.",
        );

    let (mut parser, opening_curly_brace_error) = opening_curly_brace(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This switch is missing an opening curly brace.");

    let mut cases: Vec<AstSwitchCase> = vec![];
    let mut parser_for_missing_comma_error: Option<Parser> = None;
    while let Some((new_parser, case, new_parser_for_missing_comma_error)) =
        switch_case(parser.and_trailing_whitespace())
    {
        if let Some(parser_for_missing_comma_error) = parser_for_missing_comma_error {
            cases.last_mut().unwrap().comma_error = Some(
                parser_for_missing_comma_error
                    .error_at_current_offset("This case is missing a comma."),
            );
        }

        parser = new_parser;
        cases.push(case);
        parser_for_missing_comma_error = new_parser_for_missing_comma_error;
    }
    let parser = parser.and_trailing_whitespace();

    let (parser, closing_curly_brace_error) = closing_curly_brace(parser)
        .unwrap_or_ast_error(parser, "This switch is missing a closing curly brace.");

    Some((
        parser,
        AstSwitch {
            value: value.map(Box::new),
            opening_curly_brace_error,
            cases,
            closing_curly_brace_error,
        },
    ))
}
#[instrument(level = "trace")]
fn switch_case<'a>(parser: Parser) -> Option<(Parser, AstSwitchCase, Option<Parser>)> {
    let (parser, variant) = raw_identifier(parser)?.and_trailing_whitespace();

    let (parser, value_name) = opening_parenthesis(parser)
        .and_trailing_whitespace()
        .map_or((parser, None), |parser| {
            let (parser, value_name) = raw_identifier(parser)
                .and_trailing_whitespace()
                .unwrap_or_ast_error_result(parser, "Switch case is missing a value name.");
            let (parser, closing_parenthesis_error) = closing_parenthesis(parser)
                .and_trailing_whitespace()
                .unwrap_or_ast_error(
                    parser,
                    "Switch case is missing a closing parenthesis after the value name.",
                );
            (parser, Some((value_name, closing_parenthesis_error)))
        });

    let (parser, arrow_error) = arrow(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "Switch case is missing an arrow.");

    let (parser, expression) = expression(parser)
        .and_trailing_whitespace()
        .unwrap_or_ast_error(parser, "This switch case is missing an expression.");

    let (parser, parser_for_missing_comma_error) =
        comma(parser).map_or((parser, Some(parser)), |parser| (parser, None));

    Some((
        parser,
        AstSwitchCase {
            variant,
            value_name,
            arrow_error,
            expression,
            comma_error: None,
        },
        parser_for_missing_comma_error,
    ))
}

#[instrument(level = "trace")]
fn expression_suffix_navigation<'s>(
    parser: Parser<'s>,
    current: &mut AstExpression,
) -> Option<Parser<'s>> {
    let start_offset = parser.offset();
    let parser = whitespace(parser).unwrap_or(parser);

    let parser = dot(parser)?.and_trailing_whitespace();

    let (parser, key) = raw_identifier(parser)
        .unwrap_or_ast_error_result(parser, "This struct access is missing a key.");

    replace_with_or_abort(current, |current| AstExpression {
        span: start_offset..parser.offset(),
        kind: AstExpressionKind::Navigation(AstNavigation {
            receiver: Box::new(current),
            key,
        }),
    });

    Some(parser)
}

#[instrument(level = "trace")]
fn expression_suffix_call<'s>(
    parser: Parser<'s>,
    current: &mut AstExpression,
) -> Option<Parser<'s>> {
    let start_offset = parser.offset();
    let parser = whitespace(parser).unwrap_or(parser);

    let (parser, type_arguments) = type_arguments(parser).optional(parser);

    let (parser, arguments) = if let Some((parser, arguments)) = arguments(parser) {
        (parser, AstResult::ok(arguments))
    } else {
        type_arguments.as_ref()?;
        (
            parser,
            AstResult::error(
                None,
                parser.error_at_current_offset("This call is missing arguments."),
            ),
        )
    };

    replace_with_or_abort(current, |current| AstExpression {
        span: start_offset..parser.offset(),
        kind: AstExpressionKind::Call(AstCall {
            receiver: Box::new(current),
            type_arguments,
            arguments,
        }),
    });

    Some(parser)
}
#[instrument(level = "trace")]
fn arguments<'s>(parser: Parser<'s>) -> Option<(Parser<'s>, AstArguments)> {
    let opening_parenthesis_start = parser.offset();
    let mut parser = opening_parenthesis(parser)?.and_trailing_whitespace();
    let opening_parenthesis_span = opening_parenthesis_start..parser.offset();

    let mut arguments: Vec<AstArgument> = vec![];
    let mut parser_for_missing_comma_error: Option<Parser> = None;
    while let Some((new_parser, argument, new_parser_for_missing_comma_error)) =
        argument(parser.and_trailing_whitespace())
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

    let (parser, closing_parenthesis_error) = closing_parenthesis(parser)
        .unwrap_or_ast_error(parser, "This call is missing a closing parenthesis.");

    Some((
        parser,
        AstArguments {
            opening_parenthesis_span,
            arguments,
            closing_parenthesis_error,
        },
    ))
}
#[instrument(level = "trace")]
fn argument<'a>(parser: Parser) -> Option<(Parser, AstArgument, Option<Parser>)> {
    let start_offset = parser.offset();
    let (parser, value) = expression(parser)?.and_trailing_whitespace();

    let (parser, parser_for_missing_comma_error) =
        comma(parser).map_or((parser, Some(parser)), |parser| (parser, None));

    Some((
        parser,
        AstArgument {
            value,
            comma_error: None,
            span: start_offset..parser.offset(),
        },
        parser_for_missing_comma_error,
    ))
}
