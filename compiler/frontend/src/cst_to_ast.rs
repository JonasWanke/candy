use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    ast::{
        self, Assignment, AssignmentBody, Ast, AstError, AstKind, AstString, Call, CollectErrors,
        Function, Identifier, Int, List, Match, MatchCase, OrPattern, Struct, StructAccess, Symbol,
        Text, TextPart,
    },
    cst::{self, Cst, CstDb, CstKind, UnwrapWhitespaceAndComment},
    error::{CompilerError, CompilerErrorPayload},
    module::Module,
    position::Offset,
    rcst_to_cst::RcstToCst,
    string_to_rcst::ModuleError,
    utils::AdjustCasingOfFirstLetter,
};
use std::{collections::HashMap, ops::Range, sync::Arc};

#[salsa::query_group(CstToAstStorage)]
pub trait CstToAst: CstDb + RcstToCst {
    #[salsa::transparent]
    fn ast_to_cst_id(&self, id: &ast::Id) -> Option<cst::Id>;
    #[salsa::transparent]
    fn ast_id_to_span(&self, id: &ast::Id) -> Option<Range<Offset>>;
    #[salsa::transparent]
    fn ast_id_to_display_span(&self, id: &ast::Id) -> Option<Range<Offset>>;

    #[salsa::transparent]
    fn cst_to_ast_ids(&self, module: Module, id: cst::Id) -> Vec<ast::Id>;

    fn ast(&self, module: Module) -> AstResult;
}

pub type AstResult = Result<(Arc<Vec<Ast>>, Arc<FxHashMap<ast::Id, cst::Id>>), ModuleError>;

fn ast_to_cst_id(db: &dyn CstToAst, id: &ast::Id) -> Option<cst::Id> {
    let (_, ast_to_cst_id_mapping) = db.ast(id.module.clone()).ok()?;
    ast_to_cst_id_mapping.get(id).copied()
}
fn ast_id_to_span(db: &dyn CstToAst, id: &ast::Id) -> Option<Range<Offset>> {
    let cst_id = db.ast_to_cst_id(id)?;
    Some(db.find_cst(id.module.clone(), cst_id).data.span)
}
fn ast_id_to_display_span(db: &dyn CstToAst, id: &ast::Id) -> Option<Range<Offset>> {
    let cst_id = db.ast_to_cst_id(id)?;
    Some(db.find_cst(id.module.clone(), cst_id).display_span())
}

fn cst_to_ast_ids(db: &dyn CstToAst, module: Module, id: cst::Id) -> Vec<ast::Id> {
    if let Ok((_, ast_to_cst_id_mapping)) = db.ast(module) {
        ast_to_cst_id_mapping
            .iter()
            .filter_map(|(key, value)| if value == &id { Some(key) } else { None })
            .cloned()
            .sorted()
            .collect_vec()
    } else {
        vec![]
    }
}

