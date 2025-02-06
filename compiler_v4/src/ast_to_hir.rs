use crate::{
    ast::{
        Ast, AstArguments, AstAssignment, AstBody, AstCall, AstDeclaration, AstEnum, AstExpression,
        AstExpressionKind, AstFunction, AstImpl, AstLambda, AstParameters, AstResult, AstStatement,
        AstString, AstStruct, AstStructKind, AstSwitch, AstTextPart, AstTrait, AstType,
        AstTypeParameter, AstTypeParameters,
    },
    error::CompilerError,
    hir::{
        self, Assignment, Body, BodyOrBuiltin, BuiltinFunction, ContainsError, Expression,
        ExpressionKind, Function, FunctionSignature, FunctionType, Hir, Id, Impl, NamedType,
        Parameter, ParameterType, SliceOfTypeParameter, StructField, SwitchCase, Trait,
        TraitDefinition, TraitFunction, Type, TypeDeclaration, TypeDeclarationKind, TypeParameter,
    },
    id::IdGenerator,
    position::Offset,
    type_solver::{
        goals::{Environment, SolverGoal, SolverRule, SolverSolution},
        values::{SolverType, SolverVariable},
    },
    utils::HashMapExtension,
};
use itertools::{Itertools, Position};
use petgraph::{
    algo::toposort,
    graph::{DiGraph, NodeIndex},
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    collections::hash_map::Entry,
    fmt::{self, Display, Formatter},
    iter, mem,
    ops::Range,
    path::Path,
};
use strum::VariantArray;

pub fn ast_to_hir(path: &Path, ast: &Ast) -> (Hir, Vec<CompilerError>) {
    let mut context = Context::new(path, ast);
    context.add_builtin_functions();
    context.lower_declarations();
    context.into_hir()
}

#[derive(Debug)]
struct Context<'a> {
    path: &'a Path,
    ast: &'a Ast,
    id_generator: IdGenerator<Id>,
    traits: FxHashMap<Box<str>, TraitDeclaration<'a>>,
    impls: Vec<ImplDeclaration<'a>>,
    environment: Environment,
    global_identifiers: FxHashMap<Box<str>, Named>,
    assignments: FxHashMap<Id, AssignmentDeclaration<'a>>,
    assignment_dependency_graph: DiGraph<Id, ()>,
    functions: FxHashMap<Id, FunctionDeclaration<'a>>,
    errors: Vec<CompilerError>,
    hir: Hir,
}
#[derive(Clone, Debug)]
struct TraitDeclaration<'a> {
    type_parameters: Box<[TypeParameter]>,
    solver_goal: SolverGoal,
    solver_subgoals: Box<[SolverGoal]>,
    functions: FxHashMap<Id, FunctionDeclaration<'a>>,
}
impl<'a> TraitDeclaration<'a> {
    fn find_parent_function_of(
        &self,
        substitutions: &FxHashMap<ParameterType, Type>,
        impl_function: &FunctionDeclaration<'a>,
    ) -> Option<Id> {
        self.functions
            .iter()
            .find(|(_, trait_function)| {
                impl_function.name == trait_function.name
                    && impl_function.signature.type_parameters.len()
                        == trait_function.signature.type_parameters.len()
                    && impl_function
                        .signature
                        .type_parameters
                        .iter()
                        .zip(trait_function.signature.type_parameters.iter())
                        .all(|(impl_type_parameter, trait_type_parameter)| {
                            impl_type_parameter.name == trait_type_parameter.name
                                && impl_type_parameter.upper_bound
                                    == trait_type_parameter.upper_bound.as_ref().map(|it| {
                                        it.as_ref().map(|it| it.substitute(substitutions))
                                    })
                        })
                    && impl_function.signature.parameters.len()
                        == trait_function.signature.parameters.len()
                    && impl_function
                        .signature
                        .parameters
                        .iter()
                        .zip(trait_function.signature.parameters.iter())
                        .all(|(impl_parameter, trait_parameter)| {
                            impl_parameter.name == trait_parameter.name
                                && impl_parameter.type_
                                    == trait_parameter.type_.substitute(substitutions)
                        })
                    && impl_function.signature.return_type
                        == trait_function
                            .signature
                            .return_type
                            .substitute(substitutions)
            })
            .map(|(id, _)| *id)
    }

    #[must_use]
    fn into_definition(self) -> TraitDefinition {
        TraitDefinition {
            type_parameters: self.type_parameters,
            solver_goal: self.solver_goal,
            solver_subgoals: self.solver_subgoals,
            functions: self
                .functions
                .into_iter()
                .map(|(id, function)| (id, function.into_trait_function()))
                .collect(),
        }
    }
}
#[derive(Clone, Debug)]
struct ImplDeclaration<'a> {
    type_parameters: Box<[TypeParameter]>,
    type_: Type,
    trait_: Trait,
    functions: FxHashMap<Id, FunctionDeclaration<'a>>,
}
impl ImplDeclaration<'_> {
    #[must_use]
    fn into_impl(self) -> Impl {
        Impl {
            type_parameters: self.type_parameters,
            type_: self.type_,
            trait_: self.trait_,
            functions: self
                .functions
                .into_iter()
                .map(|(id, function)| (id, function.into_function()))
                .collect(),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Named {
    Assignment(Id),
    Functions(Vec<Id>),
}
#[derive(Debug)]
struct AssignmentDeclaration<'a> {
    ast: &'a AstAssignment,
    type_: Type,
    graph_index: NodeIndex,
    body: Option<Body>,
}
#[derive(Clone, Debug)]
struct FunctionDeclaration<'a> {
    ast: Option<&'a AstFunction>,
    name: Box<str>,
    name_span: Option<Range<Offset>>,
    signature: Signature,
    body: Option<BodyOrBuiltin>,
}
impl FunctionDeclaration<'_> {
    fn signature_to_string(&self) -> String {
        format!("{}{}", self.name, self.signature)
    }

    fn call_signature_to_string(function_name: &str, argument_types: &[Type]) -> String {
        format!("{}({})", function_name, argument_types.iter().join(", "))
    }

    #[must_use]
    fn into_trait_function(mut self) -> TraitFunction {
        let body = mem::take(&mut self.body).map(|it| match it {
            BodyOrBuiltin::Body(body) => body,
            BodyOrBuiltin::Builtin(_) => panic!("Trait functions may not be built-in"),
        });
        TraitFunction {
            signature: self.into_function_signature(),
            body,
        }
    }
    #[must_use]
    fn into_function(mut self) -> Function {
        let body = mem::take(&mut self.body).unwrap();
        Function {
            signature: self.into_function_signature(),
            body,
        }
    }
    #[must_use]
    fn into_function_signature(self) -> FunctionSignature {
        FunctionSignature {
            name: self.name,
            type_parameters: self.signature.type_parameters,
            parameters: self.signature.parameters,
            return_type: self.signature.return_type,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature {
    pub type_parameters: Box<[TypeParameter]>,
    pub parameters: Box<[Parameter]>,
    pub return_type: Type,
}
impl Display for Signature {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}({})",
            if self.type_parameters.is_empty() {
                String::new()
            } else {
                format!(
                    "[{}]",
                    self.type_parameters
                        .iter()
                        .map(|it| it.upper_bound.as_ref().map_or_else(
                            || it.name.to_string(),
                            |upper_bound| format!("{}: {upper_bound}", it.name)
                        ))
                        .join(", ")
                )
            },
            self.parameters
                .iter()
                .map(|it| format!("{}: {}", it.name, it.type_))
                .join(", "),
        )
    }
}

impl<'a> Context<'a> {
    fn new(path: &'a Path, ast: &'a Ast) -> Self {
        Self {
            path,
            ast,
            id_generator: IdGenerator::start_at(BuiltinFunction::VARIANTS.len()),
            traits: FxHashMap::default(),
            impls: vec![],
            // Placeholder until `lower_declarations(…)` runs:
            environment: Environment::default(),
            global_identifiers: FxHashMap::default(),
            assignments: FxHashMap::default(),
            assignment_dependency_graph: DiGraph::new(),
            functions: FxHashMap::default(),
            errors: vec![],
            hir: Hir::default(),
        }
    }

