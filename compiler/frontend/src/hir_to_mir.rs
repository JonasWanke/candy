use super::{
    ast_to_hir::AstToHir,
    cst::CstDb,
    error::CompilerError,
    hir,
    mir::{BodyBuilder, Expression, Id, Mir},
    tracing::TracingConfig,
};
use crate::{
    builtin_functions::BuiltinFunction,
    module::{Module, ModuleKind},
    position::PositionConversionDb,
    string_to_rcst::ModuleError,
    tracing::CallTracingMode,
};
use itertools::Itertools;
use linked_hash_map::LinkedHashMap;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ExecutionTarget {
    Module(Module),
    MainFunction(Module),
}
impl ExecutionTarget {
    #[must_use]
    pub const fn module(&self) -> &Module {
        match &self {
            Self::Module(module) => module,
            Self::MainFunction(module) => module,
        }
    }
}

impl Display for ExecutionTarget {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Module(module) => write!(f, "module `{module}`"),
            Self::MainFunction(module) => write!(f, "main function of module `{module}`"),
        }
    }
}

#[salsa::query_group(HirToMirStorage)]
pub trait HirToMir: PositionConversionDb + CstDb + AstToHir {
    fn mir(&self, target: ExecutionTarget, tracing: TracingConfig) -> MirResult;
}

pub type MirResult = Result<(Arc<Mir>, Arc<FxHashSet<CompilerError>>), ModuleError>;

#[allow(clippy::needless_pass_by_value)]
fn mir(db: &dyn HirToMir, target: ExecutionTarget, tracing: TracingConfig) -> MirResult {
    let (module, target_is_main_function) = match target {
        ExecutionTarget::Module(module) => (module, false),
        ExecutionTarget::MainFunction(module) => {
            assert_eq!(module.kind(), ModuleKind::Code);
            (module, true)
        }
    };
    let (mir, errors) = match module.kind() {
        ModuleKind::Code => {
            let (hir, _) = db.hir(module.clone())?;
            let mut errors = FxHashSet::default();
            let mir = LoweringContext::compile_module(
                module,
                target_is_main_function,
                &hir,
                tracing,
                &mut errors,
            );
            (mir, errors)
        }
        ModuleKind::Asset => {
            let Some(bytes) = db.get_module_content(module) else {
                return Err(ModuleError::DoesNotExist);
            };
            (
                Mir::build(|body| {
                    let bytes = bytes.iter().map(|&it| body.push_int(it)).collect_vec();
                    body.push_list(bytes);
                }),
                FxHashSet::default(),
            )
        }
    };
    Ok((Arc::new(mir), Arc::new(errors)))
}

/// In the MIR, there's no longer the concept of needs. Instead, HIR IDs are
/// first-class expressions and there's a `panic` expression that takes a HIR
/// ID that's responsible.
///
/// This function generates the `needs` function. Unlike regular functions, it
/// also expects a HIR ID as a normal parameter.
///
/// Here's a high-level pseudocode of the generated `needs` function:
///
/// ```pseudocode
/// needs = { condition reason responsibleForCondition (responsibleForCall) ->
///   isConditionBool = builtinIfElse
///     builtinEquals condition True
///     { True }
///     { builtinEquals condition False }
///   builtinIfElse isConditionBool { Nothing } {
///     panic "The condition must be either `True` or `False`." responsibleForCall
///   }
///
///   builtinIfElse (builtinEquals (builtinTypeOf reason) Text) { Nothing} {
///     panic "The `reason` must be a text." responsibleForCall
///   }
///
///   builtinIfElse condition { Nothing } { panic reason responsibleForCondition }
/// }
/// ```
fn generate_needs_function(body: &mut BodyBuilder) -> Id {
    let needs_id = hir::Id::needs();
    body.push_function(needs_id.clone(), |body, responsible_for_call| {
        let condition = body.new_parameter();
        let reason = body.new_parameter();
        let responsible_for_condition = body.new_parameter();

        // Common stuff.
        let needs_code = body.push_hir_id(needs_id.clone());
        let builtin_equals = body.push_builtin(BuiltinFunction::Equals);

        // Make sure the condition is a bool.
        let is_condition_bool = body.push_is_bool(&needs_id, condition, needs_code);
        body.push_if_not(
            &needs_id.child("isConditionBool"),
            is_condition_bool,
            |body| {
                let panic_reason =
                    body.push_text("The `condition` must be either `True` or `False`.".to_string());
                body.push_panic(panic_reason, responsible_for_call);
            },
            needs_code,
        );

        // Make sure the reason is a text.
        let builtin_type_of = body.push_builtin(BuiltinFunction::TypeOf);
        let type_of_reason = body.push_call(builtin_type_of, vec![reason], responsible_for_call);
        let text_tag = body.push_tag("Text".to_string(), None);
        let is_reason_text = body.push_call(
            builtin_equals,
            vec![type_of_reason, text_tag],
            responsible_for_call,
        );
        body.push_if_not(
            &needs_id.child("isReasonText"),
            is_reason_text,
            |body| {
                let panic_reason = body.push_text("The `reason` must be a text.".to_string());
                body.push_panic(panic_reason, responsible_for_call);
            },
            needs_code,
        );

        // The core logic of the needs.
        body.push_if_not(
            &needs_id.child("condition"),
            condition,
            |body| {
                body.push_panic(reason, responsible_for_condition);
            },
            needs_code,
        );
    })
}

