use super::{
    body::body,
    function::function,
    int::int,
    list::list,
    literal::{
        arrow, bar, closing_bracket, closing_curly_brace, closing_parenthesis, colon_equals_sign,
        comma, dot, equals_sign, percent,
    },
    struct_::struct_,
    text::text,
    whitespace::{comment, single_line_whitespace, whitespaces_and_newlines},
    word::{identifier, symbol, word},
};
use crate::{
    cst::{CstError, CstKind, IsMultiline},
    rcst::{Rcst, SplitOuterTrailingWhitespace},
};
use tracing::instrument;

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
pub struct ExpressionParsingOptions {
    pub allow_assignment: bool,
    pub allow_call: bool,
    pub allow_bar: bool,
    pub allow_function: bool,
}

#[instrument(level = "trace")]
pub fn expression(
    input: &str,
    indentation: usize,
    options: ExpressionParsingOptions,
) -> Option<(&str, Rcst)> {
    // If we start the call list with `if … else …`, the formatting looks weird.
    // Hence, we start with a single `None`.
    let (mut input, mut result) = None
        .or_else(|| int(input))
        .or_else(|| text(input, indentation))
        .or_else(|| symbol(input))
        .or_else(|| list(input, indentation))
        .or_else(|| struct_(input, indentation, options.allow_function))
        .or_else(|| {
            if options.allow_function {
                function(input, indentation)
            } else {
                None
            }
        })
        .or_else(|| identifier(input))
        .or_else(|| {
            word(input).map(|(input, word)| {
                (
                    input,
                    CstKind::Error {
                        unparsable_input: word,
                        error: CstError::UnexpectedCharacters,
                    }
                    .into(),
                )
            })
        })?;

    loop {
        fn parse_suffix<'input>(
            input: &mut &'input str,
            indentation: usize,
            result: &mut Rcst,
            parser: fn(&'input str, &Rcst, usize) -> Option<(&'input str, Rcst)>,
        ) -> bool {
            if let Some((new_input, expression)) = parser(input, result, indentation) {
                *input = new_input;
                *result = expression;
                true
            } else {
                false
            }
        }

        let mut did_make_progress = false;

        did_make_progress |= parse_suffix(
            &mut input,
            indentation,
            &mut result,
            expression_suffix_struct_access,
        );

        if options.allow_call {
            did_make_progress |=
                parse_suffix(&mut input, indentation, &mut result, expression_suffix_call);
        }
        if options.allow_bar {
            did_make_progress |=
                parse_suffix(&mut input, indentation, &mut result, expression_suffix_bar);
            did_make_progress |= parse_suffix(
                &mut input,
                indentation,
                &mut result,
                expression_suffix_match,
            );
        }

        if options.allow_assignment {
            did_make_progress |= parse_suffix(
                &mut input,
                indentation,
                &mut result,
                expression_suffix_assignment,
            );
        }

        if !did_make_progress {
            break;
        }
    }
    Some((input, result))
}

#[instrument(level = "trace")]
fn expression_suffix_struct_access<'a>(
    input: &'a str,
    current: &Rcst,
    indentation: usize,
) -> Option<(&'a str, Rcst)> {
    let (input, whitespace_after_struct) = whitespaces_and_newlines(input, indentation + 1, true);

    let (input, dot) = dot(input)?;
    let (new_input, whitespace_after_dot) = whitespaces_and_newlines(input, indentation + 1, true);
    let dot = dot.wrap_in_whitespace(whitespace_after_dot);

    let (input, key) = identifier(new_input)?;

    Some((
        input,
        CstKind::StructAccess {
            struct_: Box::new(current.clone().wrap_in_whitespace(whitespace_after_struct)),
            dot: Box::new(dot),
            key: Box::new(key),
        }
        .into(),
    ))
}