    fn into_hir(mut self) -> (Hir, Vec<CompilerError>) {
        self.hir.main_function_id = self.find_main_function().unwrap_or_default();

        if let Err(cycle) = toposort(&self.assignment_dependency_graph, None) {
            let id = *self
                .assignment_dependency_graph
                .node_weight(cycle.node_id())
                .unwrap();
            self.add_error(
                // TODO: report actual error location
                Offset(0)..Offset(0),
                // TODO: print full cycle
                format!(
                    "Cycle in global assignments including `{}`",
                    self.global_identifiers
                        .iter()
                        .find(|(_, named)| *named == &Named::Assignment(id))
                        .unwrap()
                        .0,
                ),
            );
        }

        self.hir.solver_environment = self.environment;
        self.hir.traits = self
            .traits
            .into_iter()
            .map(|(name, trait_)| (name, trait_.into_definition()))
            .collect();
        self.hir.impls = self
            .impls
            .into_iter()
            .map(ImplDeclaration::into_impl)
            .collect();

        let mut assignments = FxHashMap::default();
        let mut functions = FxHashMap::default();
        for (name, named) in self.global_identifiers {
            match named {
                Named::Assignment(id) => {
                    let AssignmentDeclaration { type_, body, .. } =
                        self.assignments.remove(&id).unwrap();
                    assignments.force_insert(
                        id,
                        Assignment {
                            name: name.clone(),
                            type_,
                            body: body.unwrap(),
                        },
                    );
                }
                Named::Functions(ids) => {
                    for id in ids {
                        functions.force_insert(
                            id,
                            self.functions
                                .remove(&id)
                                .unwrap_or_else(|| panic!("Missing {id}"))
                                .into_function(),
                        );
                    }
                }
            };
        }
        self.hir.assignments = assignments;
        self.hir.functions = functions;

        (self.hir, self.errors)
    }
    fn find_main_function(&mut self) -> Option<Id> {
        if let Some(named) = self.global_identifiers.get("main") {
            match named {
                Named::Assignment(assignment) => {
                    let span = self.assignments[assignment]
                        .ast
                        .name
                        .value()
                        .unwrap()
                        .span
                        .clone();
                    self.add_error(span, "`main` must be a function");
                    None
                }
                Named::Functions(ids) => {
                    assert!(!ids.is_empty());

                    let function = &self.functions[ids.first().unwrap()];
                    if ids.len() > 1 {
                        self.add_error(
                            function.ast.unwrap().name.value().unwrap().span.clone(),
                            "Main function may not be overloaded",
                        );
                        None
                    } else if !function.signature.parameters.is_empty() {
                        self.add_error(
                            function.ast.unwrap().name.value().unwrap().span.clone(),
                            "Main function must not have parameters",
                        );
                        None
                    } else if function.signature.return_type != Type::Error
                        && function.signature.return_type != NamedType::int().into()
                    {
                        self.add_error(
                            function.ast.unwrap().name.value().unwrap().span.clone(),
                            "Main function must return an Int",
                        );
                        None
                    } else {
                        Some(ids[0])
                    }
                }
            }
        } else {
            self.add_error(Offset(0)..Offset(0), "Program is missing a main function");
            None
        }
    }

    fn add_builtin_functions(&mut self) {
        for builtin_function in BuiltinFunction::VARIANTS {
            let id = builtin_function.id();
            let signature = builtin_function.signature();
            let type_parameters = signature
                .type_parameters
                .into_vec()
                .into_iter()
                .map(|name| TypeParameter {
                    name,
                    upper_bound: None,
                })
                .collect::<Box<_>>();
            let parameters = signature
                .parameters
                .into_vec()
                .into_iter()
                .map(|(name, type_)| Parameter {
                    id: self.id_generator.generate(),
                    name,
                    type_,
                })
                .collect::<Box<_>>();
            self.functions.force_insert(
                id,
                FunctionDeclaration {
                    ast: None,
                    name: signature.name.clone(),
                    name_span: None,
                    signature: Signature {
                        type_parameters,
                        parameters,
                        return_type: signature.return_type,
                    },
                    body: Some(BodyOrBuiltin::Builtin(*builtin_function)),
                },
            );
            self.global_identifiers
                .entry(signature.name)
                .and_modify(|it| match it {
                    Named::Assignment(_) => panic!(),
                    Named::Functions(function_ids) => function_ids.push(id),
                })
                .or_insert_with(|| Named::Functions(vec![id]));
        }
    }

    fn lower_declarations(&mut self) {
        // TODO: remove these vecs?
        let mut assignments_to_lower = vec![];
        let mut functions_to_lower = vec![];
        for declaration in self.ast {
            let AstDeclaration::Trait(trait_) = declaration else {
                continue;
            };
            self.lower_trait_definition(trait_);
        }
        for declaration in self.ast {
            match declaration {
                AstDeclaration::Struct(struct_) => self.lower_struct(struct_),
                AstDeclaration::Enum(enum_) => self.lower_enum(enum_),
                AstDeclaration::Trait(_) => {}
                AstDeclaration::Impl(impl_) => self.lower_impl(impl_),
                AstDeclaration::Assignment(assignment) => {
                    if let Some(id) = self.lower_assignment_signature(assignment) {
                        assignments_to_lower.push(id);
                    }
                }
                AstDeclaration::Function(function) => {
                    if let Some((id, function)) =
                        self.lower_top_level_function_signature(&[], None, function)
                    {
                        // dbg!(&function);
                        self.functions.force_insert(id, function);
                        functions_to_lower.push(id);
                    }
                }
            }
        }

        for trait_name in self.traits.keys().cloned().collect_vec() {
            let trait_ = &self.traits[&trait_name];
            let type_parameters = trait_.type_parameters.clone();
            let self_base_type = NamedType::new(trait_name.clone(), type_parameters.type_()).into();
            for (id, mut function) in trait_.functions.clone() {
                self.lower_function(Some(&self_base_type), &type_parameters, &mut function, true);
                self.traits
                    .get_mut(&trait_name)
                    .unwrap()
                    .functions
                    .insert(id, function)
                    .unwrap();
            }
        }
        for index in 0..self.impls.len() {
            let impl_ = &self.impls[index];
            let type_parameters = impl_.type_parameters.clone();
            let self_base_type = impl_.type_.clone();
            for (id, mut function) in impl_.functions.clone() {
                self.lower_function(
                    Some(&self_base_type),
                    &type_parameters,
                    &mut function,
                    false,
                );
                self.impls[index].functions.insert(id, function).unwrap();
            }
        }
        for id in assignments_to_lower {
            self.lower_assignment(id);
        }
        for id in functions_to_lower {
            let mut function = self.functions.get(&id).unwrap().clone();
            self.lower_function(None, &[], &mut function, false);
            self.functions.insert(id, function).unwrap();
        }
    }

    fn lower_struct(&mut self, struct_type: &'a AstStruct) {
        let Some(name) = struct_type.name.value() else {
            return;
        };

        let type_parameters =
            self.lower_type_parameters(&[], None, struct_type.type_parameters.as_ref());
        let self_base_type = NamedType::new(name.string.clone(), type_parameters.type_()).into();

        let fields = match &struct_type.kind {
            AstStructKind::Builtin { .. } => None,
            AstStructKind::UserDefined { fields, .. } => Some(
                fields
                    .iter()
                    .filter_map(|field| {
                        let name = field.name.value()?;

                        let type_ = self.lower_type(
                            &type_parameters,
                            Some(&self_base_type),
                            field.type_.value(),
                        );
                        Some(StructField {
                            name: name.string.clone(),
                            type_,
                        })
                    })
                    .collect(),
            ),
        };

        if self.traits.contains_key(&name.string) {
            self.add_error(
                name.span.clone(),
                format!("Duplicate type name: `{}`", name.string),
            );
            return;
        }
        match self.hir.type_declarations.entry(name.string.clone()) {
            Entry::Occupied(_) => {
                self.add_error(
                    name.span.clone(),
                    format!("Duplicate type name: `{}`", name.string),
                );
            }
            Entry::Vacant(entry) => {
                entry.insert(TypeDeclaration {
                    type_parameters,
                    kind: TypeDeclarationKind::Struct { fields },
                });
            }
        };
    }
    fn lower_enum(&mut self, enum_type: &'a AstEnum) {
        let Some(name) = enum_type.name.value() else {
            return;
        };

        let type_parameters =
            self.lower_type_parameters(&[], None, enum_type.type_parameters.as_ref());
        let self_base_type = NamedType::new(name.string.clone(), type_parameters.type_()).into();

        let variants = enum_type
            .variants
            .iter()
            .filter_map(|variant| {
                let name = variant.name.value()?;

                let type_ = variant
                    .type_
                    .as_ref()
                    .map(|it| self.lower_type(&type_parameters, Some(&self_base_type), it.value()));
                Some((name.string.clone(), type_))
            })
            .collect();

        if self.traits.contains_key(&name.string) {
            self.add_error(
                name.span.clone(),
                format!("Duplicate type name: `{}`", name.string),
            );
            return;
        }
        match self.hir.type_declarations.entry(name.string.clone()) {
            Entry::Occupied(_) => {
                self.add_error(
                    name.span.clone(),
                    format!("Duplicate type name: `{}`", name.string),
                );
            }
            Entry::Vacant(entry) => {
                entry.insert(TypeDeclaration {
                    type_parameters,
                    kind: TypeDeclarationKind::Enum { variants },
                });
            }
        };
    }