struct LoweringContext<'a> {
    mapping: &'a mut FxHashMap<hir::Id, Id>,
    needs_function: Id,
    tracing: TracingConfig,
    ongoing_destructuring: Option<OngoingDestructuring>,
    errors: &'a mut FxHashSet<CompilerError>,
}
#[derive(Clone)]
struct OngoingDestructuring {
    result: Id,

    /// Assignments such as `foo = …` or simple match patters (something % foo -> ...) are considered trivial.
    is_trivial: bool,
}

impl<'a> LoweringContext<'a> {
    fn compile_module(
        module: Module,
        target_is_main_function: bool,
        hir: &hir::Body,
        tracing: TracingConfig,
        errors: &mut FxHashSet<CompilerError>,
    ) -> Mir {
        Mir::build(|body| {
            let mut mapping = FxHashMap::default();

            let needs_function = generate_needs_function(body);

            let module_hir_id = hir::Id::new(module, vec![]);
            let module_id = body.push_hir_id(module_hir_id.clone());
            let mut context = LoweringContext {
                mapping: &mut mapping,
                needs_function,
                tracing,
                ongoing_destructuring: None,
                errors,
            };
            context.compile_expressions(body, module_id, &hir.expressions);

            if target_is_main_function {
                let export_struct = body.current_return_value();
                LoweringContext::compile_get_main_function_from_export_struct(
                    body,
                    &module_hir_id,
                    module_id,
                    export_struct,
                );
            }
        })
    }
    fn compile_get_main_function_from_export_struct(
        body: &mut BodyBuilder,
        module_hir_id: &hir::Id,
        module_id: Id,
        export_struct: Id,
    ) {
        let struct_has_key_function = body.push_builtin(BuiltinFunction::StructHasKey);
        let main_tag = body.push_tag("Main".to_string(), None);
        let export_contains_main_function = body.push_call(
            struct_has_key_function,
            vec![export_struct, main_tag],
            module_id,
        );
        let reason = body.push_text("The module doesn't export a main function.".to_string());
        body.push_panic_if_false(
            module_hir_id,
            export_contains_main_function,
            reason,
            module_id,
        );

        let struct_get_function = body.push_builtin(BuiltinFunction::StructGet);
        let main_function = body.push_call(
            struct_get_function,
            vec![export_struct, main_tag],
            module_id,
        );

        let type_of_function = body.push_builtin(BuiltinFunction::TypeOf);
        let type_of_main = body.push_call(type_of_function, vec![main_function], module_id);
        let equals_function = body.push_builtin(BuiltinFunction::Equals);
        let function_tag = body.push_tag("Function".to_string(), None);
        let type_of_main_equals_function =
            body.push_call(equals_function, vec![type_of_main, function_tag], module_id);
        let reason = body.push_text("The exported main value is not a function.".to_string());
        body.push_panic_if_false(
            module_hir_id,
            type_of_main_equals_function,
            reason,
            module_id,
        );

        let get_argument_count_function = body.push_builtin(BuiltinFunction::GetArgumentCount);
        let main_function_parameter_count =
            body.push_call(get_argument_count_function, vec![main_function], module_id);
        let one = body.push_int(1);
        let main_function_parameter_count_is_one = body.push_call(
            equals_function,
            vec![main_function_parameter_count, one],
            module_id,
        );
        let reason = body.push_text(
            "The exported main function doesn't accept exactly one parameter.".to_string(),
        );
        body.push_panic_if_false(
            module_hir_id,
            main_function_parameter_count_is_one,
            reason,
            module_id,
        );

        body.push_reference(main_function);
    }

