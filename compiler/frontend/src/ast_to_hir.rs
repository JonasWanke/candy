use crate::{
    ast::{
        self, Assignment, Ast, AstKind, AstString, Call, Identifier, Int, List, MatchCase,
        OrPattern, Struct, StructAccess, Symbol, Text, TextPart,
    },
    builtin_functions::BuiltinFunction,
    cst::{self, CstDb},
    cst_to_ast::CstToAst,
    error::{CompilerError, CompilerErrorPayload},
    hir::{
        self, Body, Expression, Function, FunctionKind, HirError, IdKey, Pattern,
        PatternIdentifierId,
    },
    id::IdGenerator,
    module::{Module, Package},
    position::Offset,
    string_to_rcst::ModuleError,
    utils::AdjustCasingOfFirstLetter,
};
use itertools::Itertools;
use rustc_hash::FxHashMap;
use std::{collections::hash_map::Entry, mem, ops::Range, sync::Arc};
use strum::VariantArray;

#[salsa::query_group(AstToHirStorage)]
pub trait AstToHir: CstDb + CstToAst {
    #[salsa::transparent]
    fn hir_to_ast_id(&self, id: &hir::Id) -> Option<ast::Id>;
    #[salsa::transparent]
    fn hir_to_cst_id(&self, id: &hir::Id) -> Option<cst::Id>;
    #[salsa::transparent]
    fn hir_id_to_span(&self, id: &hir::Id) -> Option<Range<Offset>>;
    #[salsa::transparent]
    fn hir_id_to_display_span(&self, id: &hir::Id) -> Option<Range<Offset>>;

    #[salsa::transparent]
    fn ast_to_hir_ids(&self, id: &ast::Id) -> Vec<hir::Id>;
    #[salsa::transparent]
    fn cst_to_hir_ids(&self, module: Module, id: cst::Id) -> Vec<hir::Id>;

    // For example, an identifier in a struct pattern (`[foo]`) can correspond
    // to two HIR IDs: The implicit key `Foo` and the capturing identifier
    // `foo`. This function returns the latter.
    #[salsa::transparent]
    fn cst_to_last_hir_id(&self, module: Module, id: cst::Id) -> Option<hir::Id>;

    fn hir(&self, module: Module) -> HirResult;
}

pub type HirResult = Result<(Arc<Body>, Arc<FxHashMap<hir::Id, ast::Id>>), ModuleError>;

fn hir_to_ast_id(db: &dyn AstToHir, id: &hir::Id) -> Option<ast::Id> {
    let (_, hir_to_ast_id_mapping) = db.hir(id.module.clone()).ok()?;
    hir_to_ast_id_mapping.get(id).cloned()
}
fn hir_to_cst_id(db: &dyn AstToHir, id: &hir::Id) -> Option<cst::Id> {
    db.ast_to_cst_id(&db.hir_to_ast_id(id)?)
}
fn hir_id_to_span(db: &dyn AstToHir, id: &hir::Id) -> Option<Range<Offset>> {
    db.ast_id_to_span(&db.hir_to_ast_id(id)?)
}
fn hir_id_to_display_span(db: &dyn AstToHir, id: &hir::Id) -> Option<Range<Offset>> {
    let cst_id = db.hir_to_cst_id(id)?;
    Some(db.find_cst(id.module.clone(), cst_id).display_span())
}

fn ast_to_hir_ids(db: &dyn AstToHir, id: &ast::Id) -> Vec<hir::Id> {
    if let Ok((_, hir_to_ast_id_mapping)) = db.hir(id.module.clone()) {
        hir_to_ast_id_mapping
            .iter()
            .filter_map(|(key, value)| if value == id { Some(key) } else { None })
            .cloned()
            .sorted()
            .collect_vec()
    } else {
        vec![]
    }
}
fn cst_to_hir_ids(db: &dyn AstToHir, module: Module, id: cst::Id) -> Vec<hir::Id> {
    let ids = db.cst_to_ast_ids(module, id);
    ids.into_iter()
        .flat_map(|id| db.ast_to_hir_ids(&id))
        .sorted()
        .collect_vec()
}
fn cst_to_last_hir_id(db: &dyn AstToHir, module: Module, id: cst::Id) -> Option<hir::Id> {
    db.cst_to_hir_ids(module, id).pop()
}

