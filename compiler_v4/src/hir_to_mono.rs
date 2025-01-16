use crate::{
    ast_to_hir::TypeUnifier,
    hir::{self, BuiltinFunction, Hir, NamedType, ParameterType, Type},
    id::IdGenerator,
    mono::{self, Mono},
    type_solver::{goals::SolverSolution, values::SolverVariable},
    utils::HashMapExtension,
};
use itertools::Itertools;
use rustc_hash::FxHashMap;
use std::{borrow::Cow, collections::hash_map::Entry, mem};

pub fn hir_to_mono(hir: &Hir) -> Mono {
    Context::lower(hir)
}

struct Context<'h> {
    hir: &'h Hir,
    type_declarations: FxHashMap<Box<str>, Option<mono::TypeDeclaration>>,
    assignments: FxHashMap<Box<str>, Option<mono::Assignment>>,
    assignment_initialization_order: Vec<Box<str>>,
    functions: FxHashMap<Box<str>, Option<mono::Function>>,
}
impl<'h> Context<'h> {
    #[must_use]
    fn lower(hir: &'h Hir) -> Mono {
        let mut context = Self {
            hir,
            type_declarations: FxHashMap::default(),
            assignments: FxHashMap::default(),
            assignment_initialization_order: vec![],
            functions: FxHashMap::default(),
        };
        let main_function = context.lower_function(hir.main_function_id, &FxHashMap::default());
        context.lower_function(BuiltinFunction::Panic.id(), &FxHashMap::default());
        Mono {
            type_declarations: context
                .type_declarations
                .into_iter()
                .map(|(name, declaration)| (name, declaration.unwrap()))
                .collect(),
            assignments: context
                .assignments
                .into_iter()
                .map(|(name, assignment)| (name, assignment.unwrap()))
                .collect(),
            assignment_initialization_order: context
                .assignment_initialization_order
                .into_boxed_slice(),
            functions: context
                .functions
                .into_iter()
                .map(|(name, function)| (name, function.unwrap()))
                .collect(),
            main_function,
        }
    }

    fn lower_assignment(&mut self, id: hir::Id) -> Box<str> {
        let assignment = &self
            .hir
            .assignments
            .get(&id)
            .unwrap_or_else(|| panic!("Unknown assignment: {id}"));
        let name = assignment.name.to_string().into_boxed_str();
        match self.assignments.entry(name.clone()) {
            Entry::Occupied(_) => return name.clone(),
            Entry::Vacant(entry) => entry.insert(None),
        };

        let type_ = self.lower_type(&assignment.type_);
        let (_, body) = BodyBuilder::build(self, &FxHashMap::default(), |builder| {
            builder.lower_expressions(&assignment.body.expressions);
        });
        let assignment = mono::Assignment { type_, body };
        *self.assignments.get_mut(&name).unwrap() = Some(assignment);
        self.assignment_initialization_order.push(name.clone());
        name.clone()
    }