    fn compile_expressions(
        &mut self,
        body: &mut BodyBuilder,
        responsible_for_needs: Id,
        expressions: &LinkedHashMap<hir::Id, hir::Expression>,
    ) {
        for (id, expression) in expressions {
            self.compile_expression(body, responsible_for_needs, id, expression);
        }
    }
    fn compile_expression(
        &mut self,
        body: &mut BodyBuilder,
        responsible_for_needs: Id,
        hir_id: &hir::Id,
        expression: &hir::Expression,
    ) {
        let id = match expression {
            hir::Expression::Int(int) => body.push_int(int.clone()),
            hir::Expression::Text(text) => body.push_text(text.clone()),
            hir::Expression::Reference(reference) => body.push_reference(self.mapping[reference]),
            hir::Expression::Symbol(symbol) => body.push_tag(symbol.clone(), None),
            hir::Expression::Builtin(builtin) => body.push_builtin(*builtin),
            hir::Expression::List(items) => {
                body.push_list(items.iter().map(|item| self.mapping[item]).collect())
            }
            hir::Expression::Struct(fields) => {
                let fields = fields
                    .iter()
                    .map(|(key, value)| (self.mapping[key], self.mapping[value]))
                    .collect();
                body.push_struct(fields)
            }
            hir::Expression::Destructure {
                expression,
                pattern,
            } => {
                let responsible = body.push_hir_id(hir_id.clone());
                let expression = self.mapping[expression];

                if let hir::Pattern::NewIdentifier(_) = pattern {
                    // The trivial case: `foo = …`.
                    let result = body.push_reference(expression);
                    self.ongoing_destructuring = Some(OngoingDestructuring {
                        result,
                        is_trivial: true,
                    });
                    result
                } else {
                    let pattern_result = PatternLoweringContext::check_pattern(
                        body,
                        hir_id.clone(),
                        responsible,
                        expression,
                        pattern,
                    );
                    self.ongoing_destructuring = Some(OngoingDestructuring {
                        result: pattern_result,
                        is_trivial: false,
                    });

                    let is_match = body.push_is_match(pattern_result, responsible);
                    body.push_if_not(
                        &hir_id.child("isMatch"),
                        is_match,
                        |body| {
                            let reason = body.push_text("The value doesn't match the pattern on the left side of the destructuring.".to_string());
                            body.push_panic(reason, responsible);
                        },
                        responsible,
                    )
                }
            }
            hir::Expression::PatternIdentifierReference(identifier_id) => {
                let OngoingDestructuring { result, is_trivial } =
                    self.ongoing_destructuring.clone().unwrap();

                if is_trivial {
                    body.push_reference(result)
                } else {
                    let responsible = body.push_hir_id(hir_id.clone());
                    let tag_get_value = body.push_builtin(BuiltinFunction::TagGetValue);
                    let captured = body.push_call(tag_get_value, vec![result], responsible);
                    let list_get = body.push_builtin(BuiltinFunction::ListGet);
                    let index = body.push_int(identifier_id.0);
                    body.push_call(list_get, vec![captured, index], responsible)
                }
            }
            hir::Expression::Match { expression, cases } => {
                assert!(!cases.is_empty());

                let responsible_for_match = body.push_hir_id(hir_id.clone());
                let expression = self.mapping[expression];
                self.compile_match(
                    hir_id.clone(),
                    body,
                    expression,
                    cases,
                    responsible_for_needs,
                    responsible_for_match,
                )
            }
            hir::Expression::Function(hir::Function {
                parameters: original_parameters,
                body: original_body,
                kind,
            }) => {
                let function =
                    body.push_function(hir_id.clone(), |function, responsible_parameter| {
                        for original_parameter in original_parameters {
                            let parameter = function.new_parameter();
                            self.mapping.insert(original_parameter.clone(), parameter);
                        }

                        let responsible = if kind.uses_own_responsibility() {
                            responsible_parameter
                        } else {
                            // This is a function with curly braces, so whoever is responsible
                            // for `needs` in the current scope is also responsible for
                            // `needs` in the function.
                            responsible_for_needs
                        };

                        self.compile_expressions(function, responsible, &original_body.expressions);
                    });

                if self.tracing.register_fuzzables.is_enabled() && kind.is_fuzzable() {
                    let hir_definition = body.push(Expression::HirId(hir_id.clone()));
                    body.push(Expression::TraceFoundFuzzableFunction {
                        hir_definition,
                        function,
                    });
                    body.push_reference(function)
                } else {
                    function
                }
            }
            hir::Expression::Call {
                function,
                arguments,
            } => {
                let responsible = body.push_hir_id(hir_id.clone());
                let arguments = arguments
                    .iter()
                    .map(|argument| self.mapping[argument])
                    .collect_vec();

                let builtin_equals = body.push_builtin(BuiltinFunction::Equals);
                let builtin_get_argument_count =
                    body.push_builtin(BuiltinFunction::GetArgumentCount);
                let builtin_tag_has_value = body.push_builtin(BuiltinFunction::TagHasValue);
                let builtin_tag_with_value = body.push_builtin(BuiltinFunction::TagWithValue);
                let builtin_text_concatenate = body.push_builtin(BuiltinFunction::TextConcatenate);
                let builtin_to_debug_text = body.push_builtin(BuiltinFunction::ToDebugText);
                let builtin_type_of = body.push_builtin(BuiltinFunction::TypeOf);

                let callee = self.mapping[function];
                let callee_type = body.push_call(builtin_type_of, vec![callee], responsible);
                let tag_tag = body.push_tag("Tag".to_string(), None);
                let callee_is_tag =
                    body.push_call(builtin_equals, vec![callee_type, tag_tag], responsible);
                body.push_if_else(
                    &hir_id.child("calleeIsTag"),
                    callee_is_tag,
                    |body| {
                        let already_has_value =
                            body.push_call(builtin_tag_has_value, vec![callee], responsible);
                        body.push_if_else(
                            &hir_id.child("doesTagHaveValue"),
                            already_has_value,
                            |body| {
                                let reason = body.push_text(
                                    "You called a tag that already has a value.".to_string(),
                                );
                                body.push_panic(reason, responsible);
                            },
                            |body| {
                                if arguments.len() == 1 {
                                    body.push_call(
                                        builtin_tag_with_value,
                                        vec![callee, arguments[0]],
                                        responsible,
                                    );
                                } else {
                                    let reason = body.push_text(
                                        "Tags can only be created with one value.".to_string(),
                                    );
                                    body.push_panic(reason, responsible);
                                }
                            },
                            responsible,
                        );
                    },
                    |body| {
                        let function_tag = body.push_tag("Function".to_string(), None);
                        let callee_is_function = body.push_call(
                            builtin_equals,
                            vec![callee_type, function_tag],
                            responsible,
                        );

                        body.push_if_else(
                            &hir_id.child("calleeIsFunction"),
                            callee_is_function,
                            |body| {
                                let argument_count = body.push_call(
                                    builtin_get_argument_count,
                                    vec![callee],
                                    responsible,
                                );
                                let expected = body.push_int(arguments.len());
                                let has_correct_number_of_arguments = body.push_call(
                                    builtin_equals,
                                    vec![argument_count, expected],
                                    responsible,
                                );
                                body.push_if_else(
                                    &hir_id.child("hasCorrectNumberOfArguments"),
                                    has_correct_number_of_arguments,
                                    |body| {
                                        self.push_call(
                                            body,
                                            hir_id,
                                            self.mapping[function],
                                            arguments.clone(),
                                            responsible,
                                        );
                                    },
                                    |body| {
                                        let reason_1 = body.push_text(
                                            "You called a function that expects ".to_string(),
                                        );
                                        let reason_2 = body.push_call(
                                            builtin_to_debug_text,
                                            vec![argument_count],
                                            responsible,
                                        );
                                        let reason_3 = body.push_text(format!(
                                            " arguments with {} arguments.",
                                            arguments.len(),
                                        ));
                                        let reason_1_2 = body.push_call(
                                            builtin_text_concatenate,
                                            vec![reason_1, reason_2],
                                            responsible,
                                        );
                                        let reason = body.push_call(
                                            builtin_text_concatenate,
                                            vec![reason_1_2, reason_3],
                                            responsible,
                                        );
                                        body.push_panic(reason, responsible);
                                    },
                                    responsible,
                                );
                            },
                            |body| {
                                let reason = body
                                    .push_text("You can only call tags or functions.".to_string());
                                body.push_panic(reason, responsible);
                            },
                            responsible,
                        );
                    },
                    responsible,
                )
            }
            hir::Expression::UseModule {
                current_module,
                relative_path,
            } => body.push(Expression::UseModule {
                current_module: current_module.clone(),
                relative_path: self.mapping[relative_path],
                // The `UseModule` expression only exists in the generated `use`
                // function. If a use fails, that's also the fault of the caller.
                // Essentially, the `UseModule` expression works exactly like a
                // `needs`.
                responsible: responsible_for_needs,
            }),
            hir::Expression::Needs { condition, reason } => {
                let responsible = body.push_hir_id(hir_id.clone());
                self.push_call(
                    body,
                    hir_id,
                    self.needs_function,
                    vec![
                        self.mapping[condition],
                        self.mapping[reason],
                        responsible_for_needs,
                    ],
                    responsible,
                )
            }
            hir::Expression::Error { errors, .. } => {
                self.errors.extend(errors.clone());
                let responsible = body.push_hir_id(hir_id.clone());
                body.compile_errors(responsible, errors)
            }
        };
        self.mapping.insert(hir_id.clone(), id);

        if self.tracing.evaluated_expressions.is_enabled() {
            let hir_expression = body.push_hir_id(hir_id.clone());
            body.push(Expression::TraceExpressionEvaluated {
                hir_expression,
                value: id,
            });
            body.push_reference(id);
        }
    }

