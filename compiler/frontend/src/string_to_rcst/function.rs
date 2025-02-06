use super::{
    body::body,
    expression::{expression, ExpressionParsingOptions},
    literal::{arrow, closing_curly_brace, opening_curly_brace},
    whitespace::whitespaces_and_newlines,
};
use crate::{
    cst::{CstError, CstKind},
    rcst::Rcst,
};
use tracing::instrument;

#[instrument(level = "trace")]
pub fn function(input: &str, indentation: usize) -> Option<(&str, Rcst)> {
    let (input, opening_curly_brace) = opening_curly_brace(input)?;
    let (input, mut opening_curly_brace, mut parameters_and_arrow) = {
        let input_without_params = input;
        let opening_curly_brace_without_params = opening_curly_brace.clone();

        let mut input = input;
        let mut opening_curly_brace = opening_curly_brace;
        let mut parameters: Vec<Rcst> = vec![];
        loop {
            let (i, whitespace) = whitespaces_and_newlines(input, indentation + 1, true);
            if let Some(last_parameter) = parameters.pop() {
                parameters.push(last_parameter.wrap_in_whitespace(whitespace));
            } else {
                opening_curly_brace = opening_curly_brace.wrap_in_whitespace(whitespace);
            }

            input = i;
            match expression(
                input,
                indentation + 1,
                ExpressionParsingOptions {
                    allow_assignment: false,
                    allow_call: false,
                    allow_bar: false,
                    allow_function: false,
                },
            ) {
                Some((i, parameter)) => {
                    input = i;
                    parameters.push(parameter);
                }
                None => break,
            };
        }
        match arrow(input) {
            Some((input, arrow)) => (input, opening_curly_brace, Some((parameters, arrow))),
            None => (
                input_without_params,
                opening_curly_brace_without_params,
                None,
            ),
        }
    };

    let (input, whitespace) = whitespaces_and_newlines(input, indentation + 1, true);
    if let Some((parameters, arrow)) = parameters_and_arrow {
        parameters_and_arrow = Some((parameters, arrow.wrap_in_whitespace(whitespace)));
    } else {
        opening_curly_brace = opening_curly_brace.wrap_in_whitespace(whitespace);
    }

    let mut body_expressions = vec![];
    let (input, mut whitespace_before_closing_curly_brace, closing_curly_brace) = {
        let input = match expression(
            input,
            indentation + 1,
            ExpressionParsingOptions {
                allow_assignment: true,
                allow_call: true,
                allow_bar: true,
                allow_function: true,
            },
        ) {
            Some((input, expression)) => {
                body_expressions.push(expression);
                input
            }
            None => input,
        };
        let (input, mut whitespace) = whitespaces_and_newlines(input, indentation + 1, true);

        if let Some((input, curly_brace)) = closing_curly_brace(input) {
            (input, whitespace, curly_brace)
        } else {
            // There is no closing brace after a single expression. Thus, we now
            // try to parse a body of multiple expressions. We didn't try this
            // first because then the body would also have consumed any trailing
            // closing curly brace in the same line.
            //
            // For example, for the function `{ 2 }`, the body parser would have
            // already consumed the `}`. The body parser works great for
            // multiline bodies, though.
            body_expressions.append(&mut whitespace);
            let (input, mut body) = body(input, indentation + 1);
            body_expressions.append(&mut body);

            let input_after_body = input;
            let (input, whitespace) = whitespaces_and_newlines(input, indentation, true);
            match closing_curly_brace(input) {
                Some((input, closing_curly_brace)) => (input, whitespace, closing_curly_brace),
                None => (
                    input_after_body,
                    vec![],
                    CstKind::Error {
                        unparsable_input: String::new(),
                        error: CstError::CurlyBraceNotClosed,
                    }
                    .into(),
                ),
            }
        }
    };

    // Attach the `whitespace_before_closing_curly_brace`.
    if !body_expressions.is_empty() {
        body_expressions.append(&mut whitespace_before_closing_curly_brace);
    } else if let Some((parameters, arrow)) = parameters_and_arrow {
        parameters_and_arrow = Some((
            parameters,
            arrow.wrap_in_whitespace(whitespace_before_closing_curly_brace),
        ));
    } else {
        opening_curly_brace =
            opening_curly_brace.wrap_in_whitespace(whitespace_before_closing_curly_brace);
    }

    Some((
        input,
        CstKind::Function {
            opening_curly_brace: Box::new(opening_curly_brace),
            parameters_and_arrow: parameters_and_arrow
                .map(|(parameters, arrow)| (parameters, Box::new(arrow))),
            body: body_expressions,
            closing_curly_brace: Box::new(closing_curly_brace),
        }
        .into(),
    ))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::string_to_rcst::utils::assert_rich_ir_snapshot;

    #[test]
    fn test_function() {
        assert_rich_ir_snapshot!(function("2", 0), @"Nothing was parsed");
        assert_rich_ir_snapshot!(function("{ 2 }", 0), @r###"
        Remaining input: ""
        Parsed: Function:
          opening_curly_brace: TrailingWhitespace:
            child: OpeningCurlyBrace
            whitespace:
              Whitespace " "
          parameters_and_arrow: None
          body:
            Int:
              radix_prefix: None
              value: 2
              string: "2"
            Whitespace " "
          closing_curly_brace: ClosingCurlyBrace
        "###);
        // { a ->
        //   foo
        // }
        assert_rich_ir_snapshot!(function("{ a ->\n  foo\n}", 0), @r###"
        Remaining input: ""
        Parsed: Function:
          opening_curly_brace: TrailingWhitespace:
            child: OpeningCurlyBrace
            whitespace:
              Whitespace " "
          parameters_and_arrow:
            parameters:
              TrailingWhitespace:
                child: Identifier "a"
                whitespace:
                  Whitespace " "
            arrow: TrailingWhitespace:
              child: Arrow
              whitespace:
                Newline "\n"
                Whitespace "  "
          body:
            Identifier "foo"
            Newline "\n"
          closing_curly_brace: ClosingCurlyBrace
        "###);
        // {
        // foo
        assert_rich_ir_snapshot!(function("{\nfoo", 0), @r###"
        Remaining input: "
        foo"
        Parsed: Function:
          opening_curly_brace: OpeningCurlyBrace
          parameters_and_arrow: None
          body:
          closing_curly_brace: Error:
            unparsable_input: ""
            error: CurlyBraceNotClosed
        "###);
        // {->
        // }
        assert_rich_ir_snapshot!(function("{->\n}", 1), @r###"
        Remaining input: "
        }"
        Parsed: Function:
          opening_curly_brace: OpeningCurlyBrace
          parameters_and_arrow:
            parameters:
            arrow: Arrow
          body:
          closing_curly_brace: Error:
            unparsable_input: ""
            error: CurlyBraceNotClosed
        "###);
        // { foo
        //   bar
        // }
        assert_rich_ir_snapshot!(function("{ foo\n  bar\n}", 0), @r###"
        Remaining input: ""
        Parsed: Function:
          opening_curly_brace: TrailingWhitespace:
            child: OpeningCurlyBrace
            whitespace:
              Whitespace " "
          parameters_and_arrow: None
          body:
            Identifier "foo"
            Newline "\n"
            Whitespace "  "
            Identifier "bar"
            Newline "\n"
          closing_curly_brace: ClosingCurlyBrace
        "###);
        // { foo # abc
        // }
        assert_rich_ir_snapshot!(function("{ foo # abc\n}", 0), @r###"
        Remaining input: ""
        Parsed: Function:
          opening_curly_brace: TrailingWhitespace:
            child: OpeningCurlyBrace
            whitespace:
              Whitespace " "
          parameters_and_arrow: None
          body:
            Identifier "foo"
            Whitespace " "
            Comment:
              octothorpe: Octothorpe
              comment: " abc"
            Newline "\n"
          closing_curly_brace: ClosingCurlyBrace
        "###);
    }
}
