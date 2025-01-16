use super::{
    literal::{newline, octothorpe},
    utils::whitespace_indentation_score,
};
use crate::{
    cst::{CstError, CstKind},
    rcst::Rcst,
    string_to_rcst::utils::SUPPORTED_WHITESPACE,
};
use itertools::Itertools;
use tracing::instrument;

#[instrument(level = "trace")]
pub fn single_line_whitespace(mut input: &str) -> Option<(&str, Rcst)> {
    let mut chars = vec![];
    let mut has_error = false;
    while let Some(c) = input.chars().next() {
        const SPACE: char = ' ';
        match c {
            SPACE => {}
            c if SUPPORTED_WHITESPACE.contains(c) && c != '\n' && c != '\r' => {
                has_error = true;
            }
            _ => break,
        }
        chars.push(c);
        input = &input[c.len_utf8()..];
    }
    let whitespace = chars.into_iter().join("");
    if has_error {
        Some((
            input,
            CstKind::Error {
                unparsable_input: whitespace,
                error: CstError::WeirdWhitespace,
            }
            .into(),
        ))
    } else if !whitespace.is_empty() {
        Some((input, CstKind::Whitespace(whitespace).into()))
    } else {
        None
    }
}

#[instrument(level = "trace")]
pub fn comment(input: &str) -> Option<(&str, Rcst)> {
    let (mut input, octothorpe) = octothorpe(input)?;
    let mut comment = vec![];
    loop {
        match input.chars().next() {
            Some('\n' | '\r') | None => {
                break;
            }
            Some(c) => {
                comment.push(c);
                input = &input[c.len_utf8()..];
            }
        }
    }
    Some((
        input,
        CstKind::Comment {
            octothorpe: Box::new(octothorpe),
            comment: comment.into_iter().join(""),
        }
        .into(),
    ))
}

#[instrument(level = "trace")]
pub fn leading_indentation(mut input: &str, indentation: usize) -> Option<(&str, Rcst)> {
    let mut chars = vec![];
    let mut has_weird_whitespace = false;
    let mut indentation_score = 0;

    while indentation_score < 2 * indentation {
        let c = input.chars().next()?;
        let is_weird = match c {
            ' ' => false,
            '\n' | '\r' => return None,
            c if c.is_whitespace() => true,
            _ => return None,
        };
        chars.push(c);
        has_weird_whitespace |= is_weird;
        indentation_score += whitespace_indentation_score(&format!("{c}"));
        input = &input[c.len_utf8()..];
    }
    let whitespace = chars.into_iter().join("");
    Some((
        input,
        if has_weird_whitespace {
            CstKind::Error {
                unparsable_input: whitespace,
                error: CstError::WeirdWhitespaceInIndentation,
            }
            .into()
        } else {
            CstKind::Whitespace(whitespace).into()
        },
    ))
}

/// Consumes all leading whitespace (including newlines) and optionally comments
/// that are still within the given indentation. Won't consume a newline
/// followed by less-indented whitespace followed by non-whitespace stuff like
/// an expression.
#[instrument(level = "trace")]
pub fn whitespaces_and_newlines(
    mut input: &str,
    indentation: usize,
    also_comments: bool,
) -> (&str, Vec<Rcst>) {
    let mut parts = vec![];

    if let Some((new_input, whitespace)) = single_line_whitespace(input) {
        input = new_input;
        parts.push(whitespace);
    }

    let mut new_input = input;
    let mut new_parts = vec![];
    let mut is_sufficiently_indented = true;
    loop {
        let new_input_from_iteration_start = new_input;

        if also_comments
            && is_sufficiently_indented
            && let Some((new_new_input, whitespace)) = comment(new_input)
        {
            new_input = new_new_input;
            new_parts.push(whitespace);

            input = new_input;
            parts.append(&mut new_parts);
        }

        if let Some((new_new_input, newline)) = newline(new_input) {
            new_input = new_new_input;
            new_parts.push(newline);
            is_sufficiently_indented = false;
        }
        if let Some((new_new_input, whitespace)) = leading_indentation(new_input, indentation) {
            new_input = new_new_input;
            new_parts.push(whitespace);

            input = new_input;
            parts.append(&mut new_parts);
            is_sufficiently_indented = true;
        }
        if let Some((new_new_input, whitespace)) = single_line_whitespace(new_input) {
            new_input = new_new_input;
            new_parts.push(whitespace);
        }

        if new_input == new_input_from_iteration_start {
            break;
        }
    }

    let parts = parts
        .into_iter()
        .filter(|it| {
            if let CstKind::Whitespace(ws) = &it.kind {
                !ws.is_empty()
            } else {
                true
            }
        })
        .collect();
    (input, parts)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::string_to_rcst::utils::assert_rich_ir_snapshot;

    #[test]
    pub fn test_single_line_whitespace() {
        assert_rich_ir_snapshot!(single_line_whitespace("  \nfoo"), @r###"
        Remaining input: "
        foo"
        Parsed: Whitespace "  "
        "###);
    }

    #[test]
    fn test_leading_indentation() {
        assert_rich_ir_snapshot!(leading_indentation("foo", 0), @r###"
        Remaining input: "foo"
        Parsed: Whitespace ""
        "###);
        assert_rich_ir_snapshot!(leading_indentation("  foo", 1), @r###"
        Remaining input: "foo"
        Parsed: Whitespace "  "
        "###);
        assert_rich_ir_snapshot!(leading_indentation("  foo", 2), @"Nothing was parsed");
    }

    #[test]
    fn test_whitespaces_and_newlines() {
        assert_rich_ir_snapshot!(whitespaces_and_newlines("foo", 0, true), @r###"
        Remaining input: "foo"
        Parsed: 
        "###);
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines("\nfoo", 0, true),
            @r###"
        Remaining input: "foo"
        Parsed: Newline "\n"
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines("\nfoo", 1, true),
            @r###"
        Remaining input: "
        foo"
        Parsed: 
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines("\n  foo", 1, true),
            @r###"
        Remaining input: "foo"
        Parsed: Newline "\n"
        Whitespace "  "
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines("\n  foo", 0, true),
            @r###"
        Remaining input: "foo"
        Parsed: Newline "\n"
        Whitespace "  "
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines(" \n  foo", 0, true),
            @r###"
        Remaining input: "foo"
        Parsed: Whitespace " "
        Newline "\n"
        Whitespace "  "
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines("\n  foo", 2, true),
            @r###"
        Remaining input: "
          foo"
        Parsed: 
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines("\tfoo", 1, true),
            @r###"
        Remaining input: "foo"
        Parsed: Error:
          unparsable_input: "	"
          error: WeirdWhitespace
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines("# hey\n  foo", 1, true),
            @r###"
        Remaining input: "foo"
        Parsed: Comment:
          octothorpe: Octothorpe
          comment: " hey"
        Newline "\n"
        Whitespace "  "
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines("# foo\n\n  #bar\n", 1, true),
            @r###"
        Remaining input: "
        "
        Parsed: Comment:
          octothorpe: Octothorpe
          comment: " foo"
        Newline "\n"
        Newline "\n"
        Whitespace "  "
        Comment:
          octothorpe: Octothorpe
          comment: "bar"
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines(" # abc\n", 1, true),
            @r###"
        Remaining input: "
        "
        Parsed: Whitespace " "
        Comment:
          octothorpe: Octothorpe
          comment: " abc"
        "###
        );
        assert_rich_ir_snapshot!(
            whitespaces_and_newlines("\n# abc\n", 1, true),
            @r###"
        Remaining input: "
        # abc
        "
        Parsed: 
        "###
        );
    }
}