    fn lower_trait_definition(&mut self, trait_: &'a AstTrait) {
        let Some(name) = trait_.name.value() else {
            return;
        };

        let type_parameters =
            self.lower_type_parameters(&[], None, trait_.type_parameters.as_ref());

        let solver_goal = SolverGoal {
            trait_: name.string.clone(),
            parameters: vec![SolverVariable::self_().into()].into_boxed_slice(),
        };
        let solver_subgoals = type_parameters
            .iter()
            .filter_map(|type_parameter| type_parameter.clone().try_into().ok())
            .collect();

        let self_base_type = NamedType::new(name.string.clone(), type_parameters.type_()).into();
        // TODO: check that functions accept self as first parameter
        let functions = trait_
            .functions
            .iter()
            .filter_map(|function| {
                self.lower_function_signature(&type_parameters, Some(&self_base_type), function)
                    .map(|function| (self.id_generator.generate(), function))
            })
            .collect();

        if self.hir.type_declarations.contains_key(&name.string) {
            self.add_error(
                name.span.clone(),
                format!("Duplicate type name: `{}`", name.string),
            );
            return;
        }
        match self.traits.entry(name.string.clone()) {
            Entry::Occupied(_) => {
                self.add_error(
                    name.span.clone(),
                    format!("Duplicate type name: `{}`", name.string),
                );
            }
            Entry::Vacant(entry) => {
                entry.insert(TraitDeclaration {
                    type_parameters,
                    solver_goal,
                    solver_subgoals,
                    functions,
                });
            }
        };
    }
    fn lower_trait(
        &mut self,
        type_parameters: &[TypeParameter],
        self_base_type: Option<&Type>,
        type_: impl Into<Option<&AstType>>,
    ) -> hir::Result<Trait> {
        let type_: Option<&AstType> = type_.into();
        let Some(type_) = type_ else {
            return hir::Err;
        };

        let type_ = match type_ {
            AstType::Named(type_) => type_,
            AstType::Function(function_type) => {
                self.add_error(function_type.span.clone(), "Function type is not a trait");
                return hir::Err;
            }
        };
        let Some(name) = type_.name.value() else {
            return hir::Err;
        };

        if &*name.string == "Self" {
            self.add_error(name.span.clone(), "`Self` is not a trait.");
            return hir::Err;
        }

        if type_parameters.iter().any(|it| it.name == name.string) {
            self.add_error(
                name.span.clone(),
                format!("`{}` is a type parameter, not a trait.", name.string),
            );
            return hir::Err;
        }

        let type_arguments = type_
            .type_arguments
            .as_ref()
            .map_or_else(Box::default, |it| {
                it.arguments
                    .iter()
                    .map(|it| self.lower_type(type_parameters, self_base_type, &it.type_))
                    .collect::<Box<_>>()
            });

        let type_parameters = match self.ast.iter().find_map(|it| match it {
            AstDeclaration::Trait(AstTrait {
                name: it_name,
                type_parameters,
                ..
            }) if it_name.value().map(|it| &it.string) == Some(&name.string) => {
                Some(Ok(type_parameters
                    .as_ref()
                    .map_or::<&[AstTypeParameter], _>(&[], |it| &it.parameters)))
            }
            AstDeclaration::Struct(AstStruct { name: it_name, .. })
            | AstDeclaration::Enum(AstEnum { name: it_name, .. })
                if it_name.value().map(|it| &it.string) == Some(&name.string) =>
            {
                self.add_error(name.span.clone(), "Types can't be used as traits.");
                Some(Err(()))
            }
            _ => None,
        }) {
            Some(Ok(type_parameters)) => type_parameters,
            Some(Err(())) => return hir::Err,
            None => {
                self.add_error(name.span.clone(), format!("Unknown trait: `{}`", **name));
                return hir::Err;
            }
        };

        let type_arguments: Box<[Type]> = if type_arguments.len() == type_parameters.len() {
            type_arguments
        } else {
            self.add_error(
                type_.type_arguments.as_ref().unwrap().span.clone(),
                format!(
                    "Expected {} type {}, got {}.",
                    type_parameters.len(),
                    if type_parameters.len() == 1 {
                        "argument"
                    } else {
                        "arguments"
                    },
                    type_arguments.len(),
                ),
            );
            if type_arguments.len() < type_parameters.len() {
                let missing_count = type_parameters.len() - type_arguments.len();
                type_arguments
                    .into_vec()
                    .into_iter()
                    .chain(iter::repeat_n(Type::Error, missing_count))
                    .collect()
            } else {
                let mut type_arguments = type_arguments.into_vec();
                type_arguments.truncate(type_parameters.len());
                type_arguments.into_boxed_slice()
            }
        };

        hir::Ok(Trait {
            name: name.string.clone(),
            type_arguments,
        })
    }
    fn lower_impl(&mut self, impl_: &'a AstImpl) {
        let type_parameters = self.lower_type_parameters(&[], None, impl_.type_parameters.as_ref());

        let Some(ast_type) = impl_.type_.value() else {
            return;
        };
        let type_ = self.lower_type(&type_parameters, None, ast_type);

        let Some(trait_) = impl_.trait_.value() else {
            return;
        };
        let hir::Ok(trait_) = self.lower_trait(&type_parameters, Some(&type_), trait_) else {
            return;
        };
        let trait_declaration = self.traits[&*trait_.name].clone();

        if let Ok(solver_type) = SolverType::try_from(type_.clone()) {
            let rule = SolverRule {
                goal: SolverGoal {
                    trait_: trait_.name.clone(),
                    parameters: trait_declaration
                        .type_parameters
                        .iter()
                        .map(|it| SolverVariable::new(it.type_()).into())
                        .chain([solver_type])
                        .collect(),
                },
                subgoals: type_parameters
                    .iter()
                    .filter_map(|type_parameter| type_parameter.clone().try_into().ok())
                    .collect(),
            };
            self.environment.rules.push(rule);
        };

        let trait_substitutions = trait_declaration
            .type_parameters
            .iter()
            .map(TypeParameter::type_)
            .zip(trait_.type_arguments.iter().cloned())
            .chain([(ParameterType::self_type(), type_.clone())])
            .collect();

        let functions = impl_
            .functions
            .iter()
            .filter_map(|function| try {
                let function =
                    self.lower_function_signature(&type_parameters, Some(&type_), function)?;
                let Some(parent_function_id) =
                    trait_declaration.find_parent_function_of(&trait_substitutions, &function)
                else {
                    self.add_error(
                        function.name_span.unwrap(),
                        "This function is not part of the implemented trait.",
                    );
                    return None;
                };

                (parent_function_id, function)
            })
            .collect::<FxHashMap<_, _>>();
        if functions.len() < trait_declaration.functions.len() {
            for (trait_function_id, trait_function) in trait_declaration.functions {
                if !functions.contains_key(&trait_function_id) {
                    self.add_error(
                        impl_.impl_keyword_span.clone(),
                        format!(
                            "Missing implementation of function {}.",
                            trait_function.signature_to_string(),
                        ),
                    );
                }
            }
        }

        self.impls.push(ImplDeclaration {
            type_parameters,
            type_,
            trait_,
            functions,
        });
    }

    fn lower_type_parameters(
        &mut self,
        outer_type_parameters: &[TypeParameter],
        self_base_type: Option<&Type>,
        type_parameters: Option<&AstTypeParameters>,
    ) -> Box<[TypeParameter]> {
        type_parameters.map_or_else(Box::default, |it| {
            it.parameters
                .iter()
                .filter_map(|it| {
                    let name = it.name.value()?;
                    if outer_type_parameters
                        .iter()
                        .any(|it| it.name == name.string)
                    {
                        self.add_error(
                            name.span.clone(),
                            format!("Duplicate type parameter name: `{}`", name.string),
                        );
                        return None;
                    }

                    let upper_bound = it
                        .upper_bound
                        .as_ref()
                        .and_then(|it| it.value())
                        .map(|it| self.lower_trait(outer_type_parameters, self_base_type, it));
                    Some(TypeParameter {
                        name: name.string.clone(),
                        upper_bound,
                    })
                })
                .collect()
        })
    }
    fn lower_type(
        &mut self,
        type_parameters: &[TypeParameter],
        self_base_type: Option<&Type>,
        type_: impl Into<Option<&AstType>>,
    ) -> Type {
        let type_: Option<&AstType> = type_.into();
        let Some(type_) = type_ else {
            return Type::Error;
        };

        let type_ = match type_ {
            AstType::Named(type_) => type_,
            AstType::Function(type_) => {
                return Type::Function(FunctionType {
                    parameter_types: type_
                        .parameter_types
                        .iter()
                        .map(|it| self.lower_type(type_parameters, self_base_type, &*it.type_))
                        .collect(),
                    return_type: Box::new(self.lower_type(
                        type_parameters,
                        self_base_type,
                        type_.return_type.value().map(AsRef::as_ref),
                    )),
                })
            }
        };
        let Some(name) = type_.name.value() else {
            return Type::Error;
        };

        if &*name.string == "Self" {
            return self_base_type.map_or_else(
                || {
                    self.add_error(
                        name.span.clone(),
                        "`Self` can only be used in traits and impls",
                    );
                    Type::Error
                },
                |self_base_type| Type::Self_ {
                    base_type: Box::new(self_base_type.clone()),
                },
            );
        }

        if let Some(type_parameter) = type_parameters.iter().find(|it| it.name == name.string) {
            if let Some(type_arguments) = &type_.type_arguments {
                self.add_error(
                    type_arguments.span.clone(),
                    "Type parameters can't have type arguments",
                );
            }
            return type_parameter.type_().into();
        }

        let type_arguments = type_
            .type_arguments
            .as_ref()
            .map_or_else(Box::default, |it| {
                it.arguments
                    .iter()
                    .map(|it| self.lower_type(type_parameters, self_base_type, &it.type_))
                    .collect::<Box<_>>()
            });

        let type_parameters = match self.ast.iter().find_map(|it| match it {
            AstDeclaration::Struct(AstStruct {
                name: it_name,
                type_parameters,
                ..
            })
            | AstDeclaration::Enum(AstEnum {
                name: it_name,
                type_parameters,
                ..
            }) if it_name.value().map(|it| &it.string) == Some(&name.string) => {
                Some(Ok(type_parameters
                    .as_ref()
                    .map_or::<&[AstTypeParameter], _>(&[], |it| &it.parameters)))
            }
            AstDeclaration::Trait(AstTrait { name: it_name, .. })
                if it_name.value().map(|it| &it.string) == Some(&name.string) =>
            {
                self.add_error(name.span.clone(), "Traits can't be used as types");
                Some(Err(()))
            }
            _ => None,
        }) {
            Some(Ok(type_parameters)) => type_parameters,
            Some(Err(())) => return Type::Error,
            None => {
                self.add_error(name.span.clone(), format!("Unknown type: `{}`", **name));
                return Type::Error;
            }
        };

        let type_arguments: Box<[Type]> = if type_arguments.len() == type_parameters.len() {
            type_arguments
        } else {
            self.add_error(
                type_.type_arguments.as_ref().unwrap().span.clone(),
                format!(
                    "Expected {} type {}, got {}.",
                    type_parameters.len(),
                    if type_parameters.len() == 1 {
                        "argument"
                    } else {
                        "arguments"
                    },
                    type_arguments.len(),
                ),
            );
            if type_arguments.len() < type_parameters.len() {
                let missing_count = type_parameters.len() - type_arguments.len();
                type_arguments
                    .into_vec()
                    .into_iter()
                    .chain(iter::repeat_n(Type::Error, missing_count))
                    .collect()
            } else {
                let mut type_arguments = type_arguments.into_vec();
                type_arguments.truncate(type_parameters.len());
                type_arguments.into_boxed_slice()
            }
        };

        NamedType {
            name: name.string.clone(),
            type_arguments,
        }
        .into()
    }