#[instrument(level = "trace")]
fn expression_suffix_call<'a>(
    mut input: &'a str,
    current: &Rcst,
    indentation: usize,
) -> Option<(&'a str, Rcst)> {
    let mut expressions = vec![current.clone()];

    let mut has_multiline_whitespace = false;
    loop {
        let (i, whitespace) = whitespaces_and_newlines(input, indentation + 1, true);
        has_multiline_whitespace |= whitespace.is_multiline();
        let indentation = if has_multiline_whitespace {
            indentation + 1
        } else {
            indentation
        };
        let last = expressions.pop().unwrap();
        expressions.push(last.wrap_in_whitespace(whitespace));

        let parsed_expression = expression(
            i,
            indentation,
            ExpressionParsingOptions {
                allow_assignment: false,
                allow_call: has_multiline_whitespace,
                allow_bar: has_multiline_whitespace,
                allow_function: true,
            },
        );
        let (i, expr) = if let Some(it) = parsed_expression {
            it
        } else {
            let fallback = closing_parenthesis(i)
                .or_else(|| closing_bracket(i))
                .or_else(|| closing_curly_brace(i))
                .or_else(|| arrow(i));
            if let Some((i, cst)) = fallback
                && has_multiline_whitespace
            {
                (i, cst)
            } else {
                input = i;
                break;
            }
        };

        expressions.push(expr);
        input = i;
    }

    if expressions.len() < 2 {
        return None;
    }

    let (whitespace, mut expressions) = expressions.split_outer_trailing_whitespace();
    let receiver = expressions.remove(0);
    let arguments = expressions;

    Some((
        input,
        CstKind::Call {
            receiver: Box::new(receiver),
            arguments,
        }
        .wrap_in_whitespace(whitespace),
    ))
}

#[instrument(level = "trace")]
fn expression_suffix_bar<'a>(
    input: &'a str,
    current: &Rcst,
    indentation: usize,
) -> Option<(&'a str, Rcst)> {
    let (input, whitespace_after_receiver) = whitespaces_and_newlines(input, indentation, true);

    let (input, bar) = bar(input)?;
    let (input, whitespace_after_bar) = whitespaces_and_newlines(input, indentation + 1, true);
    let bar = bar.wrap_in_whitespace(whitespace_after_bar);

    let indentation = if bar.is_multiline() {
        indentation + 1
    } else {
        indentation
    };
    let (input, call) = expression(
        input,
        indentation,
        ExpressionParsingOptions {
            allow_assignment: false,
            allow_call: true,
            allow_bar: false,
            allow_function: true,
        },
    )
    .unwrap_or_else(|| {
        let error = CstKind::Error {
            unparsable_input: String::new(),
            error: CstError::BinaryBarMissesRight,
        };
        (input, error.into())
    });

    Some((
        input,
        CstKind::BinaryBar {
            left: Box::new(
                current
                    .clone()
                    .wrap_in_whitespace(whitespace_after_receiver),
            ),
            bar: Box::new(bar),
            right: Box::new(call),
        }
        .into(),
    ))
}

#[instrument(level = "trace")]
fn expression_suffix_match<'a>(
    input: &'a str,
    current: &Rcst,
    indentation: usize,
) -> Option<(&'a str, Rcst)> {
    let (input, whitespace_after_receiver) = whitespaces_and_newlines(input, indentation, true);
    let (input, percent) = percent(input)?;
    let (mut input, whitespace) = whitespaces_and_newlines(input, indentation + 1, true);
    let percent = percent.wrap_in_whitespace(whitespace);

    let mut cases = vec![];
    loop {
        let Some((new_input, case)) = match_case(input, indentation + 1) else {
            break;
        };
        let (new_input, whitespace) = whitespaces_and_newlines(new_input, indentation + 1, true);
        input = new_input;
        let is_whitespace_multiline = whitespace.is_multiline();
        let case = case.wrap_in_whitespace(whitespace);
        cases.push(case);
        if !is_whitespace_multiline {
            break;
        }
    }
    if cases.is_empty() {
        cases.push(
            CstKind::Error {
                unparsable_input: String::new(),
                error: CstError::MatchMissesCases,
            }
            .into(),
        );
    }

    Some((
        input,
        CstKind::Match {
            expression: Box::new(
                current
                    .clone()
                    .wrap_in_whitespace(whitespace_after_receiver),
            ),
            percent: Box::new(percent),
            cases,
        }
        .into(),
    ))
}

