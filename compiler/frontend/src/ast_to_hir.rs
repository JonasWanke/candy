use crate::{
    ast::{
        self, Assignment, Ast, AstKind, AstString, Call, Identifier, Int, List, MatchCase,
        OrPattern, Struct, StructAccess, Symbol, Text, TextPart,
    },
    builtin_functions::{self, BuiltinFunction},
    cst::{self, CstDb},
    cst_to_ast::CstToAst,
    error::{CompilerError, CompilerErrorPayload},
    hir::{self, Body, Expression, Function, HirError, IdKey, Pattern, PatternIdentifierId},
    id::IdGenerator,
    module::{Module, Package},
    position::Offset,
    string_to_rcst::ModuleError,
    utils::AdjustCasingOfFirstLetter,
};
use itertools::Itertools;
use rustc_hash::FxHashMap;
use std::{collections::hash_map::Entry, mem, ops::Range, sync::Arc};

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
    fn ast_to_hir_id(&self, id: &ast::Id) -> Vec<hir::Id>;
    #[salsa::transparent]
    fn cst_to_hir_id(&self, module: Module, id: &cst::Id) -> Vec<hir::Id>;

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
    Some(db.find_cst(id.module.to_owned(), cst_id).display_span())
}

fn ast_to_hir_id(db: &dyn AstToHir, id: &ast::Id) -> Vec<hir::Id> {
    if let Ok((_, hir_to_ast_id_mapping)) = db.hir(id.module.clone()) {
        hir_to_ast_id_mapping
            .iter()
            .filter_map(|(key, value)| if value == id { Some(key) } else { None })
            .cloned()
            .collect_vec()
    } else {
        vec![]
    }
}
fn cst_to_hir_id(db: &dyn AstToHir, module: Module, id: &cst::Id) -> Vec<hir::Id> {
    let ids = db.cst_to_ast_id(module, id);
    ids.into_iter()
        .flat_map(|id| db.ast_to_hir_id(&id))
        .collect_vec()
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
    let is_builtins_package = module.package == Package::builtins();
    let mut context = Context {
        module: module.clone(),
        id_mapping: FxHashMap::default(),
        db,
        public_identifiers: FxHashMap::default(),
        body: Body::default(),
        id_prefix: hir::Id::new(module, vec![]),
        identifiers: im::HashMap::new(),
        is_top_level: true,
        use_id: None,
    };

    if is_builtins_package {
        context.generate_sparkles();
    }
    context.generate_use();
    context.compile(ast);
    context.generate_exports_struct();

    let id_mapping = context
        .id_mapping
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect();
    (context.body, id_mapping)
}

struct Context<'a> {
    module: Module,
    id_mapping: FxHashMap<hir::Id, Option<ast::Id>>,
    db: &'a dyn AstToHir,
    public_identifiers: FxHashMap<String, hir::Id>,
    body: Body,
    id_prefix: hir::Id,
    identifiers: im::HashMap<String, hir::Id>,
    is_top_level: bool,
    use_id: Option<hir::Id>,
}

impl Context<'_> {
    fn start_non_top_level(&mut self) -> NonTopLevelResetState {
        NonTopLevelResetState(mem::replace(&mut self.is_top_level, false))
    }
    fn end_non_top_level(&mut self, reset_state: NonTopLevelResetState) {
        self.is_top_level = reset_state.0;
    }
}
struct NonTopLevelResetState(bool);