    fn lower_assignment_signature(&mut self, assignment: &'a AstAssignment) -> Option<Id> {
        let name = assignment.name.value()?;

        let id = self.id_generator.generate();
        // TODO: infer type
        let type_ = if let Some(type_) = assignment.type_.as_ref() {
            self.lower_type(&[], None, type_.value())
        } else {
            self.add_error(
                assignment.display_span.clone(),
                "Assignment is missing a type",
            );
            Type::Error
        };

        match self.global_identifiers.entry(name.string.clone()) {
            Entry::Occupied(mut entry) => {
                self.errors.push(CompilerError {
                    path: self.path.to_path_buf(),
                    span:
                    name.span.clone(),
                    message: match entry.get_mut() {
                        Named::Functions(_) => "A top-level assignment can't have the same name as a top-level function.".to_string(),
                        Named::Assignment(_) => "Top-level assignments can't have the same name.".to_string(),
                    },
                });
                return None;
            }
            Entry::Vacant(entry) => {
                entry.insert(Named::Assignment(id));
            }
        }

        let graph_index = self.assignment_dependency_graph.add_node(id);

        self.assignments.force_insert(
            id,
            AssignmentDeclaration {
                ast: assignment,
                type_,
                graph_index,
                body: None,
            },
        );
        Some(id)
    }
    fn lower_assignment(&mut self, id: Id) {
        let declaration = self.assignments.get(&id).unwrap();
        let value = &declaration.ast.value;
        let type_ = declaration.type_.clone();
        let graph_index = declaration.graph_index;

        let (hir_body, global_assignment_dependencies) =
            BodyBuilder::build(self, &[], None, |builder| {
                if let Some(value) = value.value() {
                    builder.lower_expression(value, Some(&type_));
                } else {
                    builder.push_error();
                }
            });

        for dependency_id in global_assignment_dependencies {
            let dependency = self.assignments.get(&dependency_id).unwrap();
            self.assignment_dependency_graph
                .add_edge(graph_index, dependency.graph_index, ());
        }

        self.assignments.get_mut(&id).unwrap().body = Some(hir_body);
    }

    fn lower_top_level_function_signature(
        &mut self,
        outer_type_parameters: &[TypeParameter],
        self_base_type: Option<&Type>,
        function: &'a AstFunction,
    ) -> Option<(Id, FunctionDeclaration<'a>)> {
        let function =
            self.lower_function_signature(outer_type_parameters, self_base_type, function)?;
        let id = self.id_generator.generate();
        match self.global_identifiers.entry(function.name.clone()) {
            Entry::Occupied(mut entry) => match entry.get_mut() {
                Named::Functions(functions) => {
                    // TODO: check for invalid overloads
                    functions.push(id);
                }
                Named::Assignment(_) => {
                    self.add_error(
                        function.ast.unwrap().display_span.clone(),
                        "A top-level function can't have the same name as a top-level assignment.",
                    );
                    return None;
                }
            },
            Entry::Vacant(entry) => {
                entry.insert(Named::Functions(vec![id]));
            }
        }
        Some((id, function))
    }
    fn lower_function_signature(
        &mut self,
        outer_type_parameters: &[TypeParameter],
        self_base_type: Option<&Type>,
        function: &'a AstFunction,
    ) -> Option<FunctionDeclaration<'a>> {
        let name = function.name.value()?;

        let type_parameters = self.lower_type_parameters(
            outer_type_parameters,
            self_base_type,
            function.type_parameters.as_ref(),
        );
        let all_type_parameters = outer_type_parameters
            .iter()
            .chain(type_parameters.iter())
            .cloned()
            .collect::<Box<_>>();

        let parameters = function
            .parameters
            .value()
            .map(|it| self.lower_parameters(&all_type_parameters, self_base_type, it))
            .unwrap_or_default();
        let return_type = function.return_type.as_ref().map_or_else(
            || NamedType::nothing().into(),
            |it| self.lower_type(&all_type_parameters, self_base_type, it),
        );
        Some(FunctionDeclaration {
            ast: Some(function),
            name: name.string.clone(),
            name_span: Some(name.span.clone()),
            signature: Signature {
                type_parameters,
                parameters,
                return_type,
            },
            body: None,
        })
    }
    fn lower_parameters(
        &mut self,
        type_parameters: &[TypeParameter],
        self_base_type: Option<&Type>,
        parameters: &'a AstParameters,
    ) -> Box<[Parameter]> {
        let mut parameter_names = FxHashSet::default();
        parameters
            .parameters
            .iter()
            .filter_map(|parameter| try {
                let name = parameter.name.value()?.clone();
                if !parameter_names.insert(name.clone()) {
                    self.add_error(
                        name.span.clone(),
                        format!("Duplicate parameter name: {}", *name),
                    );
                    return None;
                }

                let type_ =
                    self.lower_type(type_parameters, self_base_type, parameter.type_.value());

                let id = self.id_generator.generate();
                Parameter {
                    id,
                    name: name.string,
                    type_,
                }
            })
            .collect()
    }
    fn lower_function(
        &mut self,
        self_base_type: Option<&Type>,
        outer_type_parameters: &[TypeParameter],
        function: &mut FunctionDeclaration<'a>,
        body_is_optional: bool,
    ) {
        if body_is_optional && function.ast.unwrap().body.is_none() {
            return;
        }

        let (hir_body, _) = BodyBuilder::build(
            self,
            &outer_type_parameters
                .iter()
                .cloned()
                .chain(function.signature.type_parameters.iter().cloned())
                .collect_vec(),
            self_base_type,
            |builder| {
                for parameter in &function.signature.parameters {
                    builder.push_parameter(parameter.clone());
                }

                if let Some(body) = function.ast.unwrap().body.as_ref() {
                    builder
                        .lower_statements(&body.statements, Some(&function.signature.return_type));
                } else {
                    builder.context.add_error(
                        function.ast.unwrap().display_span.clone(),
                        "No function body provided",
                    );
                    builder.push_panic("No function body provided");
                }
            },
        );

        function.body = Some(BodyOrBuiltin::Body(hir_body));
    }
    fn get_function(&self, id: Id) -> (&FunctionDeclaration<'a>, Option<&TraitDeclaration<'a>>) {
        self.functions
            .get(&id)
            .map(|function| (function, None))
            .or_else(|| {
                self.traits.iter().find_map(|(_, trait_)| {
                    trait_
                        .functions
                        .get(&id)
                        .map(|function| (function, Some(trait_)))
                })
            })
            .unwrap()
    }

    fn get_all_functions_matching_name(&self, name: &str) -> Vec<Id> {
        self.functions
            .iter()
            .chain(self.traits.iter().flat_map(|(_, trait_)| &trait_.functions))
            .filter(|(_, function)| &*function.name == name)
            .map(|(id, _)| *id)
            .collect()
    }

    fn add_error(&mut self, span: Range<Offset>, message: impl Into<String>) {
        self.errors.push(CompilerError {
            path: self.path.to_path_buf(),
            span,
            message: message.into(),
        });
    }
}