#[instrument(level = "trace")]
fn expression_suffix_assignment<'a>(
    input: &'a str,
    left: &Rcst,
    indentation: usize,
) -> Option<(&'a str, Rcst)> {
    let (input, whitespace_after_left) = whitespaces_and_newlines(input, indentation, true);
    let (input, mut assignment_sign) = colon_equals_sign(input).or_else(|| equals_sign(input))?;

    // By now, it's clear that we are in an assignment, so we can do more
    // expensive operations. We also save some state in case the assignment is
    // invalid (so we can stop parsing right after the assignment sign).
    let left = left.clone().wrap_in_whitespace(whitespace_after_left);
    let just_the_assignment_sign = assignment_sign.clone();
    let input_after_assignment_sign = input;

    let (input, more_whitespace) = whitespaces_and_newlines(input, indentation + 1, false);
    assignment_sign = assignment_sign.wrap_in_whitespace(more_whitespace);

    let is_multiline = left.is_multiline() || assignment_sign.is_multiline();
    let (input, assignment_sign, body) = if is_multiline {
        let (input, body) = body(input, indentation + 1);
        if body.is_empty() {
            (
                input_after_assignment_sign,
                just_the_assignment_sign,
                vec![],
            )
        } else {
            (input, assignment_sign, body)
        }
    } else {
        let mut body = vec![];
        let mut input = input;
        if let Some((new_input, expression)) = expression(
            input,
            indentation,
            ExpressionParsingOptions {
                allow_assignment: false,
                allow_call: true,
                allow_bar: true,
                allow_function: true,
            },
        ) {
            input = new_input;
            body.push(expression);
            if let Some((new_input, whitespace)) = single_line_whitespace(input) {
                input = new_input;
                body.push(whitespace);
            }
        }
        if let Some((new_input, comment)) = comment(input) {
            input = new_input;
            body.push(comment);
        }

        if body.is_empty() {
            (
                input_after_assignment_sign,
                just_the_assignment_sign,
                vec![],
            )
        } else {
            (input, assignment_sign, body)
        }
    };

    let (whitespace, (assignment_sign, body)) =
        (assignment_sign, body).split_outer_trailing_whitespace();
    Some((
        input,
        CstKind::Assignment {
            left: Box::new(left),
            assignment_sign: Box::new(assignment_sign),
            body,
        }
        .wrap_in_whitespace(whitespace),
    ))
}