fn ast(db: &dyn CstToAst, module: Module) -> AstResult {
    let mut context = LoweringContext::new(module.clone());

    db.cst(module).map(|cst| {
        let cst = cst.unwrap_whitespace_and_comment();
        let asts = context.lower_csts(&cst);
        (Arc::new(asts), Arc::new(context.id_mapping))
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum LoweringType {
    Expression,
    Pattern,
    PatternLiteralPart,
}

struct LoweringContext {
    module: Module,
    next_id: usize,
    id_mapping: FxHashMap<ast::Id, cst::Id>,
}
impl LoweringContext {
    fn new(module: Module) -> Self {
        Self {
            module,
            next_id: 0,
            id_mapping: FxHashMap::default(),
        }
    }
    fn lower_csts(&mut self, csts: &[Cst]) -> Vec<Ast> {
        csts.iter()
            .map(|it| self.lower_cst(it, LoweringType::Expression))
            .collect()
    }
    fn lower_cst(&mut self, cst: &Cst, lowering_type: LoweringType) -> Ast {
        match &cst.kind {
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
            | CstKind::Octothorpe => self.create_error_ast(
                cst,
                vec![self.create_error(cst, AstError::UnexpectedPunctuation)],
            ),
            CstKind::Whitespace(_)
            | CstKind::Newline(_)
            | CstKind::Comment { .. }
            | CstKind::TrailingWhitespace { .. } => {
                panic!("Whitespace should have been removed before lowering to AST.")
            }
            CstKind::Identifier(identifier) => {
                if lowering_type == LoweringType::PatternLiteralPart {
                    return self.create_ast_for_invalid_expression_in_pattern(cst);
                };
                let string = self.create_string(cst.data.id, identifier.to_string());
                self.create_ast(cst.data.id, Identifier(string))
            }
            CstKind::Symbol(symbol) => {
                let string = self.create_string(cst.data.id, symbol.to_string());
                self.create_ast(cst.data.id, Symbol(string))
            }
            CstKind::Int { value, .. } => self.create_ast(cst.data.id, Int(value.clone())),
            CstKind::Text {
                opening,
                parts,
                closing,
            } => {
                let mut errors = vec![];

                let opening_single_quote_count = match &opening.kind {
                    CstKind::OpeningText {
                        opening_single_quotes,
                        opening_double_quote: box Cst {
                            kind: CstKind::DoubleQuote,
                            ..
                        }
                    } if opening_single_quotes
                        .iter()
                        .all(|single_quote| single_quote.kind.is_single_quote()) => opening_single_quotes.len(),
                    _ => panic!("Text needs to start with any number of single quotes followed by a double quote, but started with {opening}.")
                };

                let lowered_parts = parts.iter().filter_map(|part| {
                    match &part.kind {
                        CstKind::TextNewline(_) => {
                            let newline = self.create_string(part.data.id, "\n".to_string());
                            Some(self.create_ast(part.data.id, AstKind::TextPart(TextPart(newline))))
                        },
                        CstKind::TextPart(text) => {
                            let string = self.create_string(part.data.id, text.clone());
                            Some(self.create_ast(part.data.id, AstKind::TextPart(TextPart(string))))
                        },
                        CstKind::TextInterpolation {
                            opening_curly_braces,
                            expression,
                            closing_curly_braces,
                        } => {
                            if lowering_type != LoweringType::Expression {
                                return Some(self.create_ast_for_invalid_expression_in_pattern(cst));
                            };

                            if opening_curly_braces.len() != (opening_single_quote_count + 1)
                                || !opening_curly_braces
                                    .iter()
                                    .all(|opening_curly_brace| opening_curly_brace.kind.is_opening_curly_brace())
                            {
                                panic!(
                                    "Text interpolation needs to start with {} opening curly braces, but started with {}.", 
                                    opening_single_quote_count + 1,
                                    opening_curly_braces.iter().map(|cst| format!("{cst}")).join(""),
                                )
                            }

                            let ast = self.lower_cst(expression, LoweringType::Expression);
                            if closing_curly_braces.len() == opening_single_quote_count + 1
                                && closing_curly_braces
                                    .iter()
                                    .all(|closing_curly_brace| closing_curly_brace.kind.is_closing_curly_brace())
                            {
                                Some(ast)
                            } else {
                                errors.push(self.create_error(
                                    part,
                                    AstError::TextInterpolationMissesClosingCurlyBraces,
                                ));
                                None
                            }
                        },
                        _ => panic!("TextLine contains non-TextPart. Whitespaces should have been removed already."),
                    }
                }).collect_vec();

                if !matches!(
                    &closing.kind,
                    CstKind::ClosingText {
                        closing_double_quote: box Cst {
                            kind: CstKind::DoubleQuote,
                            ..
                        },
                        closing_single_quotes
                    } if closing_single_quotes
                        .iter()
                        .all(|single_quote| single_quote.kind.is_single_quote())
                        && opening_single_quote_count == closing_single_quotes.len()
                ) {
                    errors.push(self.create_error(closing, AstError::TextMissesClosingQuote));
                }

                self.create_errors_or_ast(cst, errors, Text(lowered_parts))
            }
            CstKind::OpeningText { .. } => panic!("OpeningText should only occur in Text."),
            CstKind::ClosingText { .. } => panic!("ClosingText should only occur in Text."),
            CstKind::TextNewline(_) => panic!("TextNewline should only occur in Text."),
            CstKind::TextPart(_) => panic!("TextPart should only occur in Text."),
            CstKind::TextInterpolation { .. } => {
                panic!("TextInterpolation should only occur in Text.")
            }
            CstKind::BinaryBar { left, bar, right } => {
                match lowering_type {
                    // In an expression context, a bar introduces a call.
                    LoweringType::Expression => {
                        let left = self.lower_cst(left, LoweringType::Expression);

                        assert!(
                            bar.kind.is_bar(),
                            "BinaryBar must contain a bar, but instead contained a {bar}.",
                        );

                        let call = self.lower_cst(right, LoweringType::Expression);
                        let call = match call {
                            Ast {
                                kind:
                                    AstKind::Call(Call {
                                        receiver,
                                        mut arguments,
                                        ..
                                    }),
                                ..
                            } if right.kind.is_call() => {
                                arguments.insert(0, left);
                                Call {
                                    receiver,
                                    arguments,
                                    is_from_pipe: true,
                                }
                            }
                            call => Call {
                                receiver: Box::new(call),
                                arguments: vec![left],
                                is_from_pipe: true,
                            },
                        };
                        self.create_ast(cst.data.id, call)
                    }
                    // In a pattern context, a bar represents an or pattern.
                    LoweringType::Pattern | LoweringType::PatternLiteralPart => {
                        let mut patterns = vec![];

                        let mut cst = cst;
                        while let Cst {
                            kind: CstKind::BinaryBar { left, bar, right },
                            ..
                        } = cst
                        {
                            patterns.push(self.lower_cst(right, LoweringType::Pattern));
                            assert!(
                                bar.kind.is_bar(),
                                "Expected a bar after the left side of an or pattern, but found {bar}.",
                            );
                            cst = left;
                        }
                        patterns.push(self.lower_cst(left, LoweringType::Pattern));
                        patterns.reverse();

                        let captured_identifiers = patterns
                            .iter()
                            .map(|it| it.captured_identifiers())
                            .collect_vec();
                        let all_identifiers = captured_identifiers
                            .iter()
                            .flat_map(HashMap::keys)
                            .collect::<FxHashSet<_>>();
                        let mut errors = vec![];
                        for identifier in all_identifiers {
                            let number_of_missing_captures = captured_identifiers
                                .iter()
                                .filter(|it| !it.contains_key(identifier))
                                .count();
                            if number_of_missing_captures == 0 {
                                continue;
                            }

                            let empty_vec = vec![];
                            let all_captures = captured_identifiers
                                .iter()
                                .flat_map(|it| it.get(identifier).unwrap_or(&empty_vec))
                                .filter_map(|it| self.id_mapping.get(it).copied())
                                .collect_vec();
                            errors.push(self.create_error(
                                left,
                                AstError::OrPatternIsMissingIdentifiers {
                                    identifier: identifier.clone(),
                                    number_of_missing_captures:
                                        number_of_missing_captures.try_into().unwrap(),
                                    all_captures,
                                },
                            ));
                        }

                        self.create_errors_or_ast(cst, errors, OrPattern(patterns))
                    }
                }
            }
            CstKind::Parenthesized {
                opening_parenthesis,
                inner,
                closing_parenthesis,
            } => {
                match lowering_type {
                    LoweringType::Expression => {}
                    LoweringType::Pattern | LoweringType::PatternLiteralPart => {
                        return self.create_error_ast(
                            cst,
                            vec![self.create_error(cst, AstError::ParenthesizedInPattern)],
                        );
                    }
                }

                assert!(
                    opening_parenthesis.kind.is_opening_parenthesis(),
                    "Parenthesized needs to start with opening parenthesis, but started with {opening_parenthesis}.",
                );

                let inner = self.lower_cst(inner, LoweringType::Expression);

                let mut errors = vec![];
                if !closing_parenthesis.kind.is_closing_parenthesis() {
                    errors.push(self.create_error(
                        closing_parenthesis,
                        AstError::ParenthesizedMissesClosingParenthesis,
                    ));
                }

                self.create_errors_or_use_ast(cst, errors, inner)
            }
            CstKind::Call {
                receiver,
                arguments,
            } => {
                let mut receiver_kind = &receiver.kind;
                loop {
                    receiver_kind = match receiver_kind {
                        CstKind::Parenthesized {
                            opening_parenthesis,
                            inner,
                            closing_parenthesis,
                        } => {
                            assert!(
                                opening_parenthesis.kind.is_opening_parenthesis(),
                                "Parenthesized needs to start with opening parenthesis, but started with {opening_parenthesis}.",
                            );
                            if !closing_parenthesis.kind.is_closing_parenthesis() {
                                return self.create_error_ast(
                                    closing_parenthesis,
                                    vec![self.create_error(
                                        cst,
                                        AstError::ParenthesizedMissesClosingParenthesis,
                                    )],
                                );
                            }
                            &inner.kind
                        }
                        _ => break,
                    };
                }
                let receiver = self.lower_cst(receiver, LoweringType::Expression);
                let arguments = self.lower_csts(arguments);

                self.create_ast(
                    cst.data.id,
                    Call {
                        receiver: receiver.into(),
                        arguments,
                        is_from_pipe: false,
                    },
                )
            }
            CstKind::List {
                opening_parenthesis,
                items,
                closing_parenthesis,
            } => {
                if lowering_type == LoweringType::PatternLiteralPart {
                    return self.create_error_ast(
                        cst,
                        vec![self.create_error(
                            cst,
                            AstError::PatternLiteralPartContainsInvalidExpression,
                        )],
                    );
                };

                assert!(
                    opening_parenthesis.kind.is_opening_parenthesis(),
                    "List should always have an opening parenthesis, but instead had {opening_parenthesis}.",
                );

                let mut ast_items = vec![];
                let mut errors = vec![];
                if items.len() == 1 && items[0].kind.is_comma() {
                    // Empty list (`(,)`), do nothing.
                } else {
                    for item in items {
                        let CstKind::ListItem { value, comma } = &item.kind else {
                            errors.push(self.create_error(cst, AstError::ListWithNonListItem));
                            continue;
                        };

                        let value = self.lower_cst(&value.clone(), lowering_type);

                        if let Some(comma) = comma
                            && !comma.kind.is_comma()
                        {
                            errors.push(self.create_error(comma, AstError::ListItemMissesComma));
                        }

                        ast_items.push(value);
                    }
                }

                if !closing_parenthesis.kind.is_closing_parenthesis() {
                    errors.push(
                        self.create_error(
                            closing_parenthesis,
                            AstError::ListMissesClosingParenthesis,
                        ),
                    );
                }

                self.create_errors_or_ast(cst, errors, List(ast_items))
            }
            CstKind::ListItem { .. } => panic!("ListItem should only appear in List."),
            CstKind::Struct {
                opening_bracket,
                fields,
                closing_bracket,
            } => {
                if lowering_type == LoweringType::PatternLiteralPart {
                    return self.create_ast_for_invalid_expression_in_pattern(cst);
                };

                assert!(
                    opening_bracket.kind.is_opening_bracket(),
                    "Struct should always have an opening bracket, but instead had {opening_bracket}.",
                );

                let mut lowered_fields = vec![];
                let mut errors = vec![];
                for field in fields {
                    let CstKind::StructField {
                        key_and_colon,
                        value,
                        comma,
                    } = &field.kind
                    else {
                        errors.push(self.create_error(cst, AstError::StructWithNonStructField));
                        continue;
                    };

                    if let Some(box (key, colon)) = key_and_colon {
                        // Normal syntax, e.g. `[foo: bar]`.

                        let key_lowering_type = match lowering_type {
                            LoweringType::Expression => LoweringType::Expression,
                            LoweringType::Pattern | LoweringType::PatternLiteralPart => {
                                LoweringType::PatternLiteralPart
                            }
                        };
                        let key = if colon.kind.is_colon() {
                            self.lower_cst(key, key_lowering_type)
                        } else {
                            self.create_error_ast(
                                colon,
                                vec![self.create_error(colon, AstError::StructKeyMissesColon)],
                            )
                        };

                        let mut value = self.lower_cst(&value.clone(), lowering_type);

                        if let Some(comma) = comma
                            && !comma.kind.is_comma()
                        {
                            value = self.create_error_ast(
                                comma,
                                vec![self.create_error(comma, AstError::StructValueMissesComma)],
                            );
                        }

                        lowered_fields.push((Some(key), value));
                    } else {
                        // Shorthand syntax, e.g. `[foo]`.
                        let ast = self.lower_cst(&value.clone(), lowering_type);

                        if !ast.kind.is_identifier() {
                            errors.push(
                                self.create_error(
                                    value,
                                    AstError::StructShorthandWithNotIdentifier,
                                ),
                            );
                        }

                        if let Some(comma) = comma
                            && !comma.kind.is_comma()
                        {
                            errors.push(self.create_error(comma, AstError::StructValueMissesComma));
                        }
                        lowered_fields.push((None, ast));
                    }
                }

                if !closing_bracket.kind.is_closing_bracket() {
                    errors.push(
                        self.create_error(closing_bracket, AstError::StructMissesClosingBrace),
                    );
                }

                self.create_errors_or_ast(
                    cst,
                    errors,
                    Struct {
                        fields: lowered_fields,
                    },
                )
            }
            CstKind::StructField { .. } => panic!("StructField should only appear in Struct."),
            CstKind::StructAccess { struct_, dot, key } => {
                if lowering_type != LoweringType::Expression {
                    return self.create_ast_for_invalid_expression_in_pattern(cst);
                };

                self.lower_struct_access(cst.data.id, struct_, dot, key)
            }
            CstKind::Match {
                expression,
                percent,
                cases,
            } => {
                if lowering_type != LoweringType::Expression {
                    return self.create_ast_for_invalid_expression_in_pattern(cst);
                };

                let expression = self.lower_cst(expression, LoweringType::Expression);

                assert!(
                    percent.kind.is_percent(),
                    "Expected a percent sign after the expression to match over, but found {percent}.",
                );

                let cases = self.lower_csts(cases);

                self.create_ast(
                    cst.data.id,
                    Match {
                        expression: Box::new(expression),
                        cases,
                    },
                )
            }
            CstKind::MatchCase {
                pattern,
                condition,
                arrow,
                body,
            } => {
                if lowering_type != LoweringType::Expression {
                    return self.create_ast_for_invalid_expression_in_pattern(cst);
                };
                let mut errors = vec![];
                let pattern = self.lower_cst(pattern, LoweringType::Pattern);

                let condition = condition
                    .as_ref()
                    .map(|box (_, condition)| self.lower_cst(condition, LoweringType::Expression));

                if let CstKind::Error {
                    unparsable_input: _,
                    error,
                } = arrow.kind
                {
                    errors.push(self.create_error(arrow, error));
                }

                let body = self.lower_csts(body);

                self.create_errors_or_ast(
                    cst,
                    errors,
                    MatchCase {
                        pattern: Box::new(pattern),
                        condition: condition.map(Box::new),
                        body,
                    },
                )
            }
            CstKind::Function {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => {
                if lowering_type != LoweringType::Expression {
                    return self.create_ast_for_invalid_expression_in_pattern(cst);
                }

                assert!(
                    opening_curly_brace.kind.is_opening_curly_brace(),
                    "Expected an opening curly brace at the beginning of a function, but found {opening_curly_brace}.",
                );

                let parameters = if let Some((parameters, arrow)) = parameters_and_arrow {
                    assert!(
                        arrow.kind.is_arrow(),
                        "Expected an arrow after the parameters in a function, but found `{arrow}`.",
                    );
                    self.lower_parameters(parameters, false)
                } else {
                    vec![]
                };

                let body = self.lower_csts(body);

                let mut errors = vec![];
                if !closing_curly_brace.kind.is_closing_curly_brace() {
                    errors.push(self.create_error(
                        closing_curly_brace,
                        AstError::FunctionMissesClosingCurlyBrace,
                    ));
                }

                self.create_errors_or_ast(
                    cst,
                    errors,
                    Function {
                        parameters,
                        body,
                        fuzzable: false,
                    },
                )
            }
            CstKind::Assignment {
                left,
                assignment_sign,
                body,
            } => {
                if lowering_type != LoweringType::Expression {
                    return self.create_ast_for_invalid_expression_in_pattern(cst);
                };

                assert!(
                    matches!(assignment_sign.kind, CstKind::EqualsSign | CstKind::ColonEqualsSign),
                    "Expected an equals sign or colon equals sign for the assignment, but found {assignment_sign} instead.",
                );

                let body = self.lower_csts(body);
                let body = if let CstKind::Call {
                    receiver: name,
                    arguments: parameters,
                } = &left.kind
                    && !matches!(name.kind, CstKind::Symbol(_))
                {
                    let name = match &name.kind {
                        CstKind::Identifier(identifier) => {
                            self.create_string(name.data.id, identifier.clone())
                        }
                        CstKind::Error { error, .. } => {
                            return self
                                .create_error_ast(cst, vec![self.create_error(cst, *error)]);
                        }
                        _ => {
                            return self.create_error_ast(
                                cst,
                                vec![self.create_error(
                                    cst,
                                    AstError::ExpectedNameOrPatternInAssignment,
                                )],
                            );
                        }
                    };

                    let parameters = self.lower_parameters(parameters, true);
                    AssignmentBody::Function {
                        name,
                        function: Function {
                            parameters,
                            body,
                            fuzzable: true,
                        },
                    }
                } else {
                    AssignmentBody::Body {
                        pattern: Box::new(self.lower_cst(left, LoweringType::Pattern)),
                        body,
                    }
                };

                self.create_ast(
                    cst.data.id,
                    Assignment {
                        is_public: assignment_sign.kind.is_colon_equals_sign(),
                        body,
                    },
                )
            }
            CstKind::Error { error, .. } => {
                self.create_error_ast(cst, vec![self.create_error(cst, *error)])
            }
        }
    }

    fn lower_struct_access(&mut self, id: cst::Id, struct_: &Cst, dot: &Cst, key: &Cst) -> Ast {
        let struct_ = self.lower_cst(struct_, LoweringType::Expression);

        assert!(
            dot.kind.is_dot(),
            "Struct access should always have a dot, but instead had {dot}.",
        );

        match key.kind.clone() {
            CstKind::Identifier(identifier) => {
                let key = self.create_string(key.data.id, identifier.uppercase_first_letter());
                self.create_ast(
                    id,
                    StructAccess {
                        struct_: Box::new(struct_),
                        key,
                    },
                )
            }
            CstKind::Error { error, .. } => {
                self.create_error_ast(key, vec![self.create_error(key, error)])
            }
            _ => panic!(
                "Expected an identifier after the dot in a struct access, but found `{key}`.",
            ),
        }
    }

    fn lower_parameters(&mut self, csts: &[Cst], is_fuzzable: bool) -> Vec<Ast> {
        csts.iter()
            .enumerate()
            .map(|(index, it)| self.lower_parameter(index, it, is_fuzzable))
            .collect()
    }
    fn lower_parameter(&mut self, index: usize, cst: &Cst, is_fuzzable: bool) -> Ast {
        if !is_fuzzable {
            return self.lower_cst(cst, LoweringType::Pattern);
        }

        if let CstKind::Identifier(identifier) = &cst.kind {
            let identifier = self.create_string(cst.data.id, identifier.clone());
            self.create_ast(cst.data.id, Identifier(identifier))
        } else {
            let identifier = self.create_string(cst.data.id, format!("<invalid#{index}>"));
            self.create_ast(cst.data.id, Identifier(identifier));
            self.create_error_ast(
                cst,
                vec![self.create_error(cst, AstError::ExpectedParameter)],
            )
        }
    }

    fn create_ast(&mut self, cst_id: cst::Id, kind: impl Into<AstKind>) -> Ast {
        Ast {
            id: self.create_next_id(cst_id),
            kind: kind.into(),
        }
    }
    fn create_string(&mut self, cst_id: cst::Id, value: String) -> AstString {
        AstString {
            id: self.create_next_id(cst_id),
            value,
        }
    }
    fn create_next_id(&mut self, cst_id: cst::Id) -> ast::Id {
        let id = self.create_next_id_without_mapping();
        assert!(self.id_mapping.insert(id.clone(), cst_id).is_none());
        id
    }
    fn create_next_id_without_mapping(&mut self) -> ast::Id {
        let id = ast::Id::new(self.module.clone(), self.next_id);
        self.next_id += 1;
        id
    }

    // Errors
    fn create_ast_for_invalid_expression_in_pattern(&mut self, cst: &Cst) -> Ast {
        self.create_error_ast(
            cst,
            vec![self.create_error(cst, AstError::PatternContainsInvalidExpression)],
        )
    }
    fn create_errors_or_use_ast(
        &mut self,
        cst: &Cst,
        mut errors: Vec<CompilerError>,
        ast: Ast,
    ) -> Ast {
        if errors.is_empty() {
            ast
        } else {
            ast.collect_errors(&mut errors);
            self.create_error_ast(cst, errors)
        }
    }
    fn create_errors_or_ast(
        &mut self,
        cst: &Cst,
        errors: Vec<CompilerError>,
        ast: impl Into<AstKind>,
    ) -> Ast {
        let ast = self.create_ast(cst.data.id, ast.into());
        self.create_errors_or_use_ast(cst, errors, ast)
    }
    fn create_error_ast(&mut self, cst: &Cst, errors: Vec<CompilerError>) -> Ast {
        self.create_ast(cst.data.id, AstKind::Error { errors })
    }
    fn create_error(&self, cst: &Cst, error: impl Into<CompilerErrorPayload>) -> CompilerError {
        CompilerError {
            module: self.module.clone(),
            span: cst.data.span.clone(),
            payload: error.into(),
        }
    }
}