impl Context<'_> {
    #[must_use]
    fn start_scope(&mut self) -> ScopeResetState {
        ScopeResetState {
            body: mem::take(&mut self.body),
            id_prefix: self.id_prefix.clone(),
            identifiers: self.identifiers.clone(),
            non_top_level_reset_state: self.start_non_top_level(),
        }
    }
    #[must_use]
    fn end_scope(&mut self, reset_state: ScopeResetState) -> Body {
        let inner_body = mem::replace(&mut self.body, reset_state.body);
        self.id_prefix = reset_state.id_prefix;
        self.identifiers = reset_state.identifiers;
        self.end_non_top_level(reset_state.non_top_level_reset_state);
        inner_body
    }
}
struct ScopeResetState {
    body: Body,
    id_prefix: hir::Id,
    identifiers: im::HashMap<String, hir::Id>,
    non_top_level_reset_state: NonTopLevelResetState,
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
            AstKind::Int(Int(int)) => {
                self.push(Some(ast.id.clone()), Expression::Int(int.to_owned()), None)
            }
            AstKind::Text(text) => self.lower_text(Some(ast.id.clone()), text),
            AstKind::TextPart(TextPart(string)) => self.push(
                Some(ast.id.clone()),
                Expression::Text(string.value.to_owned()),
                None,
            ),
            AstKind::Identifier(Identifier(name)) => {
                let reference = match self.identifiers.get(&name.value) {
                    Some(reference) => reference.to_owned(),
                    None => {
                        return self.push_error(
                            Some(name.id.clone()),
                            self.db.ast_id_to_display_span(&ast.id).unwrap(),
                            HirError::UnknownReference {
                                name: name.value.clone(),
                            },
                        );
                    }
                };
                self.push(Some(ast.id.clone()), Expression::Reference(reference), None)
            }
            AstKind::Symbol(Symbol(symbol)) => self.push(
                Some(ast.id.clone()),
                Expression::Symbol(symbol.value.to_owned()),
                None,
            ),
            AstKind::List(List(items)) => {
                let hir_items = items
                    .iter()
                    .map(|item| self.compile_single(item))
                    .collect_vec();
                self.push(Some(ast.id.clone()), Expression::List(hir_items), None)
            }
            AstKind::Struct(Struct { fields }) => {
                let fields = fields
                    .iter()
                    .map(|(key, value)| {
                        let key = key
                            .as_ref()
                            .map(|key| self.compile_single(key))
                            .unwrap_or_else(|| match &value.kind {
                                AstKind::Identifier(Identifier(name)) => self.push(
                                    Some(value.id.clone()),
                                    Expression::Symbol(name.value.uppercase_first_letter()),
                                    None,
                                ),
                                AstKind::Error { errors, .. } => self.push(
                                    Some(ast.id.clone()),
                                    Expression::Error {
                                        child: None,
                                        // TODO: These errors are already reported for the value itself.
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
                self.push(Some(ast.id.clone()), Expression::Struct(fields), None)
            }
            AstKind::StructAccess(struct_access) => {
                self.lower_struct_access(Some(ast.id.clone()), struct_access)
            }
            AstKind::Function(function) => self.compile_function(ast.id.clone(), function, None),
            AstKind::Call(call) => self.lower_call(Some(ast.id.clone()), call),
            AstKind::Assignment(Assignment { is_public, body }) => {
                let (names, body) = match body {
                    ast::AssignmentBody::Function { name, function } => {
                        let name_string = name.value.to_owned();
                        let body =
                            self.compile_function(ast.id.clone(), function, Some(name_string));
                        let name_id = self.push(
                            Some(name.id.clone()),
                            Expression::Reference(body.clone()),
                            Some(name.value.to_owned()),
                        );
                        (vec![(name.value.to_owned(), name_id)], body)
                    }
                    ast::AssignmentBody::Body { pattern, body } => {
                        let reset_state = self.start_non_top_level();
                        let body = self.compile(body);
                        self.end_non_top_level(reset_state);

                        let (pattern, identifier_ids) = self.lower_pattern(pattern);
                        let body = self.push(
                            None,
                            Expression::Destructure {
                                expression: body,
                                pattern,
                            },
                            None,
                        );

                        let names = identifier_ids
                            .into_iter()
                            .sorted_by_key(|(_, (_, identifier_id))| identifier_id.0)
                            .map(|(name, (ast_id, identifier_id))| {
                                let id = self.push(
                                    Some(ast_id),
                                    Expression::PatternIdentifierReference(identifier_id),
                                    Some(name.to_owned()),
                                );
                                (name, id)
                            })
                            .collect_vec();

                        self.push(
                            Some(ast.id.clone()),
                            Expression::Symbol("Nothing".to_string()),
                            None,
                        );

                        (names, body)
                    }
                };
                if *is_public {
                    if self.is_top_level {
                        for (name, id) in names {
                            if self.public_identifiers.contains_key(&name) {
                                self.push_error(
                                    None,
                                    self.db.ast_id_to_display_span(&ast.id).unwrap(),
                                    HirError::PublicAssignmentWithSameName {
                                        name: name.to_owned(),
                                    },
                                );
                            }
                            self.public_identifiers.insert(name, id);
                        }
                    } else {
                        self.push_error(
                            None,
                            self.db.ast_id_to_display_span(&ast.id).unwrap(),
                            HirError::PublicAssignmentInNotTopLevel,
                        );
                    }
                }
                body
            }
            AstKind::Match(ast::Match { expression, cases }) => {
                let expression = self.compile_single(expression);

                let reset_state = self.start_scope();
                let match_id = self.create_next_id(Some(ast.id.clone()), None);
                self.id_prefix = match_id.clone();

                let cases = cases
                    .iter()
                    .map(|case| match &case.kind {
                        AstKind::MatchCase(MatchCase { box pattern, body }) => {
                            let (pattern, pattern_identifiers) = self.lower_pattern(pattern);

                            let reset_state = self.start_scope();
                            for (name, (ast_id, identifier_id)) in pattern_identifiers {
                                self.push(
                                    Some(ast_id),
                                    Expression::PatternIdentifierReference(identifier_id),
                                    Some(name.to_owned()),
                                );
                            }
                            self.compile(body.as_ref());
                            let body = self.end_scope(reset_state);

                            (pattern, body)
                        }
                        AstKind::Error { errors, .. } => {
                            let pattern = Pattern::Error {
                                child: None,
                                errors: errors.to_owned(),
                            };

                            let reset_state = self.start_scope();
                            self.compile(&[]);
                            let body = self.end_scope(reset_state);

                            (pattern, body)
                        }
                        _ => unreachable!("Expected match case in match cases, got {case:?}."),
                    })
                    .collect_vec();

                // The scope is only for hierarchical IDs. The actual bodies are
                // inside the cases.
                let _ = self.end_scope(reset_state);

                self.push_with_existing_id(match_id, Expression::Match { expression, cases }, None)
            }
            AstKind::MatchCase(_) => {
                unreachable!("Match cases should be handled in match directly.")
            }
            AstKind::OrPattern(_) => {
                unreachable!("Or patterns should be handled in `PatternContext`.")
            }
            AstKind::Error { child, errors } => {
                let child = child.as_ref().map(|child| self.compile_single(child));
                self.push(
                    Some(ast.id.clone()),
                    Expression::Error {
                        child,
                        errors: errors.clone(),
                    },
                    None,
                )
            }
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

                let reset_state = self.start_scope();
                let then_function_id = self.create_next_id(None, None);
                self.id_prefix = then_function_id.clone();
                self.push(None, Expression::Reference(hir.clone()), None);
                let then_body = self.end_scope(reset_state);
                let then_function = self.push_with_existing_id(
                    then_function_id,
                    Expression::Function(Function {
                        parameters: vec![],
                        body: then_body,
                        fuzzable: false,
                    }),
                    None,
                );

                let reset_state = self.start_scope();
                let else_function_id = self.create_next_id(None, None);
                self.id_prefix = else_function_id.clone();
                self.push(
                    None,
                    Expression::Call {
                        function: to_debug_text_function.clone(),
                        arguments: vec![hir],
                    },
                    None,
                );
                let else_body = self.end_scope(reset_state);
                let else_function = self.push_with_existing_id(
                    else_function_id,
                    Expression::Function(Function {
                        parameters: vec![],
                        body: else_body,
                        fuzzable: false,
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
            .unwrap_or_else(|| self.push(id, Expression::Text("".to_string()), None))
    }

    fn compile_function(
        &mut self,
        id: ast::Id,
        function: &ast::Function,
        identifier: Option<String>,
    ) -> hir::Id {
        let reset_state = self.start_scope();
        let function_id = self.create_next_id(Some(id), identifier);
        self.id_prefix = function_id.clone();

        for parameter in function.parameters.iter() {
            let name = parameter.value.to_string();
            let id = self.create_next_id(Some(parameter.id.clone()), Some(name.clone()));
            self.body.identifiers.insert(id.clone(), name.clone());
            self.identifiers.insert(name, id);
        }

        self.compile(&function.body);

        let inner_body = self.end_scope(reset_state);

        self.push_with_existing_id(
            function_id.clone(),
            Expression::Function(Function {
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| function_id.child(parameter.value.clone()))
                    .collect(),
                body: inner_body,
                fuzzable: function.fuzzable,
            }),
            None,
        )
    }

    fn lower_struct_access(
        &mut self,
        id: Option<ast::Id>,
        struct_access: &StructAccess,
    ) -> hir::Id {
        // We forward struct accesses to `(use "Builtins").structGet` to reuse
        // its validation logic. However, this only works outside the Builtins
        // package.
        let struct_get_id = if self.module.package == Package::builtins() {
            self.push(None, Expression::Builtin(BuiltinFunction::StructGet), None)
        } else {
            let builtins = self.push(None, Expression::Text("Builtins".to_string()), None);
            let builtins_id = self.push(
                None,
                Expression::Call {
                    function: self.use_id.clone().unwrap(),
                    arguments: vec![builtins],
                },
                None,
            );
            let struct_get_id =
                self.push(None, Expression::Builtin(BuiltinFunction::StructGet), None);
            let struct_get = self.push(None, Expression::Symbol("StructGet".to_string()), None);
            self.push(
                None,
                Expression::Call {
                    function: struct_get_id,
                    arguments: vec![builtins_id, struct_get],
                },
                None,
            )
        };

        let struct_ = self.compile_single(&struct_access.struct_);
        let key_id = self.push(
            Some(struct_access.key.id.clone()),
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
            identifier_id_generator: Default::default(),
            identifier_ids: Default::default(),
        };
        let pattern = context.compile_pattern(ast);
        (pattern, context.identifier_ids)
    }

    fn push(
        &mut self,
        ast_id: Option<ast::Id>,
        expression: Expression,
        identifier: Option<String>,
    ) -> hir::Id {
        let id = self.create_next_id(ast_id, identifier.clone());
        self.push_with_existing_id(id, expression, identifier)
    }
    fn push_with_existing_id(
        &mut self,
        id: hir::Id,
        expression: Expression,
        identifier: Option<String>,
    ) -> hir::Id {
        self.body
            .push(id.to_owned(), expression, identifier.clone());
        if let Some(identifier) = identifier {
            self.identifiers.insert(identifier, id.clone());
        }
        id
    }
    fn push_error(
        &mut self,
        ast_id: Option<ast::Id>,
        span: Range<Offset>,
        error: HirError,
    ) -> hir::Id {
        self.push(
            ast_id,
            Expression::Error {
                child: None,
                errors: vec![CompilerError {
                    module: self.module.clone(),
                    span,
                    payload: error.into(),
                }],
            },
            None,
        )
    }

    fn create_next_id(&mut self, ast_id: Option<ast::Id>, key: Option<String>) -> hir::Id {
        for disambiguator in 0.. {
            let last_part = if let Some(key) = &key {
                if disambiguator == 0 {
                    key.to_string().into()
                } else {
                    IdKey::Named {
                        name: key.to_string(),
                        disambiguator,
                    }
                }
            } else {
                disambiguator.into()
            };
            let id = self.id_prefix.child(last_part);
            if let Entry::Vacant(entry) = self.id_mapping.entry(id.clone()) {
                entry.insert(ast_id);
                return id;
            }
        }
        unreachable!()
    }

    fn generate_sparkles(&mut self) {
        let mut sparkles_map = FxHashMap::default();

        for builtin_function in builtin_functions::VALUES.iter() {
            let symbol = self.push(
                None,
                Expression::Symbol(format!("{builtin_function:?}")),
                None,
            );
            let builtin = self.push(None, Expression::Builtin(*builtin_function), None);
            sparkles_map.insert(symbol, builtin);
        }

        let sparkles_map = Expression::Struct(sparkles_map);
        self.push(None, sparkles_map, Some("✨".to_string()));
    }

    fn generate_use(&mut self) {
        // HirId(~:test.candy:use) = function { HirId(~:test.candy:use:relativePath) ->
        //   HirId(~:test.candy:use:importedFileContent) = useModule
        //     currently in ~:test.candy:use:importedFileContent
        //     relative path: HirId(~:test.candy:use:relativePath)
        //  }

        assert!(self.use_id.is_none());

        let reset_state = self.start_scope();
        let use_id = self.create_next_id(None, Some("use".to_string()));
        self.id_prefix = use_id.clone();
        let relative_path = use_id.child("relativePath");

        self.push(
            None,
            Expression::UseModule {
                current_module: self.module.clone(),
                relative_path: relative_path.clone(),
            },
            Some("importedModule".to_string()),
        );

        let inner_body = self.end_scope(reset_state);

        self.push_with_existing_id(
            use_id.clone(),
            Expression::Function(Function {
                parameters: vec![relative_path],
                body: inner_body,
                fuzzable: false,
            }),
            Some("use".to_string()),
        );
        self.use_id = Some(use_id);
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
            AstKind::Int(Int(int)) => Pattern::Int(int.to_owned()),
            AstKind::Text(Text(text)) => Pattern::Text(
                text.iter()
                    .map(|part| match &part.kind {
                        AstKind::TextPart(TextPart(string)) => string.value.to_owned(),
                        _ => panic!("AST pattern can't contain text interpolations."),
                    })
                    .join(""),
            ),
            AstKind::TextPart(_) => unreachable!("TextPart should not occur in AST patterns."),
            AstKind::Identifier(Identifier(name)) => {
                let (_, pattern_id) = self
                    .identifier_ids
                    .entry(name.value.to_owned())
                    .or_insert_with(|| {
                        (ast.id.to_owned(), self.identifier_id_generator.generate())
                    });
                Pattern::NewIdentifier(pattern_id.to_owned())
            }
            AstKind::Symbol(Symbol(symbol)) => Pattern::Tag {
                symbol: symbol.value.to_owned(),
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
                        let key = key
                            .as_ref()
                            .map(|key| self.compile_pattern(key))
                            .unwrap_or_else(|| match &value.kind {
                                AstKind::Identifier(Identifier(name)) => Pattern::Tag {
                                    symbol: name.value.uppercase_first_letter(),
                                    value: None,
                                },
                                AstKind::Error { errors, .. } => Pattern::Error {
                                    child: None,
                                    // TODO: These errors are already reported for the value itself.
                                    errors: errors.to_owned(),
                                },
                                _ => panic!(
                                    "Expected identifier in struct shorthand, got {value:?}."
                                ),
                            });
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
            AstKind::Error { child, errors, .. } => {
                let child = child
                    .as_ref()
                    .map(|child| Box::new(self.compile_pattern(child)));
                Pattern::Error {
                    child,
                    errors: errors.to_owned(),
                }
            }
        }
    }

    fn error(&self, ast: &Ast, error: HirError) -> Pattern {
        Pattern::Error {
            child: None,
            errors: vec![CompilerError {
                module: self.module.clone(),
                span: self.db.ast_id_to_span(&ast.id).unwrap(),
                payload: CompilerErrorPayload::Hir(error),
            }],
        }
    }
}