fn hir(db: &dyn AstToHir, module: Module) -> HirResult {
    db.ast(module.clone()).map(|(ast, _)| {
        let (body, id_mapping) = compile_top_level(db, module, &ast);
        (Arc::new(body), Arc::new(id_mapping))
    })
}

fn compile_top_level(
    db: &dyn AstToHir,
    module: Module,
    ast: &[Ast],
) -> (Body, FxHashMap<hir::Id, ast::Id>) {
    let is_builtins_package = module.package() == &Package::builtins();
    let mut context = Context {
        module: module.clone(),
        id_mapping: FxHashMap::default(),
        db,
        public_identifiers: FxHashMap::default(),
        body: Body::default(),
        id_prefix: hir::Id::new(module, vec![]).into(),
        identifiers: im::HashMap::new(),
        is_top_level: true,
        use_id: None,
        builtins_id: None,
    };

    context.generate_use();
    if is_builtins_package {
        context.generate_sparkles();
    } else {
        context.generate_builtins();
    }
    context.compile(ast);
    context.generate_exports_struct();

    let id_mapping = context
        .id_mapping
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect();
    (context.body, id_mapping)
}

struct IdPrefix {
    id: hir::Id,
    named_disambiguators: FxHashMap<String, usize>,
    positional_disambiguator: usize,
}
impl From<hir::Id> for IdPrefix {
    fn from(value: hir::Id) -> Self {
        Self {
            id: value,
            named_disambiguators: FxHashMap::default(),
            positional_disambiguator: 0,
        }
    }
}

struct Context<'a> {
    module: Module,
    id_mapping: FxHashMap<hir::Id, Option<ast::Id>>,
    db: &'a dyn AstToHir,
    public_identifiers: FxHashMap<String, hir::Id>,
    body: Body,
    id_prefix: IdPrefix,
    identifiers: im::HashMap<String, hir::Id>,
    is_top_level: bool,
    use_id: Option<hir::Id>,
    builtins_id: Option<hir::Id>,
}

impl Context<'_> {
    fn with_non_top_level<F, T>(&mut self, func: F) -> T
    where
        F: Fn(&mut Self) -> T,
    {
        let reset_state = mem::replace(&mut self.is_top_level, false);
        let res = func(self);
        self.is_top_level = reset_state;
        res
    }

    #[must_use]
    fn with_scope<F, T>(&mut self, id_prefix: impl Into<Option<hir::Id>>, func: F) -> (Body, T)
    where
        F: Fn(&mut Self) -> T,
    {
        let reset_state = ScopeResetState {
            body: mem::take(&mut self.body),
            id_prefix: id_prefix
                .into()
                .map(|id_prefix| mem::replace(&mut self.id_prefix, id_prefix.into())),
            identifiers: self.identifiers.clone(),
        };

        let res = self.with_non_top_level(|scope| func(scope));

        let inner_body = mem::replace(&mut self.body, reset_state.body);
        if let Some(id_prefix) = reset_state.id_prefix {
            self.id_prefix = id_prefix;
        };
        self.identifiers = reset_state.identifiers;
        (inner_body, res)
    }
}
struct ScopeResetState {
    body: Body,
    id_prefix: Option<IdPrefix>,
    identifiers: im::HashMap<String, hir::Id>,
}