    fn lower_function(
        &mut self,
        id: hir::Id,
        substitutions: &FxHashMap<ParameterType, Type>,
    ) -> Box<str> {
        let mut substitutions = Cow::Borrowed(substitutions);
        let function = self.hir.functions.get(&id).unwrap_or_else(|| {
            let impl_ = self.find_impl_for(id, &substitutions);
            let function = &impl_.functions[&id];

            let mut unifier = TypeUnifier::new(&impl_.type_parameters);
            assert!(unifier
                .unify(&substitutions[&ParameterType::self_type()], &impl_.type_)
                .unwrap());
            let substitutions = substitutions.to_mut();
            for (parameter_type, type_) in unifier.finish().unwrap() {
                substitutions.force_insert(parameter_type, type_);
            }

            function
        });

        let name = self.mangle_function(
            &function.signature.name,
            &function
                .signature
                .type_parameters
                .iter()
                .map(|it| hir::Type::from(it.type_()).substitute(&substitutions))
                .collect_vec(),
            &function
                .signature
                .parameters
                .iter()
                .map(|it| it.type_.substitute(&substitutions))
                .collect_vec(),
        );
        match self.functions.entry(name.clone()) {
            Entry::Occupied(_) => return name,
            Entry::Vacant(entry) => entry.insert(None),
        };

        let (parameters, body) = match &function.body {
            hir::BodyOrBuiltin::Body(body) => {
                let (parameters, body) = BodyBuilder::build(self, &substitutions, |builder| {
                    builder.add_parameters(&function.signature.parameters);
                    builder.lower_expressions(&body.expressions);
                });
                (parameters, mono::BodyOrBuiltin::Body(body))
            }
            hir::BodyOrBuiltin::Builtin(builtin_function) => {
                let (parameters, _) = BodyBuilder::build(self, &substitutions, |builder| {
                    builder.add_parameters(&function.signature.parameters);
                });
                (
                    parameters,
                    mono::BodyOrBuiltin::Builtin {
                        builtin_function: *builtin_function,
                        substitutions: substitutions
                            .iter()
                            .map(|(parameter_type, type_)| {
                                (parameter_type.name.clone(), self.lower_type(type_))
                            })
                            .collect(),
                    },
                )
            }
        };
        let return_type = function.signature.return_type.substitute(&substitutions);
        let function = mono::Function {
            parameters,
            return_type: self.lower_type(&return_type),
            body,
        };
        *self.functions.get_mut(&name).unwrap() = Some(function);
        name
    }
    fn find_impl_for(
        &self,
        function_id: hir::Id,
        substitutions: &FxHashMap<hir::ParameterType, hir::Type>,
    ) -> &'h hir::Impl {
        let (trait_, function) = self
            .hir
            .traits
            .iter()
            .find_map(|(trait_, trait_definition)| {
                trait_definition
                    .functions
                    .get(&function_id)
                    .map(|function| (trait_, function))
            })
            .unwrap_or_else(|| panic!("Unknown trait function: {function_id}"));
        let self_type = function.signature.parameters[0]
            .type_
            .substitute(substitutions);
        self.hir
            .impls
            .iter()
            .find(|impl_| {
                if &impl_.trait_.name != trait_ {
                    return false;
                }

                let mut unifier = TypeUnifier::new(&impl_.type_parameters);
                if unifier.unify(&self_type, &impl_.type_) != Ok(true) {
                    return false;
                }

                let trait_declaration = &self.hir.traits[trait_];

                let self_goal = substitutions
                    .get(&ParameterType::self_type())
                    .map(|self_type| {
                        trait_declaration
                            .solver_goal
                            .substitute_all(&FxHashMap::from_iter([(
                                SolverVariable::self_(),
                                self_type.clone().try_into().unwrap(),
                            )]))
                    });

                let solver_substitutions = substitutions
                    .iter()
                    .filter_map(|(parameter_type, type_)| try {
                        (
                            SolverVariable::new(parameter_type.clone()),
                            type_.clone().try_into().ok()?,
                        )
                    })
                    .collect();

                self_goal
                    .iter()
                    .chain(trait_declaration.solver_subgoals.iter())
                    .map(|subgoal| {
                        let solution = self
                            .hir
                            .solver_environment
                            .solve(&subgoal.substitute_all(&solver_substitutions), &[]);
                        match solution {
                            SolverSolution::Unique(solution) => Some(solution.used_rule),
                            SolverSolution::Ambiguous => panic!(
                                "Ambiguous solver solution for {}",
                                subgoal.substitute_all(&solver_substitutions),
                            ),
                            SolverSolution::Impossible => None,
                        }
                    })
                    .collect::<Option<Vec<_>>>()
                    .is_some()
            })
            .unwrap_or_else(|| {
                panic!("No matching impl found for trait `{trait_}` with {substitutions:?}")
            })
    }
    fn mangle_function(
        &mut self,
        name: &str,
        type_parameters: &[hir::Type],
        parameter_types: &[hir::Type],
    ) -> Box<str> {
        let mut result = if name == "main" && parameter_types.is_empty() {
            // Avoid name clash with the main function in C.
            // It would be cleaner to do this in `mono_to_c`, but it's easier to do it here.
            "main$"
        } else {
            name
        }
        .to_string();
        for type_parameter in type_parameters {
            result.push('$');
            result.push_str(&self.lower_type(type_parameter));
        }
        result.push('$');
        for parameter_type in parameter_types {
            result.push('$');
            result.push_str(&self.lower_type(parameter_type));
        }
        result.into_boxed_str()
    }

    fn lower_type(&mut self, type_: &hir::Type) -> Box<str> {
        let mangled_name = Self::mangle_type(type_);
        let entry = match self.type_declarations.entry(mangled_name.clone()) {
            Entry::Occupied(_) => return mangled_name,
            Entry::Vacant(entry) => entry,
        };

        let declaration = match type_ {
            hir::Type::Named(NamedType {
                name,
                type_arguments,
            }) => {
                let Some(declaration) = &self.hir.type_declarations.get(name) else {
                    // Builtin type
                    return mangled_name;
                };
                match &declaration.kind {
                    hir::TypeDeclarationKind::Struct { fields } => {
                        entry.insert(None);
                        let environment = hir::Type::build_environment(
                            &declaration.type_parameters,
                            type_arguments,
                        );
                        if let Some(fields) = fields.as_ref() {
                            let fields = fields
                                .iter()
                                .map(|field| {
                                    (
                                        field.name.clone(),
                                        self.lower_type(&field.type_.substitute(&environment)),
                                    )
                                })
                                .collect();
                            mono::TypeDeclaration::Struct { fields }
                        } else {
                            mono::TypeDeclaration::Builtin {
                                name: name.clone(),
                                type_arguments: type_arguments
                                    .iter()
                                    .map(|it| self.lower_type(it))
                                    .collect(),
                            }
                        }
                    }
                    hir::TypeDeclarationKind::Enum { variants } => {
                        entry.insert(None);
                        let environment = hir::Type::build_environment(
                            &declaration.type_parameters,
                            type_arguments,
                        );
                        let variants = variants
                            .iter()
                            .map(|(name, value_type)| mono::EnumVariant {
                                name: name.clone(),
                                value_type: value_type
                                    .as_ref()
                                    .map(|it| self.lower_type(&it.substitute(&environment))),
                            })
                            .collect();
                        mono::TypeDeclaration::Enum { variants }
                    }
                }
            }
            hir::Type::Parameter(parameter_type) => {
                panic!("Type parameter `{parameter_type}` should have been monomorphized.")
            }
            hir::Type::Self_ { base_type } => {
                panic!("Self type (base type: {base_type}) should have been monomorphized.")
            }
            hir::Type::Function(hir::FunctionType {
                parameter_types,
                return_type,
            }) => {
                entry.insert(None);
                mono::TypeDeclaration::Function {
                    parameter_types: parameter_types
                        .iter()
                        .map(|it| self.lower_type(it))
                        .collect(),
                    return_type: self.lower_type(return_type),
                }
            }
            hir::Type::Error => unreachable!(),
        };
        *self.type_declarations.get_mut(&mangled_name).unwrap() = Some(declaration);
        mangled_name
    }
    fn mangle_type(type_: &hir::Type) -> Box<str> {
        let mut result = String::new();
        Self::mangle_type_helper(&mut result, type_);
        result.into_boxed_str()
    }
    fn mangle_type_helper(result: &mut String, type_: &hir::Type) {
        match type_ {
            hir::Type::Named(type_) => {
                result.push_str(&type_.name);
                if !type_.type_arguments.is_empty() {
                    result.push_str("$of$");
                    for type_ in &type_.type_arguments {
                        Self::mangle_type_helper(result, type_);
                        result.push('$');
                    }
                    result.push_str("end$");
                }
            }
            hir::Type::Parameter(parameter_type) => {
                panic!("Type parameter `{parameter_type}` should have been monomorphized.")
            }
            hir::Type::Self_ { base_type } => {
                panic!("Self type (base type: {base_type}) should have been monomorphized.")
            }
            hir::Type::Function(type_) => {
                result.push_str("$Fun$");
                if !type_.parameter_types.is_empty() {
                    result.push_str("of$");
                    for type_ in &type_.parameter_types {
                        Self::mangle_type_helper(result, type_);
                        result.push('$');
                    }
                    result.push_str("end$");
                }
                result.push_str("returns$");
                Self::mangle_type_helper(result, &type_.return_type);
            }
            hir::Type::Error => result.push_str("Never"),
        }
    }
}
struct BodyBuilder<'c, 'h> {
    context: &'c mut Context<'h>,
    environment: &'c FxHashMap<hir::ParameterType, hir::Type>,
    parameters: Vec<mono::Parameter>,
    body: mono::Body,
    id_generator: IdGenerator<mono::Id>,
    id_mapping: FxHashMap<hir::Id, mono::Id>,
}
impl<'c, 'h> BodyBuilder<'c, 'h> {
    #[must_use]
    fn build(
        context: &'c mut Context<'h>,
        environment: &'c FxHashMap<hir::ParameterType, hir::Type>,
        fun: impl FnOnce(&mut BodyBuilder),
    ) -> (Box<[mono::Parameter]>, mono::Body) {
        let mut builder = Self {
            context,
            environment,
            parameters: vec![],
            body: mono::Body::default(),
            id_generator: IdGenerator::default(),
            id_mapping: FxHashMap::default(),
        };
        fun(&mut builder);
        (builder.parameters.into_boxed_slice(), builder.body)
    }
    #[must_use]
    fn build_inner(
        &mut self,
        fun: impl FnOnce(&mut BodyBuilder),
    ) -> (Box<[mono::Parameter]>, mono::Body) {
        let mut builder = BodyBuilder {
            context: self.context,
            environment: self.environment,
            parameters: vec![],
            body: mono::Body::default(),
            id_generator: IdGenerator::default(),
            id_mapping: FxHashMap::default(),
        };
        builder.id_mapping.clone_from(&self.id_mapping);
        builder.id_generator = mem::take(&mut self.id_generator);

        fun(&mut builder);

        self.id_generator = builder.id_generator;
        (builder.parameters.into_boxed_slice(), builder.body)
    }