#[instrument(level = "trace")]
fn match_case(input: &str, indentation: usize) -> Option<(&str, Rcst)> {
    let (input, pattern) = expression(
        input,
        indentation,
        ExpressionParsingOptions {
            allow_assignment: false,
            allow_call: true,
            allow_bar: true,
            allow_function: true,
        },
    )?;
    let (input, whitespace) = whitespaces_and_newlines(input, indentation, true);
    let pattern = pattern.wrap_in_whitespace(whitespace);

    let (input, condition) = if let Some((input, condition_comma)) = comma(input) {
        let (input, whitespace) = whitespaces_and_newlines(input, indentation, true);
        let condition_comma = condition_comma.wrap_in_whitespace(whitespace);
        if let Some((input, condition_expression)) = expression(
            input,
            indentation,
            ExpressionParsingOptions {
                allow_assignment: false,
                allow_call: true,
                allow_bar: true,
                allow_function: true,
            },
        ) {
            (input, Some((condition_comma, condition_expression)))
        } else {
            let error = CstKind::Error {
                unparsable_input: String::new(),
                error: CstError::MatchCaseMissesCondition,
            };
            (input, Some((condition_comma, error.into())))
        }
    } else {
        (input, None)
    };

    let (input, arrow) = if let Some((input, arrow)) = arrow(input) {
        let (input, whitespace) = whitespaces_and_newlines(input, indentation, true);
        (input, arrow.wrap_in_whitespace(whitespace))
    } else {
        let error = CstKind::Error {
            unparsable_input: String::new(),
            error: CstError::MatchCaseMissesArrow,
        };
        (input, error.into())
    };

    let (input, mut body) = body(input, indentation + 1);
    if body.is_empty() {
        body.push(
            CstKind::Error {
                unparsable_input: String::new(),
                error: CstError::MatchCaseMissesBody,
            }
            .into(),
        );
    }

    let case = CstKind::MatchCase {
        pattern: Box::new(pattern),
        condition: condition.map(Box::new),
        arrow: Box::new(arrow),
        body,
    };
    Some((input, case.into()))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::string_to_rcst::utils::assert_rich_ir_snapshot;

    #[test]
    fn test_expression() {
        assert_rich_ir_snapshot!(
            expression(
                "foo",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Identifier "foo"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "(foo Bar)",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: false,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Parenthesized:
          opening_parenthesis: OpeningParenthesis
          inner: Call:
            receiver: TrailingWhitespace:
              child: Identifier "foo"
              whitespace:
                Whitespace " "
            arguments:
              Symbol "Bar"
          closing_parenthesis: ClosingParenthesis
        "###
        );
        // foo
        //   .bar
        assert_rich_ir_snapshot!(
            expression(
                "foo\n  .bar",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: StructAccess:
          struct: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Newline "\n"
              Whitespace "  "
          dot: Dot
          key: Identifier "bar"
        "###
        );
        // foo
        // .bar
        assert_rich_ir_snapshot!(
            expression(
                "foo\n.bar",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        .bar"
        Parsed: Identifier "foo"
        "###
        );
        // foo
        // | bar
        assert_rich_ir_snapshot!(
            expression(
                "foo\n| bar",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: BinaryBar:
          left: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Newline "\n"
          bar: TrailingWhitespace:
            child: Bar
            whitespace:
              Whitespace " "
          right: Identifier "bar"
        "###
        );
        // foo
        // | bar baz
        assert_rich_ir_snapshot!(
            expression(
                "foo\n| bar baz",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: BinaryBar:
          left: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Newline "\n"
          bar: TrailingWhitespace:
            child: Bar
            whitespace:
              Whitespace " "
          right: Call:
            receiver: TrailingWhitespace:
              child: Identifier "bar"
              whitespace:
                Whitespace " "
            arguments:
              Identifier "baz"
        "###
        );
        // foo %
        //   123 -> 123
        assert_rich_ir_snapshot!(
            expression(
                "foo %\n  123 -> 123",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Match:
          expression: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          percent: TrailingWhitespace:
            child: Percent
            whitespace:
              Newline "\n"
              Whitespace "  "
          cases:
            MatchCase:
              pattern: TrailingWhitespace:
                child: Int:
                  radix_prefix: None
                  value: 123
                  string: "123"
                whitespace:
                  Whitespace " "
              condition: None
              arrow: TrailingWhitespace:
                child: Arrow
                whitespace:
                  Whitespace " "
              body:
                Int:
                  radix_prefix: None
                  value: 123
                  string: "123"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "(0, foo) | (foo, 0)",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: BinaryBar:
          left: TrailingWhitespace:
            child: List:
              opening_parenthesis: OpeningParenthesis
              items:
                TrailingWhitespace:
                  child: ListItem:
                    value: Int:
                      radix_prefix: None
                      value: 0
                      string: "0"
                    comma: Comma
                  whitespace:
                    Whitespace " "
                ListItem:
                  value: Identifier "foo"
                  comma: None
              closing_parenthesis: ClosingParenthesis
            whitespace:
              Whitespace " "
          bar: TrailingWhitespace:
            child: Bar
            whitespace:
              Whitespace " "
          right: List:
            opening_parenthesis: OpeningParenthesis
            items:
              TrailingWhitespace:
                child: ListItem:
                  value: Identifier "foo"
                  comma: Comma
                whitespace:
                  Whitespace " "
              ListItem:
                value: Int:
                  radix_prefix: None
                  value: 0
                  string: "0"
                comma: None
            closing_parenthesis: ClosingParenthesis
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "foo bar",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Call:
          receiver: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          arguments:
            Identifier "bar"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "Foo 4 bar",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Call:
          receiver: TrailingWhitespace:
            child: Symbol "Foo"
            whitespace:
              Whitespace " "
          arguments:
            TrailingWhitespace:
              child: Int:
                radix_prefix: None
                value: 4
                string: "4"
              whitespace:
                Whitespace " "
            Identifier "bar"
        "###
        );
        // foo
        //   bar
        //   baz
        // 2
        assert_rich_ir_snapshot!(
            expression(
                "foo\n  bar\n  baz\n2",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        2"
        Parsed: Call:
          receiver: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Newline "\n"
              Whitespace "  "
          arguments:
            TrailingWhitespace:
              child: Identifier "bar"
              whitespace:
                Newline "\n"
                Whitespace "  "
            Identifier "baz"
        "###
        );
        // foo 1 2
        //   3
        //   4
        // bar
        assert_rich_ir_snapshot!(
            expression(
                "foo 1 2\n  3\n  4\nbar",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        bar"
        Parsed: Call:
          receiver: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          arguments:
            TrailingWhitespace:
              child: Int:
                radix_prefix: None
                value: 1
                string: "1"
              whitespace:
                Whitespace " "
            TrailingWhitespace:
              child: Int:
                radix_prefix: None
                value: 2
                string: "2"
              whitespace:
                Newline "\n"
                Whitespace "  "
            TrailingWhitespace:
              child: Int:
                radix_prefix: None
                value: 3
                string: "3"
              whitespace:
                Newline "\n"
                Whitespace "  "
            Int:
              radix_prefix: None
              value: 4
              string: "4"
        "###
        );
        // foo
        //   bar | baz
        assert_rich_ir_snapshot!(
            expression(
                "foo\n  bar | baz",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Call:
          receiver: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Newline "\n"
              Whitespace "  "
          arguments:
            BinaryBar:
              left: TrailingWhitespace:
                child: Identifier "bar"
                whitespace:
                  Whitespace " "
              bar: TrailingWhitespace:
                child: Bar
                whitespace:
                  Whitespace " "
              right: Identifier "baz"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "(foo Bar) Baz\n",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        "
        Parsed: Call:
          receiver: TrailingWhitespace:
            child: Parenthesized:
              opening_parenthesis: OpeningParenthesis
              inner: Call:
                receiver: TrailingWhitespace:
                  child: Identifier "foo"
                  whitespace:
                    Whitespace " "
                arguments:
                  Symbol "Bar"
              closing_parenthesis: ClosingParenthesis
            whitespace:
              Whitespace " "
          arguments:
            Symbol "Baz"
        "###
        );
        // foo T
        //
        //
        // bar = 5
        assert_rich_ir_snapshot!(
            expression(
                "foo T\n\n\nbar = 5",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "


        bar = 5"
        Parsed: Call:
          receiver: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          arguments:
            Symbol "T"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "foo = 42",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Whitespace " "
          body:
            Int:
              radix_prefix: None
              value: 42
              string: "42"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "foo =\n  bar\n\nbaz",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "

        baz"
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Newline "\n"
              Whitespace "  "
          body:
            Identifier "bar"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "foo 42",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Call:
          receiver: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          arguments:
            Int:
              radix_prefix: None
              value: 42
              string: "42"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "foo %",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: false,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Match:
          expression: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          percent: Percent
          cases:
            Error:
              unparsable_input: ""
              error: MatchMissesCases
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "foo %\n",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: false,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        "
        Parsed: Match:
          expression: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          percent: Percent
          cases:
            Error:
              unparsable_input: ""
              error: MatchMissesCases
        "###
        );
        // foo %
        //   1 -> 2
        // Foo
        assert_rich_ir_snapshot!(
            expression(
                "foo %\n  1 -> 2\nFoo",
                0,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: false,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        Foo"
        Parsed: Match:
          expression: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          percent: TrailingWhitespace:
            child: Percent
            whitespace:
              Newline "\n"
              Whitespace "  "
          cases:
            MatchCase:
              pattern: TrailingWhitespace:
                child: Int:
                  radix_prefix: None
                  value: 1
                  string: "1"
                whitespace:
                  Whitespace " "
              condition: None
              arrow: TrailingWhitespace:
                child: Arrow
                whitespace:
                  Whitespace " "
              body:
                Int:
                  radix_prefix: None
                  value: 2
                  string: "2"
        "###
        );
        assert_rich_ir_snapshot!(
          expression("foo %\n  n, n | int.atLeast 4 -> 42", 0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }),
                @r###"
        Remaining input: ""
        Parsed: Match:
          expression: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          percent: TrailingWhitespace:
            child: Percent
            whitespace:
              Newline "\n"
              Whitespace "  "
          cases:
            MatchCase:
              pattern: Identifier "n"
              condition:
                comma: TrailingWhitespace:
                  child: Comma
                  whitespace:
                    Whitespace " "
                expression: BinaryBar:
                  left: TrailingWhitespace:
                    child: Identifier "n"
                    whitespace:
                      Whitespace " "
                  bar: TrailingWhitespace:
                    child: Bar
                    whitespace:
                      Whitespace " "
                  right: TrailingWhitespace:
                    child: Call:
                      receiver: TrailingWhitespace:
                        child: StructAccess:
                          struct: Identifier "int"
                          dot: Dot
                          key: Identifier "atLeast"
                        whitespace:
                          Whitespace " "
                      arguments:
                        Int:
                          radix_prefix: None
                          value: 4
                          string: "4"
                    whitespace:
                      Whitespace " "
              arrow: TrailingWhitespace:
                child: Arrow
                whitespace:
                  Whitespace " "
              body:
                Int:
                  radix_prefix: None
                  value: 42
                  string: "42"
        "###
        );
        assert_rich_ir_snapshot!(
          expression("foo %\n  n, -> 42", 0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }),
                @r###"
        Remaining input: ""
        Parsed: Match:
          expression: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          percent: TrailingWhitespace:
            child: Percent
            whitespace:
              Newline "\n"
              Whitespace "  "
          cases:
            MatchCase:
              pattern: Identifier "n"
              condition:
                comma: TrailingWhitespace:
                  child: Comma
                  whitespace:
                    Whitespace " "
                expression: Error:
                  unparsable_input: ""
                  error: MatchCaseMissesCondition
              arrow: TrailingWhitespace:
                child: Arrow
                whitespace:
                  Whitespace " "
              body:
                Int:
                  radix_prefix: None
                  value: 42
                  string: "42"
        "###
        );
        // foo bar =
        //   3
        // 2
        assert_rich_ir_snapshot!(
            expression(
                "foo bar =\n  3\n2",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        2"
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Call:
              receiver: TrailingWhitespace:
                child: Identifier "foo"
                whitespace:
                  Whitespace " "
              arguments:
                Identifier "bar"
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Newline "\n"
              Whitespace "  "
          body:
            Int:
              radix_prefix: None
              value: 3
              string: "3"
        "###
        );
        // main := { environment ->
        //   input
        // }
        assert_rich_ir_snapshot!(
            expression(
                "main := { environment ->\n  input\n}",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Identifier "main"
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: ColonEqualsSign
            whitespace:
              Whitespace " "
          body:
            Function:
              opening_curly_brace: TrailingWhitespace:
                child: OpeningCurlyBrace
                whitespace:
                  Whitespace " "
              parameters_and_arrow:
                parameters:
                  TrailingWhitespace:
                    child: Identifier "environment"
                    whitespace:
                      Whitespace " "
                arrow: TrailingWhitespace:
                  child: Arrow
                  whitespace:
                    Newline "\n"
                    Whitespace "  "
              body:
                Identifier "input"
                Newline "\n"
              closing_curly_brace: ClosingCurlyBrace
        "###
        );
        // foo
        //   bar
        //   = 3
        assert_rich_ir_snapshot!(
            expression(
                "foo\n  bar\n  = 3",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Call:
              receiver: TrailingWhitespace:
                child: Identifier "foo"
                whitespace:
                  Newline "\n"
                  Whitespace "  "
              arguments:
                Identifier "bar"
            whitespace:
              Newline "\n"
              Whitespace "  "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Whitespace " "
          body:
            Int:
              radix_prefix: None
              value: 3
              string: "3"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "foo =\n  ",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
          "
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          assignment_sign: EqualsSign
          body:
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "foo = # comment\n",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        "
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Whitespace " "
          body:
            Comment:
              octothorpe: Octothorpe
              comment: " comment"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "foo = bar # comment\n",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        "
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Whitespace " "
          body:
            Identifier "bar"
            Whitespace " "
            Comment:
              octothorpe: Octothorpe
              comment: " comment"
        "###
        );
        // foo =
        //   # comment
        // 3
        assert_rich_ir_snapshot!(
            expression(
                "foo =\n  # comment\n3",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        3"
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Newline "\n"
              Whitespace "  "
          body:
            Comment:
              octothorpe: Octothorpe
              comment: " comment"
        "###
        );
        // foo =
        //   # comment
        //   5
        // 3
        assert_rich_ir_snapshot!(
            expression(
                "foo =\n  # comment\n  5\n3",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: "
        3"
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Identifier "foo"
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Newline "\n"
              Whitespace "  "
          body:
            Comment:
              octothorpe: Octothorpe
              comment: " comment"
            Newline "\n"
            Whitespace "  "
            Int:
              radix_prefix: None
              value: 5
              string: "5"
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "(foo, bar) = (1, 2)",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: List:
              opening_parenthesis: OpeningParenthesis
              items:
                TrailingWhitespace:
                  child: ListItem:
                    value: Identifier "foo"
                    comma: Comma
                  whitespace:
                    Whitespace " "
                ListItem:
                  value: Identifier "bar"
                  comma: None
              closing_parenthesis: ClosingParenthesis
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Whitespace " "
          body:
            List:
              opening_parenthesis: OpeningParenthesis
              items:
                TrailingWhitespace:
                  child: ListItem:
                    value: Int:
                      radix_prefix: None
                      value: 1
                      string: "1"
                    comma: Comma
                  whitespace:
                    Whitespace " "
                ListItem:
                  value: Int:
                    radix_prefix: None
                    value: 2
                    string: "2"
                  comma: None
              closing_parenthesis: ClosingParenthesis
        "###
        );
        assert_rich_ir_snapshot!(
            expression(
                "[Foo: foo] = bar",
                0,
                ExpressionParsingOptions {
                    allow_assignment: true,
                    allow_call: true,
                    allow_bar: true,
                    allow_function: true
                }
            ),
            @r###"
        Remaining input: ""
        Parsed: Assignment:
          left: TrailingWhitespace:
            child: Struct:
              opening_bracket: OpeningBracket
              fields:
                StructField:
                  key_and_colon:
                    key: Symbol "Foo"
                    colon: TrailingWhitespace:
                      child: Colon
                      whitespace:
                        Whitespace " "
                  value: Identifier "foo"
                  comma: None
              closing_bracket: ClosingBracket
            whitespace:
              Whitespace " "
          assignment_sign: TrailingWhitespace:
            child: EqualsSign
            whitespace:
              Whitespace " "
          body:
            Identifier "bar"
        "###
        );
    }
}