impl Context<'_> {
    fn compile(&mut self, asts: &[Ast]) -> hir::Id {
        if asts.is_empty() {
            self.push(None, Expression::nothing(), None)
        } else {
            let mut last_id = None;
            for ast in asts {
                last_id = Some(self.compile_single(ast));
            }
            last_id.unwrap()
        }
    }

    fn compile_single(&mut self, ast: &Ast) -> hir::Id {
        match &ast.kind {
            AstKind::Int(Int(int)) => self.push(ast.id.clone(), Expression::Int(int.clone()), None),
            AstKind::Text(text) => self.lower_text(Some(ast.id.clone()), text),
            AstKind::TextPart(TextPart(string)) => {
                self.push(ast.id.clone(), Expression::Text(string.value.clone()), None)
            }
            AstKind::Identifier(Identifier(name)) => {
                let reference = match self.identifiers.get(&name.value) {
                    Some(reference) => reference.clone(),
                    None => {
                        return self.push_error(
                            name.id.clone(),
                            self.db.ast_id_to_display_span(&ast.id).unwrap(),
                            HirError::UnknownReference {
                                name: name.value.clone(),
                            },
                        );
                    }
                };
                self.push(ast.id.clone(), Expression::Reference(reference), None)
            }
            AstKind::Symbol(Symbol(symbol)) => self.push(
                ast.id.clone(),
                Expression::Symbol(symbol.value.clone()),
                None,
            ),
            AstKind::List(List(items)) => {
                let hir_items = items
                    .iter()
                    .map(|item| self.compile_single(item))
                    .collect_vec();
                self.push(ast.id.clone(), Expression::List(hir_items), None)
            }
            AstKind::Struct(Struct { fields }) => {
                let fields = fields
                    .iter()
                    .map(|(key, value)| {
                        #[allow(clippy::map_unwrap_or)]
                        let key = key
                            .as_ref()
                            .map(|key| self.compile_single(key))
                            .unwrap_or_else(|| match &value.kind {
                                AstKind::Identifier(Identifier(name)) => self.push(
                                    value.id.clone(),
                                    Expression::Symbol(name.value.uppercase_first_letter()),
                                    None,
                                ),
                                AstKind::Error { errors } => self.push(
                                    ast.id.clone(),
                                    Expression::Error {
                                        errors: errors.clone(),
                                    },
                                    None,
                                ),
                                _ => panic!(
                                    "Expected identifier in struct shorthand, got {value:?}."
                                ),
                            });
                        (key, self.compile_single(value))
                    })
                    .collect();
                self.push(ast.id.clone(), Expression::Struct(fields), None)
            }
            AstKind::StructAccess(struct_access) => {
                self.lower_struct_access(Some(ast.id.clone()), struct_access)
            }
            AstKind::Function(function) => self.compile_function(ast.id.clone(), function, None),
            AstKind::Call(call) => self.lower_call(Some(ast.id.clone()), call),
            AstKind::Assignment(Assignment { is_public, body }) => {
                // An assignment to a single identifier (i.e., no destructuring)
                // gets converted to at least two HIR expressions:
                //
                // - The penultimate one is mapped to the whole AST
                // - The last is a reference to the penultimate one and gets
                //   mapped to the identifier's AST.
                //
                // This is necessary to differentiate assignments and references
                // for IDE features.
                let (names, body) = match body {
                    ast::AssignmentBody::Function { name, function } => {
                        let body = self.compile_function(ast.id.clone(), function, &***name);
                        let name_id = self.push(
                            name.id.clone(),
                            Expression::Reference(body.clone()),
                            name.value.clone(),
                        );
                        (vec![(name.value.clone(), name.id.clone(), name_id)], body)
                    }
                    ast::AssignmentBody::Body { pattern, body } => {
                        let body = self.with_non_top_level(|scope| scope.compile(body));

                        let names = if let AstKind::Identifier(Identifier(name)) = &pattern.kind {
                            let body_reference_id = self.push(
                                ast.id.clone(),
                                Expression::Reference(body),
                                name.value.clone(),
                            );
                            let assignment_reference_id = self.push(
                                name.id.clone(),
                                Expression::Reference(body_reference_id),
                                name.value.clone(),
                            );
                            vec![(name.value.clone(), name.id.clone(), assignment_reference_id)]
                        } else {
                            let pattern_id = pattern.id.clone();
                            let (pattern, identifier_ids) = self.lower_pattern(pattern);
                            self.push(
                                pattern_id,
                                Expression::Destructure {
                                    expression: body,
                                    pattern,
                                },
                                None,
                            );

                            identifier_ids
                                .into_iter()
                                .sorted_by_key(|(_, (_, identifier_id))| identifier_id.0)
                                .map(|(name, (ast_id, identifier_id))| {
                                    let id = self.push(
                                        ast_id.clone(),
                                        Expression::PatternIdentifierReference(identifier_id),
                                        name.clone(),
                                    );
                                    (name, ast_id, id)
                                })
                                .collect_vec()
                        };

                        let nothing_id = self.push(
                            ast.id.clone(),
                            Expression::Symbol("Nothing".to_string()),
                            None,
                        );

                        (names, nothing_id)
                    }
                };
                if *is_public {
                    if self.is_top_level {
                        for (name, ast_id, id) in names {
                            if let Entry::Vacant(entry) =
                                self.public_identifiers.entry(name.clone())
                            {
                                entry.insert(id);
                            } else {
                                self.push_error(
                                    ast_id.clone(),
                                    self.db.ast_id_to_display_span(&ast_id).unwrap(),
                                    HirError::PublicAssignmentWithSameName { name },
                                );
                            }
                        }
                    } else {
                        self.push_error(
                            ast.id.clone(),
                            self.db.ast_id_to_display_span(&ast.id).unwrap(),
                            HirError::PublicAssignmentInNotTopLevel,
                        );
                    }
                }
                body
            }
            AstKind::Match(ast::Match { expression, cases }) => {
                let expression = self.compile_single(expression);

                // The scope is only for hierarchical IDs. The actual bodies are
                // inside the cases.
                let match_id = self.create_next_id(ast.id.clone(), None);
                let (_, cases) = self.with_scope(match_id.clone(), |scope| {
                    cases
                        .iter()
                        .map(|case| match &case.kind {
                            AstKind::MatchCase(MatchCase {
                                box pattern,
                                condition,
                                body,
                            }) => {
                                let (pattern, pattern_identifiers) = scope.lower_pattern(pattern);

                                let (outer_body, (condition_body, body)) =
                                    scope.with_scope(None, |scope| {
                                        for (name, (ast_id, identifier_id)) in
                                            pattern_identifiers.clone()
                                        {
                                            scope.push(
                                                ast_id,
                                                Expression::PatternIdentifierReference(
                                                    identifier_id,
                                                ),
                                                name.clone(),
                                            );
                                        }

                                        let condition = condition.as_ref().map(|box condition| {
                                            scope
                                                .with_scope(match_id.clone(), |condition_scope| {
                                                    condition_scope.compile_single(condition);
                                                })
                                                .0
                                        });

                                        let (body, ()) =
                                            scope.with_scope(match_id.clone(), |scope| {
                                                scope.compile(body);
                                            });

                                        (condition, body)
                                    });

                                hir::MatchCase {
                                    pattern,
                                    identifier_expressions: outer_body,
                                    condition: condition_body,
                                    body,
                                }
                            }
                            AstKind::Error { errors } => {
                                let pattern = Pattern::Error {
                                    errors: errors.clone(),
                                };

                                let (body, inner_body) = scope.with_scope(None, |scope| {
                                    scope.with_scope(match_id.clone(), |_scope| {}).0
                                });

                                hir::MatchCase {
                                    pattern,
                                    identifier_expressions: body,
                                    condition: None,
                                    body: inner_body,
                                }
                            }
                            _ => unreachable!("Expected match case in match cases, got {case:?}."),
                        })
                        .collect_vec()
                });

                self.push_with_existing_id(match_id, Expression::Match { expression, cases }, None)
            }
            AstKind::MatchCase(_) => {
                unreachable!("Match cases should be handled in match directly.")
            }
            AstKind::OrPattern(_) => {
                unreachable!("Or patterns should be handled in `PatternContext`.")
            }
            AstKind::Error { errors } => self.push(
                ast.id.clone(),
                Expression::Error {
                    errors: errors.clone(),
                },
                None,
            ),
        }
    }

    fn lower_text(&mut self, id: Option<ast::Id>, text: &Text) -> hir::Id {
        let text_concatenate_function = self.push(
            None,
            Expression::Builtin(BuiltinFunction::TextConcatenate),
            None,
        );
        let type_of_function = self.push(None, Expression::Builtin(BuiltinFunction::TypeOf), None);
        let text_symbol = self.push(None, Expression::Symbol("Text".to_string()), None);
        let equals_function = self.push(None, Expression::Builtin(BuiltinFunction::Equals), None);
        let if_else_function = self.push(None, Expression::Builtin(BuiltinFunction::IfElse), None);
        let to_debug_text_function = self.push(
            None,
            Expression::Builtin(BuiltinFunction::ToDebugText),
            None,
        );

        let compiled_parts = text
            .0
            .iter()
            .map(|part| {
                let hir = self.compile_single(part);
                if part.kind.is_text_part() {
                    return hir;
                }

                // Convert the part to text if it is not already a text.
                let type_of = self.push(
                    None,
                    Expression::Call {
                        function: type_of_function.clone(),
                        arguments: vec![hir.clone()],
                    },
                    None,
                );
                let is_text = self.push(
                    None,
                    Expression::Call {
                        function: equals_function.clone(),
                        arguments: vec![type_of, text_symbol.clone()],
                    },
                    None,
                );
                let then_function_id = self.create_next_id(None, None);
                let (then_body, ()) = self.with_scope(then_function_id.clone(), |scope| {
                    scope.push(None, Expression::Reference(hir.clone()), None);
                });
                let then_function = self.push_with_existing_id(
                    then_function_id,
                    Expression::Function(Function {
                        parameters: vec![],
                        body: then_body,
                        kind: FunctionKind::CurlyBraces,
                    }),
                    None,
                );

                let else_function_id = self.create_next_id(None, None);
                let (else_body, ()) = self.with_scope(else_function_id.clone(), |scope| {
                    scope.push(
                        None,
                        Expression::Call {
                            function: to_debug_text_function.clone(),
                            arguments: vec![hir.clone()],
                        },
                        None,
                    );
                });
                let else_function = self.push_with_existing_id(
                    else_function_id,
                    Expression::Function(Function {
                        parameters: vec![],
                        body: else_body,
                        kind: FunctionKind::CurlyBraces,
                    }),
                    None,
                );

                self.push(
                    None,
                    Expression::Call {
                        function: if_else_function.clone(),
                        arguments: vec![is_text, then_function, else_function],
                    },
                    None,
                )
            })
            .collect_vec();

        compiled_parts
            .into_iter()
            .reduce(|left, right| {
                self.push(
                    None,
                    Expression::Call {
                        function: text_concatenate_function.clone(),
                        arguments: vec![left, right],
                    },
                    None,
                )
            })
            .unwrap_or_else(|| self.push(id, Expression::Text(String::new()), None))
    }

    fn compile_function(
        &mut self,
        id: ast::Id,
        function: &ast::Function,
        identifier: impl Into<Option<&str>>,
    ) -> hir::Id {
        let function_id = self.create_next_id(id, identifier);
        let (inner_body, parameters) = self.with_scope(function_id.clone(), |scope| {
            // TODO: Error on parameters with same name
            let mut parameters = Vec::with_capacity(function.parameters.len());
            for parameter in &function.parameters {
                if let AstKind::Identifier(Identifier(parameter)) = &parameter.kind {
                    let name = parameter.value.to_string();
                    parameters.push(scope.id_prefix.id.child(name.clone()));

                    let id = scope.create_next_id(parameter.id.clone(), &*name);
                    scope.body.identifiers.insert(id.clone(), name.clone());
                    scope.identifiers.insert(name, id);
                } else {
                    let parameter_id = scope.create_next_id(parameter.id.clone(), None);
                    parameters.push(parameter_id.clone());

                    let (pattern, identifier_ids) = scope.lower_pattern(parameter);
                    scope.push(
                        None,
                        Expression::Destructure {
                            expression: parameter_id,
                            pattern,
                        },
                        None,
                    );

                    for (name, (ast_id, identifier_id)) in identifier_ids
                        .into_iter()
                        .sorted_by_key(|(_, (_, identifier_id))| identifier_id.0)
                    {
                        scope.push(
                            ast_id,
                            Expression::PatternIdentifierReference(identifier_id),
                            name.clone(),
                        );
                    }
                }
            }

            scope.compile(&function.body);

            parameters
        });

        self.push_with_existing_id(
            function_id,
            Expression::Function(Function {
                parameters,
                body: inner_body,
                kind: if function.fuzzable {
                    FunctionKind::Normal
                } else {
                    FunctionKind::CurlyBraces
                },
            }),
            None,
        )
    }

    fn lower_struct_access(
        &mut self,
        id: Option<ast::Id>,
        struct_access: &StructAccess,
    ) -> hir::Id {
        // We forward struct accesses to `builtins.structGet` to reuse its
        // validation logic. However, this only works outside the Builtins
        // package.
        let struct_get_id = if self.module.package() == &Package::builtins() {
            self.push(None, Expression::Builtin(BuiltinFunction::StructGet), None)
        } else {
            let struct_get_id =
                self.push(None, Expression::Builtin(BuiltinFunction::StructGet), None);
            let struct_get = self.push(None, Expression::Symbol("StructGet".to_string()), None);
            self.push(
                None,
                Expression::Call {
                    function: struct_get_id,
                    arguments: vec![self.builtins_id.clone().unwrap(), struct_get],
                },
                None,
            )
        };

        let struct_ = self.compile_single(&struct_access.struct_);
        let key_id = self.push(
            struct_access.key.id.clone(),
            Expression::Symbol(struct_access.key.value.uppercase_first_letter()),
            None,
        );
        self.push(
            id,
            Expression::Call {
                function: struct_get_id,
                arguments: vec![struct_, key_id],
            },
            None,
        )
    }

    fn lower_call(&mut self, id: Option<ast::Id>, call: &Call) -> hir::Id {
        let (mut arguments, uncompiled_arguments) = if call.is_from_pipe {
            let [first_argument, remaining @ ..] = &call.arguments[..] else {
                panic!("Calls that are generated from the pipe operator must have at least one argument");
            };
            (vec![(self.compile_single(first_argument))], remaining)
        } else {
            (vec![], &call.arguments[..])
        };
        let function = match &call.receiver.kind {
            AstKind::Identifier(Identifier(AstString {
                id: name_id,
                value: name,
            })) if name == "needs" => {
                let expression = match &self.lower_call_arguments(&call.arguments[..])[..] {
                    [condition, reason] => Expression::Needs {
                        condition: condition.clone(),
                        reason: reason.clone(),
                    },
                    [condition] => Expression::Needs {
                        condition: condition.clone(),
                        reason: self.push(
                            None,
                            Expression::Text(match self.db.ast_id_to_span(&call.arguments[0].id) {
                                Some(span) => format!(
                                    "`{}` was not satisfied",
                                    &self
                                        .db
                                        .get_module_content_as_string(
                                            call.arguments[0].id.module.clone()
                                        )
                                        .unwrap()[*span.start..*span.end],
                                ),
                                None => "the needs of a function were not met".to_string(),
                            }),
                            None,
                        ),
                    },
                    _ => {
                        return self.push_error(
                            id,
                            self.db.ast_id_to_span(name_id).unwrap(),
                            HirError::NeedsWithWrongNumberOfArguments {
                                num_args: call.arguments.len(),
                            },
                        );
                    }
                };
                return self.push(id, expression, None);
            }
            _ => self.compile_single(call.receiver.as_ref()),
        };
        arguments.extend(self.lower_call_arguments(uncompiled_arguments));
        self.push(
            id,
            Expression::Call {
                function,
                arguments,
            },
            None,
        )
    }
    fn lower_call_arguments(&mut self, arguments: &[Ast]) -> Vec<hir::Id> {
        arguments
            .iter()
            .map(|argument| self.compile_single(argument))
            .collect_vec()
    }

    fn lower_pattern(&mut self, ast: &Ast) -> (Pattern, PatternIdentifierIds) {
        let mut context = PatternContext {
            db: self.db,
            module: self.module.clone(),
            identifier_id_generator: IdGenerator::default(),
            identifier_ids: FxHashMap::default(),
        };
        let pattern = context.compile_pattern(ast);
        (pattern, context.identifier_ids)
    }

    fn push(
        &mut self,
        ast_id: impl Into<Option<ast::Id>>,
        expression: Expression,
        identifier: impl Into<Option<String>>,
    ) -> hir::Id {
        let identifier = identifier.into();
        let id = self.create_next_id(ast_id, identifier.as_deref());
        self.push_with_existing_id(id, expression, identifier)
    }
    fn push_with_existing_id(
        &mut self,
        id: hir::Id,
        expression: Expression,
        identifier: impl Into<Option<String>>,
    ) -> hir::Id {
        let identifier = identifier.into();
        self.body.push(id.clone(), expression, identifier.clone());
        if let Some(identifier) = identifier {
            self.identifiers.insert(identifier, id.clone());
        }
        id
    }
    fn push_error(
        &mut self,
        ast_id: impl Into<Option<ast::Id>>,
        span: Range<Offset>,
        error: HirError,
    ) -> hir::Id {
        self.push(
            ast_id,
            Expression::Error {
                errors: vec![CompilerError {
                    module: self.module.clone(),
                    span,
                    payload: error.into(),
                }],
            },
            None,
        )
    }

    fn create_next_id(
        &mut self,
        ast_id: impl Into<Option<ast::Id>>,
        key: impl Into<Option<&str>>,
    ) -> hir::Id {
        let key = key.into();
        let last_part = key.map_or_else(
            || {
                let disambiguator = self.id_prefix.positional_disambiguator;
                self.id_prefix.positional_disambiguator = disambiguator + 1;
                disambiguator.into()
            },
            |key| match self
                .id_prefix
                .named_disambiguators
                .entry((*key).to_string())
            {
                Entry::Occupied(mut entry) => {
                    let disambiguator = *entry.get();
                    entry.insert(disambiguator + 1);
                    IdKey::Named {
                        name: (*key).to_string(),
                        disambiguator,
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(1);
                    (*key).to_string().into()
                }
            },
        );
        let id = self.id_prefix.id.child(last_part);
        let Entry::Vacant(entry) = self.id_mapping.entry(id.clone()) else {
            unreachable!()
        };
        entry.insert(ast_id.into());
        id
    }

    fn generate_sparkles(&mut self) {
        let mut sparkles_map = FxHashMap::default();

        for builtin_function in BuiltinFunction::VARIANTS {
            let symbol = self.push(
                None,
                Expression::Symbol(format!("{builtin_function:?}")),
                None,
            );
            let builtin = self.push(None, Expression::Builtin(*builtin_function), None);
            sparkles_map.insert(symbol, builtin);
        }

        let sparkles_map = Expression::Struct(sparkles_map);
        self.push(None, sparkles_map, "✨".to_string());
    }

    fn generate_use(&mut self) {
        // HirId(~:test.candy:use) = function { HirId(~:test.candy:use:relativePath) ->
        //   HirId(~:test.candy:use:importedFileContent) = useModule
        //     currently in ~:test.candy:use:importedFileContent
        //     relative path: HirId(~:test.candy:use:relativePath)
        // }

        assert!(self.use_id.is_none());

        let use_id = self.create_next_id(None, "use");
        let (inner_body, relative_path) = self.with_scope(use_id.clone(), |scope| {
            let relative_path = scope.id_prefix.id.child("relativePath");
            scope.push(
                None,
                Expression::UseModule {
                    current_module: scope.module.clone(),
                    relative_path: relative_path.clone(),
                },
                "importedModule".to_string(),
            );
            relative_path
        });

        self.push_with_existing_id(
            use_id.clone(),
            Expression::Function(Function {
                parameters: vec![relative_path],
                body: inner_body,
                kind: FunctionKind::Use,
            }),
            "use".to_string(),
        );
        self.use_id = Some(use_id);
    }

    fn generate_builtins(&mut self) {
        // HirId(~:test.candy:0) = call HirId(~:test.candy:use) "Builtins"

        assert!(self.builtins_id.is_none());

        let builtins_text = self.push(None, Expression::Text("Builtins".to_string()), None);
        let builtins_id = self.push(
            None,
            Expression::Call {
                function: self.use_id.clone().unwrap(),
                arguments: vec![builtins_text],
            },
            None,
        );
        self.builtins_id = Some(builtins_id);
    }

    fn generate_exports_struct(&mut self) -> hir::Id {
        // HirId(~:test.candy:100) = symbol Foo
        // HirId(~:test.candy:102) = struct [
        //   HirId(~:test.candy:100): HirId(~:test.candy:101),
        // ]

        let mut exports = FxHashMap::default();
        for (name, id) in self.public_identifiers.clone() {
            exports.insert(
                self.push(
                    None,
                    Expression::Symbol(name.uppercase_first_letter()),
                    None,
                ),
                id,
            );
        }
        self.push(None, Expression::Struct(exports), None)
    }
}