    fn add_parameters(&mut self, parameters: &[hir::Parameter]) {
        for parameter in parameters {
            self.add_parameter(parameter);
        }
    }
    fn add_parameter(&mut self, parameter: &hir::Parameter) {
        let id = self.id_generator.generate();
        self.id_mapping.force_insert(parameter.id, id);
        let type_ = self.lower_type(&parameter.type_);
        self.parameters.push(mono::Parameter {
            id,
            name: parameter.name.clone(),
            type_,
        });
    }

    fn lower_type(&mut self, type_: &hir::Type) -> Box<str> {
        self.context.lower_type(&type_.substitute(self.environment))
    }

    fn lower_expressions(&mut self, expressions: &[(hir::Id, Option<Box<str>>, hir::Expression)]) {
        for (id, name, expression) in expressions {
            self.lower_expression(*id, name.clone(), expression);
        }
    }
    fn lower_expression(
        &mut self,
        id: hir::Id,
        name: Option<Box<str>>,
        expression: &hir::Expression,
    ) {
        match &expression.kind {
            hir::ExpressionKind::Int(int) => {
                self.push(id, name, mono::ExpressionKind::Int(*int), &expression.type_);
            }
            hir::ExpressionKind::Text(text) => {
                self.push(
                    id,
                    name,
                    mono::ExpressionKind::Text(text.clone()),
                    &expression.type_,
                );
            }
            hir::ExpressionKind::CreateStruct { struct_, fields } => {
                let struct_ = self.lower_type(&struct_.clone().into());
                let fields = self.lower_ids(fields);
                self.push(
                    id,
                    name,
                    mono::ExpressionKind::CreateStruct { struct_, fields },
                    &expression.type_,
                );
            }
            hir::ExpressionKind::StructAccess { struct_, field } => {
                let struct_ = self.lower_id(*struct_);
                self.push(
                    id,
                    name,
                    mono::ExpressionKind::StructAccess {
                        struct_,
                        field: field.clone(),
                    },
                    &expression.type_,
                );
            }
            hir::ExpressionKind::CreateEnum {
                enum_,
                variant,
                value,
            } => {
                let enum_ = self.lower_type(&enum_.clone().into());
                let value = value.map(|it| self.lower_id(it));
                self.push(
                    id,
                    name,
                    mono::ExpressionKind::CreateEnum {
                        enum_,
                        variant: variant.clone(),
                        value,
                    },
                    &expression.type_,
                );
            }
            hir::ExpressionKind::Reference(referenced_id) => {
                let kind = if let Some(referenced_id) = self.id_mapping.get(referenced_id) {
                    mono::ExpressionKind::LocalReference(*referenced_id)
                } else {
                    mono::ExpressionKind::GlobalAssignmentReference(
                        self.context.lower_assignment(*referenced_id),
                    )
                };
                self.push(id, name, kind, &expression.type_);
            }
            hir::ExpressionKind::Call {
                function,
                substitutions,
                arguments,
            } => {
                let arguments = self.lower_ids(arguments);
                let expression_kind = if let Some(id) = self.id_mapping.get(function) {
                    let lambda = *self.id_mapping.get(function).unwrap_or_else(|| {
                        panic!("Unknown function: {function} (referenced from {id} ({name:?}))")
                    });
                    mono::ExpressionKind::CallLambda { lambda, arguments }
                } else {
                    let function = self
                        .context
                        .lower_function(*function, &self.merge_substitutions(substitutions));
                    mono::ExpressionKind::CallFunction {
                        function,
                        arguments,
                    }
                };
                self.push(id, name, expression_kind, &expression.type_);
            }
            hir::ExpressionKind::Switch {
                value,
                enum_,
                cases,
            } => {
                let value = self.lower_id(*value);
                let enum_ = self.lower_type(enum_);

                let mono::TypeDeclaration::Enum { variants } =
                    &self.context.type_declarations[&enum_].as_ref().unwrap()
                else {
                    unreachable!();
                };
                let variants = variants.clone();

                let cases = cases
                    .iter()
                    .map(|case| {
                        let value_ids = case
                            .value_id
                            .map(|hir_id| (hir_id, self.id_generator.generate()));
                        mono::SwitchCase {
                            variant: case.variant.clone(),
                            value: value_ids.map(|(_, mir_id)| {
                                (
                                    mir_id,
                                    variants
                                        .iter()
                                        .find(|it| it.name == case.variant)
                                        .unwrap()
                                        .value_type
                                        .as_ref()
                                        .unwrap()
                                        .clone(),
                                )
                            }),
                            body: self
                                .build_inner(|builder| {
                                    if let Some((hir_id, mir_id)) = value_ids {
                                        builder.id_mapping.force_insert(hir_id, mir_id);
                                    }
                                    builder.lower_expressions(&case.body.expressions);
                                })
                                .1,
                        }
                    })
                    .collect();
                self.push(
                    id,
                    name,
                    mono::ExpressionKind::Switch {
                        value,
                        enum_,
                        cases,
                    },
                    &expression.type_,
                );
            }
            hir::ExpressionKind::Lambda { parameters, body } => {
                let (parameters, body) = self.build_inner(|builder| {
                    builder.add_parameters(parameters);
                    builder.lower_expressions(&body.expressions);
                });
                self.push(
                    id,
                    name,
                    mono::ExpressionKind::Lambda(mono::Lambda { parameters, body }),
                    &expression.type_,
                );
            }
            hir::ExpressionKind::Error => todo!(),
        }
    }
    fn lower_ids(&mut self, hir_ids: &[hir::Id]) -> Box<[mono::Id]> {
        hir_ids.iter().map(|id| self.lower_id(*id)).collect()
    }
    fn lower_id(&mut self, hir_id: hir::Id) -> mono::Id {
        self.id_mapping.get(&hir_id).copied().unwrap_or_else(|| {
            let name = self.context.lower_assignment(hir_id);
            let assignment = &self.context.hir.assignments[&hir_id];
            self.push(
                None,
                None,
                mono::ExpressionKind::GlobalAssignmentReference(name),
                &assignment.type_,
            )
        })
    }