struct BodyBuilder<'c, 'a> {
    context: &'c mut Context<'a>,
    global_assignment_dependencies: FxHashSet<Id>,
    type_parameters: &'c [TypeParameter],
    self_base_type: Option<&'c Type>,
    local_identifiers: Vec<(Box<str>, Id, Type)>,
    body: Body,
}
impl<'c, 'a> BodyBuilder<'c, 'a> {
    #[must_use]
    fn build(
        context: &'c mut Context<'a>,
        type_parameters: &'c [TypeParameter],
        self_base_type: Option<&'c Type>,
        fun: impl FnOnce(&mut Self),
    ) -> (Body, FxHashSet<Id>) {
        let mut builder = Self {
            context,
            global_assignment_dependencies: FxHashSet::default(),
            type_parameters,
            self_base_type,
            local_identifiers: vec![],
            body: Body::default(),
        };
        fun(&mut builder);
        (builder.body, builder.global_assignment_dependencies)
    }
    #[must_use]
    fn build_inner<'s, 'cc>(&'s mut self, fun: impl FnOnce(&mut BodyBuilder<'cc, 'a>)) -> Body
    where
        'c: 'cc,
        's: 'cc,
    {
        BodyBuilder::<'cc, 'a>::build(
            self.context,
            self.type_parameters,
            self.self_base_type,
            |builder| {
                builder
                    .local_identifiers
                    .clone_from(&self.local_identifiers);
                fun(builder);
                self.global_assignment_dependencies
                    .extend(&builder.global_assignment_dependencies);
            },
        )
        .0
    }

    fn lower_statements(
        &mut self,
        statements: &'a [AstStatement],
        context_type: Option<&Type>,
    ) -> (Id, Type) {
        let mut last_expression = None;
        for (position, statement) in statements.iter().with_position() {
            let statement_context_type = if matches!(position, Position::Last | Position::Only) {
                context_type
            } else {
                None
            };

            match statement {
                AstStatement::Assignment(assignment) => {
                    let Some(name) = assignment.name.value().cloned() else {
                        continue;
                    };

                    let type_ = assignment.type_.as_ref().map(|it| {
                        self.context.lower_type(
                            self.type_parameters,
                            self.self_base_type,
                            it.value(),
                        )
                    });

                    let (id, type_) = if let Some(value) = assignment.value.value() {
                        self.lower_expression(value, type_.as_ref())
                    } else {
                        (self.push_error(), Type::Error)
                    };
                    self.push(name.string.clone(), ExpressionKind::Reference(id), type_);
                    last_expression = None;
                }
                AstStatement::Expression(expression) => {
                    last_expression =
                        Some(self.lower_expression(expression, statement_context_type));
                }
            }
        }

        let last_expression = last_expression.unwrap_or_else(|| {
            // TODO: check return type
            let id = self.push_nothing();
            (id, NamedType::nothing().into())
        });
        let return_type = context_type.cloned().unwrap_or(last_expression.1);
        (last_expression.0, return_type)
    }

    fn lower_expression(
        &mut self,
        expression: &'a AstExpression,
        context_type: Option<&Type>,
    ) -> (Id, Type) {
        match self.lower_expression_raw(expression, context_type) {
            LoweredExpression::Expression { id, type_ } => {
                if let Some(context_type) = context_type
                    && !type_.equals_lenient(context_type)
                {
                    self.context.add_error(
                        expression.span.clone(),
                        format!("Expected type `{context_type}`, got `{type_}`."),
                    );
                    (self.push_error(), Type::Error)
                } else {
                    (id, type_)
                }
            }
            LoweredExpression::NamedTypeReference(_)
            | LoweredExpression::TypeParameterReference { .. } => {
                self.context
                    .add_error(expression.span.clone(), "Type must be instantiated.");
                (self.push_error(), Type::Error)
            }
            LoweredExpression::Error => (self.push_error(), Type::Error),
        }
    }
    fn lower_expression_raw(
        &mut self,
        expression: &'a AstExpression,
        context_type: Option<&Type>,
    ) -> LoweredExpression {
        match &expression.kind {
            AstExpressionKind::Identifier(identifier) => {
                let Some(identifier) = identifier.identifier.value() else {
                    return LoweredExpression::Error;
                };
                self.lower_identifier(identifier)
            }
            AstExpressionKind::Int(int) => self.push_lowered(
                None,
                int.value
                    .value()
                    .map_or(ExpressionKind::Error, |it| ExpressionKind::Int(*it)),
                NamedType::int(),
            ),
            AstExpressionKind::Text(text) => {
                let text = text
                    .parts
                    .iter()
                    .map::<Id, _>(|it| match it {
                        AstTextPart::Text(text) => self.push(
                            None,
                            ExpressionKind::Text(text.value().cloned().unwrap_or_default()),
                            NamedType::text(),
                        ),
                        AstTextPart::Interpolation { expression, .. } => {
                            if let Some(expression) = expression.value() {
                                // TODO: accept impl ToText
                                self.lower_expression(expression, Some(&NamedType::text().into()))
                                    .0
                            } else {
                                self.push_error()
                            }
                        }
                    })
                    .collect_vec()
                    .into_iter()
                    .reduce(|lhs, rhs| {
                        self.push(
                            None,
                            ExpressionKind::Call {
                                function: BuiltinFunction::TextConcat.id(),
                                substitutions: FxHashMap::default(),
                                arguments: [lhs, rhs].into(),
                            },
                            NamedType::text(),
                        )
                    })
                    .unwrap_or_else(|| {
                        self.push(None, ExpressionKind::Text("".into()), NamedType::text())
                    });
                LoweredExpression::Expression {
                    id: text,
                    type_: NamedType::text().into(),
                }
            }
            AstExpressionKind::Parenthesized(parenthesized) => parenthesized
                .inner
                .value()
                .map_or(LoweredExpression::Error, |it| {
                    self.lower_expression_raw(it, context_type)
                }),
            AstExpressionKind::Call(call) => {
                let type_arguments = call.type_arguments.as_ref().map(|it| {
                    it.arguments
                        .iter()
                        .map(|it| {
                            self.context.lower_type(
                                self.type_parameters,
                                self.self_base_type,
                                &it.type_,
                            )
                        })
                        .collect_vec()
                });
                match &call.receiver.kind {
                    AstExpressionKind::Navigation(navigation)
                        if let Some(key) = navigation.key.value() =>
                    {
                        let receiver = self.lower_expression_raw(&navigation.receiver, None);
                        match receiver {
                            LoweredExpression::Expression { id, type_ } => {
                                // bar.foo(baz)
                                let arguments = self.lower_arguments(&call.arguments);
                                let arguments = iter::once((id, type_))
                                    .chain(arguments.into_vec())
                                    .collect_vec();
                                self.lower_call(key, type_arguments.as_deref(), &arguments)
                            }
                            LoweredExpression::NamedTypeReference(type_) => {
                                // Foo.blub(bar, baz)
                                let type_declaration =
                                    self.context.hir.type_declarations[&type_].clone();
                                match &type_declaration.kind {
                                    TypeDeclarationKind::Struct { .. } => {
                                        self.context.add_error(
                                            key.span.clone(),
                                            format!(
                                                "Struct type `{type_:?}` doesn't have a field `{}`",
                                                key.string,
                                            ),
                                        );
                                        LoweredExpression::Error
                                    }
                                    TypeDeclarationKind::Enum { variants } => self
                                        .lower_enum_creation(
                                            call,
                                            type_arguments.as_deref(),
                                            &type_,
                                            key,
                                            &type_declaration.type_parameters,
                                            variants,
                                        ),
                                }
                            }
                            LoweredExpression::TypeParameterReference(_) => unreachable!(),
                            LoweredExpression::Error => LoweredExpression::Error,
                        }
                    }
                    AstExpressionKind::Identifier(identifier) => {
                        let Some(identifier) = identifier.identifier.value() else {
                            return LoweredExpression::Error;
                        };

                        if identifier.string.chars().next().unwrap().is_lowercase() {
                            // foo(bar, baz)
                            let arguments = self.lower_arguments(&call.arguments);
                            return self.lower_call(
                                identifier,
                                type_arguments.as_deref(),
                                &arguments,
                            );
                        }

                        match self.lower_identifier(identifier) {
                            LoweredExpression::Expression { .. } => todo!("support lambdas"),
                            LoweredExpression::NamedTypeReference(type_) => {
                                let type_declaration =
                                    self.context.hir.type_declarations[&type_].clone();
                                match &type_declaration.kind {
                                    TypeDeclarationKind::Struct { fields } => {
                                        // Foo(bar, baz)
                                        self.lower_struct_creation(
                                            expression.span.clone(),
                                            call,
                                            type_arguments.as_deref(),
                                            &type_,
                                            &type_declaration.type_parameters,
                                            fields.as_deref(),
                                        )
                                    }
                                    TypeDeclarationKind::Enum { .. } => {
                                        self.context.add_error(
                                            call.receiver.span.clone(),
                                            "Enum variant is missing.",
                                        );
                                        LoweredExpression::Error
                                    }
                                }
                            }
                            LoweredExpression::TypeParameterReference(type_) => {
                                self.context.add_error(
                                    call.receiver.span.clone(),
                                    format!("Can't instantiate type parameter {type_} directly."),
                                );
                                LoweredExpression::Error
                            }
                            LoweredExpression::Error => LoweredExpression::Error,
                        }
                    }
                    receiver => todo!("Support calling other expressions: {receiver:?}"),
                }
            }
            AstExpressionKind::Navigation(navigation) => {
                let receiver = self.lower_expression_raw(&navigation.receiver, None);

                let Some(key) = navigation.key.value() else {
                    return LoweredExpression::Error;
                };

                match receiver {
                    LoweredExpression::Expression {
                        id: receiver_id,
                        type_: receiver_type,
                    } => match &receiver_type {
                        Type::Named(named_type) => {
                            let type_ = &self.context.hir.type_declarations[&named_type.name];
                            if let TypeDeclarationKind::Struct {
                                fields: Some(fields),
                            } = &type_.kind
                                && let Some(field) = fields.iter().find(|it| it.name == key.string)
                            {
                                return self.push_lowered(
                                    None,
                                    ExpressionKind::StructAccess {
                                        struct_: receiver_id,
                                        field: key.string.clone(),
                                    },
                                    field.type_.substitute(&Type::build_environment(
                                        &type_.type_parameters,
                                        &named_type.type_arguments,
                                    )),
                                );
                            }

                            self.context.add_error(
                                key.span.clone(),
                                format!(
                                    "Value of type `{receiver_type:?}` doesn't have a field `{}`",
                                    key.string
                                ),
                            );
                            LoweredExpression::Error
                        }
                        Type::Parameter(type_) => {
                            self.context.add_error(
                                key.span.clone(),
                                format!(
                                    "Navigation on value of type parameter type `{}` is not supported yet.",type_.name
                                ),
                            );
                            LoweredExpression::Error
                        }
                        Type::Self_ { .. } => todo!(),
                        Type::Function(type_) => {
                            self.context.add_error(
                                key.span.clone(),
                                format!(
                                    "Navigation on value of function type `{type_}` is not supported yet."
                                ),
                            );
                            LoweredExpression::Error
                        }
                        Type::Error => LoweredExpression::Error,
                    },
                    LoweredExpression::NamedTypeReference(type_) => {
                        let declaration = self.context.hir.type_declarations.get(&type_).unwrap();
                        match &declaration.kind {
                            TypeDeclarationKind::Struct { .. } => {
                                self.context.add_error(
                                    key.span.clone(),
                                    format!(
                                        "Struct type `{type_:?}` doesn't have a field `{}`",
                                        key.string,
                                    ),
                                );
                                LoweredExpression::Error
                            }
                            TypeDeclarationKind::Enum { variants } => {
                                if variants.iter().any(|(name, _)| name == &key.string) {
                                    self.context.add_error(
                                        key.span.clone(),
                                        format!(
                                            "Enum variant `{type_:?}.{}` must be called to create it",
                                            key.string,
                                        ),
                                    );
                                } else {
                                    self.context.add_error(
                                        key.span.clone(),
                                        format!(
                                            "Enum `{type_:?}` doesn't have a variant `{}`",
                                            key.string,
                                        ),
                                    );
                                }
                                LoweredExpression::Error
                            }
                        }
                    }
                    LoweredExpression::TypeParameterReference(type_parameter) => {
                        self.context.add_error(
                            key.span.clone(),
                            format!(
                                "Parameter type `{type_parameter:?}` doesn't have a field `{}`",
                                key.string,
                            ),
                        );
                        LoweredExpression::Error
                    }
                    LoweredExpression::Error => LoweredExpression::Error,
                }
            }
            AstExpressionKind::Lambda(AstLambda { parameters, body }) => {
                let parameters = self.context.lower_parameters(
                    self.type_parameters,
                    self.self_base_type,
                    parameters,
                );

                let body = self.build_inner(|builder| {
                    for parameter in &parameters {
                        builder.push_parameter(parameter.clone());
                    }

                    // TODO: pass context_type
                    builder.lower_statements(
                        &body.statements,
                        if let Some(Type::Function(FunctionType {
                            box return_type, ..
                        })) = context_type
                        {
                            Some(return_type)
                        } else {
                            None
                        },
                    );
                });
                let type_ = FunctionType::new(
                    parameters.iter().map(|it| it.type_.clone()).collect_vec(),
                    body.return_type().clone(),
                );
                self.push_lowered(None, ExpressionKind::Lambda { parameters, body }, type_)
            }
            AstExpressionKind::Body(AstBody { statements, .. }) => {
                let (id, type_) = self.lower_statements(statements, context_type);
                LoweredExpression::Expression { id, type_ }
            }
            AstExpressionKind::Switch(AstSwitch { value, cases, .. }) => {
                let Some(value) = value.value() else {
                    return LoweredExpression::Error;
                };
                let (value, enum_) = self.lower_expression(value, None);

                let (environment, variants) = match &enum_ {
                    Type::Named(type_) => {
                        let Some(declaration) =
                            &self.context.hir.type_declarations.get(&type_.name)
                        else {
                            self.context.add_error(
                                expression.span.clone(),
                                format!("Can't switch over builtin type `{enum_:?}`"),
                            );
                            return LoweredExpression::Error;
                        };
                        match &declaration.kind {
                            TypeDeclarationKind::Struct { .. } => {
                                self.context.add_error(
                                    expression.span.clone(),
                                    format!("Can't switch over struct `{enum_:?}`"),
                                );
                                return LoweredExpression::Error;
                            }
                            TypeDeclarationKind::Enum { variants } => (
                                declaration
                                    .type_parameters
                                    .iter()
                                    .map(TypeParameter::type_)
                                    .zip_eq(type_.type_arguments.iter().cloned())
                                    .collect::<FxHashMap<_, _>>(),
                                variants.clone(),
                            ),
                        }
                    }
                    Type::Parameter(type_) => {
                        self.context.add_error(
                            expression.span.clone(),
                            format!(
                                "Can't switch over value of type parameter type `{}`",
                                type_.name
                            ),
                        );
                        return LoweredExpression::Error;
                    }
                    Type::Self_ { .. } => todo!(),
                    Type::Function(type_) => {
                        self.context.add_error(
                            expression.span.clone(),
                            format!("Can't switch over value of function type `{type_}`"),
                        );
                        return LoweredExpression::Error;
                    }
                    Type::Error => return LoweredExpression::Error,
                };

                let mut variant_names = FxHashSet::default();
                let mut return_type = context_type.cloned();
                let cases = cases
                    .iter()
                    .filter_map(|case| try {
                        let variant = case.variant.value()?.clone();
                        let Some((_, value_type)) =
                            variants.iter().find(|(it, _)| it == &variant.string)
                        else {
                            self.context.add_error(
                                variant.span.clone(),
                                format!("Unknown variant in switch: `{}`", *variant),
                            );
                            return None;
                        };

                        if !variant_names.insert(variant.clone()) {
                            self.context.add_error(
                                variant.span.clone(),
                                format!("Duplicate variant in switch: `{}`", *variant),
                            );
                            return None;
                        }

                        let variant_value = match (value_type, &case.value_name) {
                            (None, None) => None,
                            (None, Some(_)) => {
                                self.context.add_error(
                                    variant.span.clone(),
                                    format!(
                                        "Switch case specifies value name for variant `{}` that doesn't have any value",
                                        *variant
                                    ),
                                );
                                return None;
                            },
                            (Some(_), None) => {
                                self.context.add_error(
                                    variant.span.clone(),
                                    format!(
                                        "Switch case is missing a value name for variant `{}`",
                                        *variant
                                    ),
                                );
                                return None;
                            },
                            (Some(value_type), Some((value_name, _))) => {
                                let value_name = value_name.value()?;
                                Some((value_type.substitute(&environment), value_name.string.clone()))
                            },
                        };

                        let mut value_id = None;
                        let body = self.build_inner(|builder| {
                            value_id = variant_value.map(|(type_,name)| {
                                let id = builder.context.id_generator.generate();
                                builder.push_parameter(Parameter { id, name, type_ });
                                id
                            });
                            if let Some(expression) = case.expression.value() {
                                let (_, new_return_type) = builder.lower_expression(expression, return_type.as_ref());
                                if return_type.is_none() {
                                    return_type = Some(new_return_type);
                                }
                            }
                        });
                        SwitchCase {
                            variant: variant.string,
                            value_id,
                            body,
                        }
                    })
                    .collect();

                // TODO: check for missing variants

                self.push_lowered(
                    None,
                    ExpressionKind::Switch {
                        value,
                        enum_,
                        cases,
                    },
                    return_type.unwrap_or_else(|| NamedType::never().into()),
                )
            }
        }
    }
    fn lower_identifier(&mut self, identifier: &AstString) -> LoweredExpression {
        let name = &identifier.string;
        if let Some((id, type_)) = self.lookup_local_identifier(identifier) {
            self.push_lowered(name.clone(), ExpressionKind::Reference(id), type_.clone())
        } else if let Some(named) = self.context.global_identifiers.get(name) {
            match named {
                Named::Assignment(id) => {
                    let id = *id;
                    self.global_assignment_dependencies.insert(id);
                    let type_ = self.context.assignments.get(&id).unwrap().type_.clone();
                    self.push_lowered(name.clone(), ExpressionKind::Reference(id), type_)
                }
                Named::Functions(_) => {
                    todo!("support function references");
                }
            }
        } else if let Some(type_parameter) = self.type_parameters.iter().find(|it| it.name == *name)
        {
            LoweredExpression::TypeParameterReference(type_parameter.type_())
        } else if self.context.hir.type_declarations.contains_key(name) {
            LoweredExpression::NamedTypeReference(name.clone())
        } else {
            self.context.add_error(
                identifier.span.clone(),
                format!("Unknown reference: {name}"),
            );
            LoweredExpression::Error
        }
    }
    fn lower_call(
        &mut self,
        name: &AstString,
        type_arguments: Option<&[Type]>,
        arguments: &[(Id, Type)],
    ) -> LoweredExpression {
        let argument_types = arguments
            .iter()
            .map(|(_, type_)| type_.clone())
            .collect::<Box<_>>();

        if let Some((_, id, type_)) = self
            .local_identifiers
            .iter()
            .find(|(it, _, _)| it == &name.string)
        {
            // Local lambda call
            let Type::Function(type_) = type_ else {
                self.context.add_error(
                    name.span.clone(),
                    format!("`{}` is not a function", name.string),
                );
                return LoweredExpression::Error;
            };
            let id = *id;
            let type_ = type_.clone();

            let result = self.match_signature(
                None,
                &[],
                &type_.parameter_types,
                type_arguments,
                &argument_types,
            );
            return match result {
                Ok(substitutions) => self.push_lowered(
                    None,
                    ExpressionKind::Call {
                        function: id,
                        substitutions,
                        arguments: arguments.iter().map(|(id, _)| *id).collect(),
                    },
                    *type_.return_type,
                ),
                Err(error) => {
                    self.context.add_error(
                        name.span.clone(),
                        match error {
                            CallLikeLoweringError::TypeArgumentCount => {
                                "Wrong number of type arguments".to_string()
                            }
                            CallLikeLoweringError::ArgumentCount => {
                                "Wrong number of arguments".to_string()
                            }
                            CallLikeLoweringError::Unification(Some(error)) => error.into_string(),
                            CallLikeLoweringError::Unification(None) => {
                                "Mismatching types".to_string()
                            }
                            CallLikeLoweringError::FunctionReachableViaMultipleImpls => {
                                "Function is reachable via multiple impls".to_string()
                            }
                            // TODO: more specific error message
                            CallLikeLoweringError::TypeArgumentMismatch => {
                                "Type arguments are not assignable".to_string()
                            }
                        },
                    );
                    LoweredExpression::Error
                }
            };
        }

        let matches = self
            .context
            .get_all_functions_matching_name(&name.string)
            .into_iter()
            .map(|id| {
                let (function, trait_) = self.context.get_function(id);
                (id, function.clone(), trait_.cloned())
            })
            .collect_vec();

        if matches.is_empty() {
            self.context.add_error(
                name.span.clone(),
                format!("Function `{}` not found", name.string),
            );
            return LoweredExpression::Error;
        }

        if argument_types.iter().any(ContainsError::contains_error) {
            return LoweredExpression::Error;
        }

        let (mut matches, mismatches): (Vec<_>, Vec<_>) = matches
            .into_iter()
            .map(|(id, function, trait_)| {
                let result = self.match_signature(
                    trait_
                        .as_ref()
                        .map(|it| (&it.solver_goal, it.solver_subgoals.as_ref())),
                    &function.signature.type_parameters,
                    &function
                        .signature
                        .parameters
                        .iter()
                        .map(|it| it.type_.clone())
                        .collect::<Box<_>>(),
                    type_arguments,
                    &argument_types,
                );
                match result {
                    Ok(substitutions) => Ok((id, function, substitutions)),
                    Err(error) => Err((function, error)),
                }
            })
            .partition_result();

        if matches.is_empty() {
            self.context.add_error(
                name.span.clone(),
                format!(
                    "No matching function found for:\n  {}\n{}:{}",
                    FunctionDeclaration::call_signature_to_string(&name.string, &argument_types),
                    if mismatches.len() == 1 {
                        "This is the candidate function"
                    } else {
                        "These are candidate functions"
                    },
                    mismatches
                        .into_iter()
                        .map(|(function, error)| format!(
                            "\n• {}: {}",
                            function.signature_to_string(),
                            match error {
                                CallLikeLoweringError::TypeArgumentCount =>
                                    "Wrong number of type arguments".to_string(),
                                CallLikeLoweringError::ArgumentCount =>
                                    "Wrong number of arguments".to_string(),
                                CallLikeLoweringError::Unification(Some(error)) =>
                                    error.into_string(),
                                CallLikeLoweringError::Unification(None) =>
                                    "Mismatching types".to_string(),
                                CallLikeLoweringError::FunctionReachableViaMultipleImpls =>
                                    "Function is reachable via multiple impls".to_string(),
                                // TODO: more specific error message
                                CallLikeLoweringError::TypeArgumentMismatch =>
                                    "Type arguments are not assignable".to_string(),
                            },
                        ))
                        .join(""),
                ),
            );
            return LoweredExpression::Error;
        } else if matches.len() > 1 {
            self.context.add_error(
                name.span.clone(),
                format!(
                    "Multiple matching function found for:\n  {}\nThese are candidate functions:{}",
                    FunctionDeclaration::call_signature_to_string(&name.string, &argument_types),
                    matches
                        .iter()
                        .map(|(_, function, _)| format!("\n• {}", function.signature_to_string()))
                        .join(""),
                ),
            );
            return LoweredExpression::Error;
        }

        let (id, function, substitutions) = matches.pop().unwrap();
        let return_type = function.signature.return_type.substitute(&substitutions);
        self.push_lowered(
            None,
            ExpressionKind::Call {
                function: id,
                substitutions,
                arguments: arguments.iter().map(|(id, _)| *id).collect(),
            },
            return_type,
        )
    }
    fn lower_struct_creation(
        &mut self,
        span: Range<Offset>,
        call: &'a AstCall,
        type_arguments: Option<&[Type]>,
        type_: &str,
        type_parameters: &[TypeParameter],
        fields: Option<&[StructField]>,
    ) -> LoweredExpression {
        let Some(fields) = fields else {
            self.context.add_error(
                call.receiver.span.clone(),
                format!("Can't instantiate builtin type {type_} directly"),
            );
            return LoweredExpression::Error;
        };

        let arguments = self.lower_arguments(&call.arguments);

        let result = self.match_signature(
            None,
            type_parameters,
            &fields.iter().map(|it| it.type_.clone()).collect::<Box<_>>(),
            type_arguments,
            &arguments
                .iter()
                .map(|(_, type_)| type_.clone())
                .collect::<Box<_>>(),
        );
        let substitutions = match result {
            Ok(substitutions) => substitutions,
            Err(error) => {
                self.context.add_error(
                    span,
                    format!(
                        "Invalid struct creation: {}",
                        match error {
                            CallLikeLoweringError::TypeArgumentCount =>
                                "Wrong number of type arguments".to_string(),
                            CallLikeLoweringError::ArgumentCount =>
                                "Wrong number of fields".to_string(),
                            CallLikeLoweringError::Unification(Some(error)) => error.into_string(),
                            CallLikeLoweringError::Unification(None) =>
                                "Mismatching types".to_string(),
                            CallLikeLoweringError::FunctionReachableViaMultipleImpls =>
                                unreachable!(),
                            // TODO: more specific error message
                            CallLikeLoweringError::TypeArgumentMismatch =>
                                "Type arguments are not assignable".to_string(),
                        },
                    ),
                );
                return LoweredExpression::Error;
            }
        };

        let struct_type = NamedType::new(
            type_,
            type_parameters
                .iter()
                .map(|it| substitutions[&it.type_()].clone())
                .collect_vec(),
        );
        self.push_lowered(
            None,
            ExpressionKind::CreateStruct {
                struct_: struct_type.clone(),
                fields: arguments.iter().map(|(id, _)| *id).collect(),
            },
            struct_type,
        )
    }
    fn lower_enum_creation(
        &mut self,
        call: &'a AstCall,
        type_arguments: Option<&[Type]>,
        type_: &str,
        variant: &AstString,
        type_parameters: &[TypeParameter],
        variants: &[(Box<str>, Option<Type>)],
    ) -> LoweredExpression {
        let Some((_, variant_type)) = variants.iter().find(|(name, _)| name == &variant.string)
        else {
            self.context.add_error(
                variant.span.clone(),
                format!(
                    "Enum `{type_:?}` doesn't have a variant `{}`",
                    variant.string,
                ),
            );
            return LoweredExpression::Error;
        };

        let parameter_types = variant_type
            .as_ref()
            .map(|variant_type| vec![variant_type.clone()].into_boxed_slice())
            .unwrap_or_default();
        let arguments = self.lower_arguments(&call.arguments);

        let result = self.match_signature(
            None,
            type_parameters,
            parameter_types.as_ref(),
            type_arguments,
            &arguments
                .iter()
                .map(|(_, type_)| type_.clone())
                .collect::<Box<_>>(),
        );
        let substitutions = match result {
            Ok(substitutions) => substitutions,
            Err(error) => {
                self.context.add_error(
                    variant.span.clone(),
                    format!(
                        "Invalid enum variant creation: {}",
                        match error {
                            CallLikeLoweringError::TypeArgumentCount =>
                                "Wrong number of type arguments".to_string(),
                            CallLikeLoweringError::ArgumentCount =>
                                "Wrong number of arguments".to_string(),
                            CallLikeLoweringError::Unification(Some(error)) => error.into_string(),
                            CallLikeLoweringError::Unification(None) =>
                                "Mismatching types".to_string(),
                            CallLikeLoweringError::FunctionReachableViaMultipleImpls =>
                                unreachable!(),
                            // TODO: more specific error message
                            CallLikeLoweringError::TypeArgumentMismatch =>
                                "Type arguments are not assignable".to_string(),
                        },
                    ),
                );
                return LoweredExpression::Error;
            }
        };

        let enum_type = NamedType::new(
            type_,
            type_parameters
                .iter()
                .map(|it| substitutions[&it.type_()].clone())
                .collect_vec(),
        );
        self.push_lowered(
            None,
            ExpressionKind::CreateEnum {
                enum_: enum_type.clone(),
                variant: variant.string.clone(),
                value: arguments.first().map(|(id, _)| *id),
            },
            enum_type,
        )
    }
    fn lower_arguments(&mut self, arguments: &'a AstResult<AstArguments>) -> Box<[(Id, Type)]> {
        arguments
            .arguments_or_default()
            .iter()
            .map(|argument| self.lower_expression(&argument.value, None))
            .collect::<Box<_>>()
    }
    fn match_signature(
        &self,
        trait_goal_and_subgoals: Option<(&SolverGoal, &[SolverGoal])>,
        type_parameters: &[TypeParameter],
        parameter_types: &[Type],
        type_arguments: Option<&[Type]>,
        argument_types: &[Type],
    ) -> Result<FxHashMap<ParameterType, Type>, CallLikeLoweringError> {
        // Check type argument count
        if let Some(type_arguments) = type_arguments
            && type_arguments.len() != type_parameters.len()
        {
            return Err(CallLikeLoweringError::TypeArgumentCount);
        }

        // Check argument count
        if argument_types.len() != parameter_types.len() {
            return Err(CallLikeLoweringError::ArgumentCount);
        }

        // Check argument types
        let substitutions = {
            let mut unifier = TypeUnifier::new(type_parameters);
            // Type arguments
            if let Some(type_arguments) = type_arguments {
                for (type_argument, type_parameter) in
                    type_arguments.iter().zip_eq(type_parameters.iter())
                {
                    match unifier.unify(type_argument, &type_parameter.type_().into()) {
                        Ok(true) => {}
                        Ok(false) => unreachable!(),
                        Err(reason) => {
                            return Err(CallLikeLoweringError::Unification(Some(reason)))
                        }
                    }
                }
            }

            // Arguments
            for (argument_type, parameter_type) in
                argument_types.iter().zip_eq(parameter_types.iter())
            {
                match unifier.unify(argument_type, parameter_type) {
                    Ok(true) => {}
                    Ok(false) => return Err(CallLikeLoweringError::Unification(None)),
                    Err(reason) => return Err(CallLikeLoweringError::Unification(Some(reason))),
                }
            }

            match unifier.finish() {
                Ok(substitutions) => substitutions,
                Err(error) => return Err(CallLikeLoweringError::Unification(Some(error))),
            }
        };

        // Solve traits
        let self_goal = substitutions
            .get(&ParameterType::self_type())
            .map(|self_type| {
                trait_goal_and_subgoals
                    .unwrap()
                    .0
                    .substitute_all(&FxHashMap::from_iter([(
                        SolverVariable::self_(),
                        self_type.clone().try_into().unwrap(),
                    )]))
            });

        let type_parameter_subgoals = type_parameters
            .iter()
            .filter_map(|it| it.clone().try_into().ok())
            .collect_vec();

        let solver_substitutions = substitutions
            .iter()
            .filter_map(|(parameter_type, type_)| try {
                (
                    SolverVariable::new(parameter_type.clone()),
                    type_.clone().try_into().ok()?,
                )
            })
            .collect();

        let error = self_goal
            .iter()
            .chain(trait_goal_and_subgoals.iter().flat_map(|it| it.1.iter()))
            .chain(type_parameter_subgoals.iter())
            .find_map(|subgoal| {
                let solution = self.context.environment.solve(
                    &subgoal.substitute_all(&solver_substitutions),
                    &self
                        .type_parameters
                        .iter()
                        .filter_map(|it| it.clone().try_into().ok())
                        .collect::<Box<[SolverGoal]>>(),
                );
                match solution {
                    SolverSolution::Unique(_) => None,
                    SolverSolution::Ambiguous => {
                        Some(CallLikeLoweringError::FunctionReachableViaMultipleImpls)
                    }
                    SolverSolution::Impossible => Some(CallLikeLoweringError::TypeArgumentMismatch),
                }
            });
        if let Some(error) = error {
            return Err(error);
        }

        Ok(substitutions)
    }

    fn push_panic(&mut self, message: impl Into<Box<str>>) {
        let message = self.push(
            None,
            ExpressionKind::Text(message.into()),
            NamedType::text(),
        );
        self.push(
            None,
            ExpressionKind::Call {
                function: BuiltinFunction::Panic.id(),
                substitutions: FxHashMap::default(),
                arguments: vec![message].into_boxed_slice(),
            },
            NamedType::never(),
        );
    }

    fn push_lowered(
        &mut self,
        name: impl Into<Option<Box<str>>>,
        kind: ExpressionKind,
        type_: impl Into<Type>,
    ) -> LoweredExpression {
        let type_ = type_.into();
        let id = self.push(name, kind, type_.clone());
        LoweredExpression::Expression { id, type_ }
    }
    fn push_nothing(&mut self) -> Id {
        self.push(
            None,
            ExpressionKind::CreateStruct {
                struct_: NamedType::nothing(),
                fields: [].into(),
            },
            Type::nothing(),
        )
    }
    fn push_parameter(&mut self, parameter: Parameter) {
        self.local_identifiers
            .push((parameter.name, parameter.id, parameter.type_));
    }
    fn push_error(&mut self) -> Id {
        self.push(None, ExpressionKind::Error, Type::Error)
    }
    fn push(
        &mut self,
        name: impl Into<Option<Box<str>>>,
        kind: ExpressionKind,
        type_: impl Into<Type>,
    ) -> Id {
        let name = name.into();
        let type_ = type_.into();
        let id = self.context.id_generator.generate();
        if let Some(name) = &name {
            self.local_identifiers
                .push(((*name).clone(), id, type_.clone()));
        }
        self.body
            .expressions
            .push((id, name, Expression { kind, type_ }));
        id
    }

    #[must_use]
    fn lookup_local_identifier(&self, name: &str) -> Option<(Id, &Type)> {
        self.local_identifiers
            .iter()
            .rev()
            .find(|(box variable_name, _, _)| variable_name == name)
            .map(|(_, id, type_)| (*id, type_))
    }
}

#[derive(Debug)]
enum LoweredExpression {
    Expression { id: Id, type_: Type },
    NamedTypeReference(Box<str>),
    TypeParameterReference(ParameterType),
    Error,
}
// #[derive(Debug)]
// enum LoweredIdentifier {
//     Expression { id: Id, type_: Type },
//     FunctionReferences(Box<[Id]>),
//     NamedTypeReference(Box<str>),
//     TypeParameterReference { name: Box<str>, id: TypeParameterId },
//     Error,
// }

enum CallLikeLoweringError {
    TypeArgumentCount,
    ArgumentCount,
    Unification(Option<Box<str>>),
    FunctionReachableViaMultipleImpls,
    TypeArgumentMismatch,
}

pub struct TypeUnifier<'h> {
    type_parameters: &'h [TypeParameter],
    substitutions: FxHashMap<ParameterType, Type>,
}
impl<'h> TypeUnifier<'h> {
    #[must_use]
    pub fn new(type_parameters: &'h [TypeParameter]) -> Self {
        Self {
            type_parameters,
            substitutions: FxHashMap::default(),
        }
    }