/// The `ast::Id` is the ID of the first occurrence of this identifier in the
/// AST.
type PatternIdentifierIds = FxHashMap<String, (ast::Id, PatternIdentifierId)>;

struct PatternContext<'a> {
    db: &'a dyn AstToHir,
    module: Module,
    identifier_id_generator: IdGenerator<PatternIdentifierId>,
    identifier_ids: PatternIdentifierIds,
}
impl<'a> PatternContext<'a> {
    fn compile_pattern(&mut self, ast: &Ast) -> Pattern {
        match &ast.kind {
            AstKind::Int(Int(int)) => Pattern::Int(int.clone()),
            AstKind::Text(Text(text)) => Pattern::Text(
                text.iter()
                    .map(|part| match &part.kind {
                        AstKind::TextPart(TextPart(string)) => string.value.clone(),
                        _ => panic!("AST pattern can't contain text interpolations."),
                    })
                    .join(""),
            ),
            AstKind::TextPart(_) => unreachable!("TextPart should not occur in AST patterns."),
            AstKind::Identifier(Identifier(name)) => {
                let (_, pattern_id) = self
                    .identifier_ids
                    .entry(name.value.clone())
                    .or_insert_with(|| (ast.id.clone(), self.identifier_id_generator.generate()));
                Pattern::NewIdentifier(*pattern_id)
            }
            AstKind::Symbol(Symbol(symbol)) => Pattern::Tag {
                symbol: symbol.value.clone(),
                value: None,
            },
            AstKind::List(List(items)) => {
                let items = items
                    .iter()
                    .map(|item| self.compile_pattern(item))
                    .collect_vec();
                Pattern::List(items)
            }
            AstKind::Struct(Struct { fields }) => {
                let fields = fields
                    .iter()
                    .map(|(key, value)| {
                        let key = key.as_ref().map_or_else(
                            || match &value.kind {
                                AstKind::Identifier(Identifier(name)) => Pattern::Tag {
                                    symbol: name.value.uppercase_first_letter(),
                                    value: None,
                                },
                                AstKind::Error { errors, .. } => Pattern::Error {
                                    errors: errors.clone(),
                                },
                                _ => panic!(
                                    "Expected identifier in struct shorthand, got {value:?}."
                                ),
                            },
                            |key| self.compile_pattern(key),
                        );
                        (key, self.compile_pattern(value))
                    })
                    .collect();
                Pattern::Struct(fields)
            }
            AstKind::Call(call) => {
                let receiver = self.compile_pattern(&call.receiver);
                let Pattern::Tag { symbol, value } = receiver else {
                    return self.error(ast, HirError::PatternContainsCall);
                };
                if value.is_some() {
                    return self.error(ast, HirError::PatternContainsCall);
                }
                if call.arguments.len() != 1 {
                    return self.error(ast, HirError::PatternContainsCall);
                }

                Pattern::Tag {
                    symbol,
                    value: Some(Box::new(self.compile_pattern(&call.arguments[0]))),
                }
            }
            AstKind::StructAccess(_)
            | AstKind::Function(_)
            | AstKind::Assignment(_)
            | AstKind::Match(_)
            | AstKind::MatchCase(_) => {
                panic!(
                    "AST pattern can't contain struct access, function, call, assignment, match, or match case, but found {ast:?}."
                )
            }
            AstKind::OrPattern(OrPattern(patterns)) => {
                let patterns = patterns
                    .iter()
                    .map(|pattern| self.compile_pattern(pattern))
                    .collect();
                Pattern::Or(patterns)
            }
            AstKind::Error { errors, .. } => Pattern::Error {
                errors: errors.clone(),
            },
        }
    }

    fn error(&self, ast: &Ast, error: HirError) -> Pattern {
        Pattern::Error {
            errors: vec![CompilerError {
                module: self.module.clone(),
                span: self.db.ast_id_to_span(&ast.id).unwrap(),
                payload: CompilerErrorPayload::Hir(error),
            }],
        }
    }
}