    fn merge_substitutions(
        &self,
        inner: &FxHashMap<ParameterType, Type>,
    ) -> FxHashMap<ParameterType, Type> {
        inner
            .iter()
            .map(|(key, value)| (key.clone(), self.merge_substitution(value)))
            .collect()
    }
    fn merge_substitution(&self, type_: &hir::Type) -> Type {
        match type_ {
            hir::Type::Named(NamedType {
                name,
                type_arguments,
            }) => hir::Type::Named(NamedType {
                name: name.clone(),
                type_arguments: type_arguments
                    .iter()
                    .map(|it| self.merge_substitution(it))
                    .collect(),
            }),
            hir::Type::Parameter(parameter_type) => self.environment[parameter_type].clone(),
            hir::Type::Function(function_type) => hir::Type::Function(hir::FunctionType {
                parameter_types: function_type
                    .parameter_types
                    .iter()
                    .map(|it| self.merge_substitution(it))
                    .collect(),
                return_type: Box::new(self.merge_substitution(&function_type.return_type)),
            }),
            hir::Type::Self_ { .. } | hir::Type::Error => unreachable!(),
        }
    }

    fn push(
        &mut self,
        hir_id: impl Into<Option<hir::Id>>,
        name: impl Into<Option<Box<str>>>,
        kind: mono::ExpressionKind,
        type_: &hir::Type,
    ) -> mono::Id {
        let id = self.id_generator.generate();
        if let Some(hir_id) = hir_id.into() {
            self.id_mapping.force_insert(hir_id, id);
        }
        let type_ = self.lower_type(type_);
        self.body
            .expressions
            .push((id, name.into(), mono::Expression { kind, type_ }));
        id
    }
}