    pub fn unify(&mut self, argument: &Type, parameter: &Type) -> Result<bool, Box<str>> {
        match (argument, parameter) {
            (Type::Error, _) | (_, Type::Error) => Ok(true),
            (_, Type::Parameter(parameter)) => {
                match self.substitutions.entry(parameter.clone()) {
                    Entry::Occupied(entry) => {
                        if !entry.get().equals_lenient(argument) {
                            // TODO: show all mismatches, not only the first
                            return Err(format!("Type parameter {} gets resolved to different types: `{}` and `{argument}`", parameter.name,entry.get()).into_boxed_str());
                        }
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(argument.clone());
                    }
                }
                Ok(true)
            }
            (Type::Named(argument), Type::Named(parameter)) => {
                // TODO: change `Ok(false)` to and `Err`
                if argument.name != parameter.name
                    || argument.type_arguments.len() != parameter.type_arguments.len()
                {
                    return Ok(false);
                }

                for (argument, parameter) in argument
                    .type_arguments
                    .iter()
                    .zip_eq(parameter.type_arguments.iter())
                {
                    let result = self.unify(argument, parameter)?;
                    if !result {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Type::Parameter { .. }, Type::Named { .. }) => Ok(true),
            (Type::Self_ { base_type }, _) => self.unify(base_type, parameter),
            (Type::Function(argument), Type::Function(parameter)) => {
                if argument.parameter_types.len() != parameter.parameter_types.len() {
                    return Ok(false);
                }
                for (argument, parameter) in argument
                    .parameter_types
                    .iter()
                    .zip_eq(parameter.parameter_types.iter())
                {
                    if !self.unify(argument, parameter)? {
                        return Ok(false);
                    }
                }

                self.unify(&argument.return_type, &parameter.return_type)
            }
            (Type::Function(_), _) | (_, Type::Function(_)) => Ok(false),
            (_, Type::Self_ { base_type: _ }) => {
                self.unify(argument, &ParameterType::self_type().into())
            }
        }
    }

    pub fn finish(self) -> Result<FxHashMap<ParameterType, Type>, Box<str>> {
        for type_parameter in self.type_parameters {
            let type_ = type_parameter.type_();
            if !self.substitutions.contains_key(&type_) {
                return Err(format!(
                    "The type parameter `{type_}` can't be resolved to a specific type.",
                )
                .into_boxed_str());
            }
        }
        Ok(self.substitutions)
    }
}