    fn compile_match(
        &mut self,
        hir_id: hir::Id,
        body: &mut BodyBuilder,
        expression: Id,
        cases: &[hir::MatchCase],
        responsible_for_needs: Id,
        responsible_for_match: Id,
    ) -> Id {
        self.compile_match_rec(
            hir_id,
            body,
            expression,
            cases,
            responsible_for_needs,
            responsible_for_match,
            0,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn compile_match_rec(
        &mut self,
        hir_id: hir::Id,
        body: &mut BodyBuilder,
        expression: Id,
        cases: &[hir::MatchCase],
        responsible_for_needs: Id,
        responsible_for_match: Id,
        case_index: usize,
    ) -> Id {
        match cases {
            [] => {
                let reason = body.push_text("No case matched the given expression.".to_string());
                // TODO: concat reasons
                body.push_panic(reason, responsible_for_match)
            }
            [hir::MatchCase {
                pattern: case_pattern,
                identifier_expressions: case_identifiers,
                condition: case_condition,
                body: case_body,
            }, rest @ ..] => {
                let pattern_result = PatternLoweringContext::check_pattern(
                    body,
                    hir_id.clone(),
                    responsible_for_match,
                    expression,
                    case_pattern,
                );

                let is_pattern_match = body.push_is_match(pattern_result, responsible_for_match);
                let case_id = hir_id.child(format!("case-{case_index}"));
                let builtin_if_else = body.push_builtin(BuiltinFunction::IfElse);

                let else_function = body.push_function(case_id.child("didNotMatch"), |body, _| {
                    self.compile_match_rec(
                        hir_id,
                        body,
                        expression,
                        rest,
                        responsible_for_needs,
                        responsible_for_match,
                        case_index + 1,
                    );
                });

                let then_function = body.push_function(case_id.child("patternMatch"), |body, _| {
                    self.ongoing_destructuring = Some(OngoingDestructuring {
                        result: pattern_result,
                        is_trivial: false,
                    });
                    self.compile_expressions(
                        body,
                        responsible_for_needs,
                        &case_identifiers.expressions,
                    );

                    self.compile_match_case_body(
                        &case_id,
                        body,
                        case_condition,
                        case_body,
                        else_function,
                        responsible_for_needs,
                        responsible_for_match,
                    );
                });

                body.push_call(
                    builtin_if_else,
                    vec![is_pattern_match, then_function, else_function],
                    responsible_for_match,
                )
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn compile_match_case_body(
        &mut self,
        case_id: &hir::Id,
        body: &mut BodyBuilder,
        case_condition: &Option<hir::Body>,
        case_body: &hir::Body,
        else_function: Id,
        responsible_for_needs: Id,
        responsible_for_match: Id,
    ) {
        let builtin_if_else = body.push_builtin(BuiltinFunction::IfElse);
        if let Some(condition) = case_condition {
            self.compile_expressions(body, responsible_for_needs, &condition.expressions);
            let condition_result = body.current_return_value();

            let is_boolean = body.push_is_bool(case_id, condition_result, responsible_for_match);
            body.push_if_not(
                &case_id.child("conditionCheck"),
                is_boolean,
                |body| {
                    let reason_parts = [
                        body.push_text("Match Condition expected boolean value, got `".to_string()),
                        body.push_to_debug_text(condition_result, responsible_for_match),
                        body.push_text("`".to_string()),
                    ];
                    let reason = body.push_text_concatenate(&reason_parts, responsible_for_match);
                    body.push_panic(reason, responsible_for_match);
                },
                responsible_for_match,
            );

            let then_function = body.push_function(case_id.child("conditionMatch"), |body, _| {
                self.compile_expressions(body, responsible_for_needs, &case_body.expressions);
            });

            body.push_call(
                builtin_if_else,
                vec![condition_result, then_function, else_function],
                responsible_for_needs,
            );
        } else {
            self.compile_expressions(body, responsible_for_needs, &case_body.expressions);
        };
    }

    fn push_call(
        &self,
        body: &mut BodyBuilder,
        hir_id: &hir::Id,
        function: Id,
        arguments: Vec<Id>,
        responsible: Id,
    ) -> Id {
        if self.tracing.calls.is_enabled() {
            let hir_call = body.push_hir_id(hir_id.clone());
            body.push(Expression::TraceCallStarts {
                hir_call,
                function,
                arguments: arguments.clone(),
                responsible,
            });
        }
        let call = body.push_call(function, arguments, responsible);
        if self.tracing.calls.is_enabled() {
            let return_value = match self.tracing.calls {
                CallTracingMode::OnlyForPanicTraces => None,
                CallTracingMode::Off | CallTracingMode::OnlyCurrent | CallTracingMode::All => {
                    Some(call)
                }
            };
            body.push(Expression::TraceCallEnds { return_value });
            body.push_reference(call)
        } else {
            call
        }
    }
}

impl hir::Pattern {
    fn is_exact(&self) -> bool {
        match self {
            Self::NewIdentifier(_) => false,
            Self::Int(_) => true,
            Self::Text(_) => true,
            Self::Tag { symbol: _, value } => value.as_ref().map_or(true, |val| val.is_exact()),
            Self::List(items) => items.iter().all(Self::is_exact),
            Self::Struct(_) => false,
            Self::Or(_) => false,
            Self::Error { .. } => true,
        }
    }
}

struct PatternLoweringContext {
    hir_id: hir::Id,
    match_tag: Id,
    no_match_tag: Id,
    responsible: Id,
}
impl PatternLoweringContext {
    /// Checks a pattern and returns an expression of type
    /// `Match (variable0, …, variableN) | NoMatch`.
    fn check_pattern(
        body: &mut BodyBuilder,
        hir_id: hir::Id,
        responsible: Id,
        expression: Id,
        pattern: &hir::Pattern,
    ) -> Id {
        let match_tag = body.push_match_tag();
        let no_match_tag = body.push_no_match_tag();
        let context = Self {
            hir_id,
            match_tag,
            no_match_tag,
            responsible,
        };
        context.check(body, expression, pattern)
    }

    fn check(&self, body: &mut BodyBuilder, expression: Id, pattern: &hir::Pattern) -> Id {
        if pattern.is_exact() {
            let exact_value = self.compile_pattern(body, pattern);
            return self.check_equals_exact_value(body, expression, exact_value);
        }
        match pattern {
            hir::Pattern::NewIdentifier(_) => self.push_match(body, vec![expression]),
            // The unreachable cases will be caught be the pattern.is_exact()
            // check above.
            hir::Pattern::Int(_) => unreachable!(),
            hir::Pattern::Text(_) => unreachable!(),
            hir::Pattern::Tag {
                symbol: _,
                value: None,
            } => unreachable!(),
            hir::Pattern::Tag {
                symbol,
                value: Some(value),
            } => self.check_type(body, expression, "Tag".to_string(), |body| {
                let builtin_tag_without_value = body.push_builtin(BuiltinFunction::TagWithoutValue);
                let actual_symbol = body.push_call(
                    builtin_tag_without_value,
                    vec![expression],
                    self.responsible,
                );
                let expected_symbol = body.push_tag(symbol.clone(), None);
                self.check_equals(body, expected_symbol, actual_symbol, |body| {
                    let builtin_tag_has_value = body.push_builtin(BuiltinFunction::TagHasValue);
                    let actual_has_value =
                        body.push_call(builtin_tag_has_value, vec![expression], self.responsible);
                    let expected_has_value = body.push_bool(true);
                    self.check_equals(body, expected_has_value, actual_has_value, |body| {
                        let builtin_tag_get_value = body.push_builtin(BuiltinFunction::TagGetValue);
                        let actual_value = body.push_call(
                            builtin_tag_get_value,
                            vec![expression],
                            self.responsible,
                        );
                        self.check(body, actual_value, value);
                    });
                });
            }),
            hir::Pattern::List(list) => {
                // Check that it's a list.
                self.check_type(body, expression, "List".to_string(), |body| {
                    // Check that the length is correct.
                    let expected = body.push_int(list.len());
                    let builtin_list_length = body.push_builtin(BuiltinFunction::ListLength);
                    let actual_length =
                        body.push_call(builtin_list_length, vec![expression], self.responsible);
                    self.check_equals(body, expected, actual_length, |body| {
                        // Destructure the items.
                        let builtin_list_get = body.push_builtin(BuiltinFunction::ListGet);
                        let condition_builders = list
                            .iter()
                            .enumerate()
                            .map(|(index, item_pattern)| {
                                move |body: &mut BodyBuilder| {
                                    let index = body.push_int(index);
                                    let item = body.push_call(
                                        builtin_list_get,
                                        vec![expression, index],
                                        self.responsible,
                                    );
                                    let result = self.check(body, item, item_pattern);
                                    (result, item_pattern.captured_identifier_count())
                                }
                            })
                            .collect_vec();
                        self.check_all(body, condition_builders);
                    });
                })
            }
            hir::Pattern::Struct(struct_) => {
                // Check that it's a struct.
                self.check_type(body, expression, "Struct".to_string(), |body| {
                    // Destructure the entries.
                    let builtin_struct_has_key = body.push_builtin(BuiltinFunction::StructHasKey);
                    let builtin_struct_get = body.push_builtin(BuiltinFunction::StructGet);
                    let condition_builders = struct_
                        .iter()
                        .map(|(key_pattern, value_pattern)| {
                            |body: &mut BodyBuilder| {
                                let key = self.compile_pattern(body, key_pattern);
                                let has_key = body.push_call(
                                    builtin_struct_has_key,
                                    vec![expression, key],
                                    self.responsible,
                                );

                                let result = body.push_if_else(
                                    &self.hir_id.child("hasKey"),
                                    has_key,
                                    |body| {
                                        let value = body.push_call(
                                            builtin_struct_get,
                                            vec![expression, key],
                                            self.responsible,
                                        );
                                        self.check(body, value, value_pattern);
                                    },
                                    |body| {
                                        self.push_no_match(body);
                                    },
                                    self.responsible,
                                );
                                (result, value_pattern.captured_identifier_count())
                            }
                        })
                        .collect_vec();
                    self.check_all(body, condition_builders);
                })
            }
            hir::Pattern::Or(patterns) => {
                let [first_pattern, rest_patterns @ ..] = patterns.as_slice() else {
                    panic!("Or pattern must contain at least two patterns.");
                };

                let mut result = self.check(body, expression, first_pattern);

                let captured_identifiers_order = first_pattern.captured_identifiers();
                let list_get_function = body.push_builtin(BuiltinFunction::ListGet);
                let nothing = body.push_nothing();

                for pattern in rest_patterns {
                    let is_match = body.push_is_match(result, self.responsible);
                    result = body.push_if_else(
                        &self.hir_id.child("isMatch"),
                        is_match,
                        |body| {
                            let captured_identifiers = pattern.captured_identifiers();
                            if captured_identifiers == captured_identifiers_order {
                                body.push_reference(result);
                                return;
                            }

                            let captured_identifiers = captured_identifiers_order
                                .iter()
                                .map(|identifier_id| {
                                    let index = captured_identifiers
                                        .iter()
                                        .position(|it| it == identifier_id);
                                    let Some(index) = index else {
                                        return body.push_reference(nothing);
                                    };

                                    let index = body.push_int(1 + index);
                                    body.push_call(
                                        list_get_function,
                                        vec![result, index],
                                        self.responsible,
                                    )
                                })
                                .collect();
                            self.push_match(body, captured_identifiers);
                        },
                        |body| {
                            self.check(body, expression, pattern);
                        },
                        self.responsible,
                    );
                }
                result
            }
            hir::Pattern::Error { errors } => body.compile_errors(self.responsible, errors),
        }
    }

    fn check_equals_exact_value(
        &self,
        body: &mut BodyBuilder,
        expression: Id,
        exact_value: Id,
    ) -> Id {
        self.check_equals(body, exact_value, expression, |body| {
            self.push_match(body, vec![]);
        })
    }

    fn check_type(
        &self,
        body: &mut BodyBuilder,
        expression: Id,
        expected_type: String,
        then_builder: impl FnOnce(&mut BodyBuilder),
    ) -> Id {
        let expected_type = body.push_tag(expected_type, None);
        let builtin_type_of = body.push_builtin(BuiltinFunction::TypeOf);
        let type_ = body.push_call(builtin_type_of, vec![expression], self.responsible);
        self.check_equals(body, expected_type, type_, then_builder)
    }

    fn check_equals(
        &self,
        body: &mut BodyBuilder,
        expected: Id,
        actual: Id,
        then_builder: impl FnOnce(&mut BodyBuilder),
    ) -> Id {
        let builtin_equals = body.push_builtin(BuiltinFunction::Equals);
        let equals = body.push_call(builtin_equals, vec![expected, actual], self.responsible);

        body.push_if_else(
            &self.hir_id.child("equals"),
            equals,
            then_builder,
            |body| {
                self.push_no_match(body);
            },
            self.responsible,
        )
    }

    fn compile_pattern(&self, body: &mut BodyBuilder, pattern: &hir::Pattern) -> Id {
        assert!(pattern.is_exact());
        match pattern {
            hir::Pattern::NewIdentifier(_) => unreachable!(),
            hir::Pattern::Int(int) => body.push_int(int.clone()),
            hir::Pattern::Text(text) => body.push_text(text.clone()),
            hir::Pattern::Tag { symbol, value } => {
                let value = value
                    .as_ref()
                    .map(|value| self.compile_pattern(body, value));
                body.push_tag(symbol.to_string(), value)
            }
            hir::Pattern::List(items) => {
                let items = items
                    .iter()
                    .map(|item| self.compile_pattern(body, item))
                    .collect();
                body.push_list(items)
            }
            hir::Pattern::Struct(_) => {
                panic!("Structs can't be used in this part of a pattern.")
            }
            hir::Pattern::Or(_) => unreachable!(),
            hir::Pattern::Error { errors } => body.compile_errors(self.responsible, errors),
        }
    }

    fn check_all(
        &self,
        body: &mut BodyBuilder,
        condition_builders: Vec<impl FnMut(&mut BodyBuilder) -> (Id, usize)>,
    ) -> Id {
        self.check_all_rec(body, condition_builders, vec![])
    }
    fn check_all_rec(
        &self,
        body: &mut BodyBuilder,
        mut condition_builders: Vec<impl FnMut(&mut BodyBuilder) -> (Id, usize)>,
        mut captured_identifiers: Vec<Id>,
    ) -> Id {
        if condition_builders.is_empty() {
            return self.push_match(body, captured_identifiers);
        };

        let mut condition_builder = condition_builders.remove(0);
        let (return_value, captured_identifier_count) = condition_builder(body);

        let is_match = body.push_is_match(return_value, self.responsible);
        body.push_if_else(
            &self.hir_id.child("isMatch"),
            is_match,
            |body| {
                let tag_get_value = body.push_builtin(BuiltinFunction::TagGetValue);
                let captured = body.push_call(tag_get_value, vec![return_value], self.responsible);
                let list_get = body.push_builtin(BuiltinFunction::ListGet);
                for index in 0..captured_identifier_count {
                    let index = body.push_int(index);
                    let captured_identifier =
                        body.push_call(list_get, vec![captured, index], self.responsible);
                    captured_identifiers.push(captured_identifier);
                }
                self.check_all_rec(body, condition_builders, captured_identifiers);
            },
            |body| {
                body.push_reference(return_value);
            },
            self.responsible,
        )
    }

    fn push_match(&self, body: &mut BodyBuilder, captured_identifiers: Vec<Id>) -> Id {
        let captured = body.push_list(captured_identifiers);
        body.push_call(self.match_tag, vec![captured], self.responsible)
    }
    fn push_no_match(&self, body: &mut BodyBuilder) -> Id {
        body.push_reference(self.no_match_tag)
    }
}

impl BodyBuilder {
    fn push_match_tag(&mut self) -> Id {
        self.push_tag("Match".to_string(), None)
    }
    fn push_no_match_tag(&mut self) -> Id {
        self.push_tag("NoMatch".to_string(), None)
    }

    /// Compiles to code taking a `Match (…,)` or `NoMatch` and returning a
    /// boolean.
    fn push_is_match(&mut self, match_or_no_match: Id, responsible: Id) -> Id {
        let tag_without_value = self.push_builtin(BuiltinFunction::TagWithoutValue);
        let match_or_no_match_symbol =
            self.push_call(tag_without_value, vec![match_or_no_match], responsible);

        let equals_function = self.push_builtin(BuiltinFunction::Equals);
        let match_tag = self.push_match_tag();
        self.push_call(
            equals_function,
            vec![match_or_no_match_symbol, match_tag],
            responsible,
        )
    }

    fn compile_errors(&mut self, responsible: Id, errors: &[CompilerError]) -> Id {
        let reason = errors
            .iter()
            .map(|error| format!("{}", error.payload))
            .join("\n");
        let reason = self.push_text(reason);
        self.push_panic(reason, responsible)
    }
}
