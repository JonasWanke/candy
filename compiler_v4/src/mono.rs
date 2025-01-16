use crate::{
    hir::BuiltinFunction,
    impl_countable_id,
    to_text::{TextBuilder, ToText},
};
use derive_more::Deref;
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};
use std::fmt::{self, Display, Formatter};

#[derive(Clone, Copy, Debug, Default, Deref, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(usize);
impl_countable_id!(Id);
impl Display for Id {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "_{}", self.0)
    }
}
impl ToText for Id {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("${self}"));
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Mono {
    pub type_declarations: FxHashMap<Box<str>, TypeDeclaration>,
    pub assignments: FxHashMap<Box<str>, Assignment>,
    pub assignment_initialization_order: Box<[Box<str>]>,
    pub functions: FxHashMap<Box<str>, Function>,
    pub main_function: Box<str>,
}
impl ToText for Mono {
    fn build_text(&self, builder: &mut TextBuilder) {
        for (name, declaration) in self
            .type_declarations
            .iter()
            .sorted_by_key(|(name, _)| *name)
        {
            builder.push("Type Declarations:");
            builder.push_newline();
            match declaration {
                TypeDeclaration::Builtin {
                    name,
                    type_arguments,
                } => {
                    builder.push(format!(
                        "builtin struct {name} = {name}{}",
                        if type_arguments.is_empty() {
                            String::new()
                        } else {
                            format!("[{}]", type_arguments.join(", "))
                        }
                    ));
                }
                TypeDeclaration::Struct { fields } => {
                    builder.push(format!("struct {name} {{"));
                    builder.push_children_custom_multiline(
                        fields.iter(),
                        |builder, (box name, box type_)| {
                            builder.push(format!("{name}: {type_},"));
                        },
                    );
                    if !fields.is_empty() {
                        builder.push_newline();
                    }
                    builder.push("}");
                }
                TypeDeclaration::Enum { variants } => {
                    builder.push(format!("enum {name} {{"));
                    builder.push_children_custom_multiline(variants.iter(), |builder, variant| {
                        builder.push(&variant.name);
                        if let Some(value_type) = &variant.value_type {
                            builder.push(format!(": {value_type}"));
                        }
                        builder.push(",");
                    });
                    if !variants.is_empty() {
                        builder.push_newline();
                    }
                    builder.push("}");
                }
                TypeDeclaration::Function {
                    parameter_types,
                    return_type,
                } => {
                    builder.push(format!(
                        "functionType {name} = ({}) {return_type}",
                        parameter_types.iter().join(", "),
                    ));
                }
            }
            builder.push_newline();
        }
        builder.push_newline();

        builder.push("Assignments (in initialization order):");
        builder.push_newline();
        for name in &self.assignment_initialization_order {
            (&**name, &self.assignments[name]).build_text(builder);
            builder.push_newline();
        }
        builder.push_newline();

        builder.push("Functions:");
        builder.push_newline();
        for (name, function) in self
            .functions
            .iter()
            .sorted_by_key(|(name, _)| (**name).clone())
        {
            (&**name, function).build_text(builder);
            builder.push_newline();
        }
        builder.push_newline();

        builder.push(format!("Main Function: {}", self.main_function));
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TypeDeclaration {
    Builtin {
        name: Box<str>,
        type_arguments: Box<[Box<str>]>,
    },
    Struct {
        fields: Box<[(Box<str>, Box<str>)]>,
    },
    Enum {
        variants: Box<[EnumVariant]>,
    },
    Function {
        parameter_types: Box<[Box<str>]>,
        return_type: Box<str>,
    },
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EnumVariant {
    pub name: Box<str>,
    pub value_type: Option<Box<str>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Assignment {
    pub type_: Box<str>,
    pub body: Body,
}
impl ToText for (&str, &Assignment) {
    fn build_text(&self, builder: &mut TextBuilder) {
        let (name, Assignment { type_, body }) = self;
        builder.push(format!("let {name}: {type_} = "));
        body.build_text(builder);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Function {
    pub parameters: Box<[Parameter]>,
    pub return_type: Box<str>,
    pub body: BodyOrBuiltin,
}
impl ToText for (&str, &Function) {
    fn build_text(&self, builder: &mut TextBuilder) {
        let (
            name,
            Function {
                parameters,
                return_type,
                body,
            },
        ) = self;
        builder.push(format!("fun {name}("));
        builder.push_children(parameters.iter(), ", ");
        builder.push(format!(") {return_type} "));
        match body {
            BodyOrBuiltin::Body(body) => body.build_text(builder),
            BodyOrBuiltin::Builtin {
                builtin_function,
                substitutions,
            } => {
                builder.push(format!("= {builtin_function:?}"));
                if !substitutions.is_empty() {
                    builder.push("[");
                    builder.push_children_custom(
                        substitutions
                            .iter()
                            .sorted_by_key(|(type_parameter, _)| (**type_parameter).clone()),
                        |builder, (type_parameter, type_argument)| {
                            builder.push(format!("{type_parameter} = {type_argument}"));
                        },
                        ", ",
                    );
                    builder.push("]");
                }
            }
        }
    }
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Parameter {
    pub id: Id,
    pub name: Box<str>,
    pub type_: Box<str>,
}
impl ToText for Parameter {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("{} {}: {}", self.name, self.id, self.type_));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodyOrBuiltin {
    Body(Body),
    Builtin {
        builtin_function: BuiltinFunction,
        substitutions: FxHashMap<Box<str>, Box<str>>,
    },
}
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Body {
    pub expressions: Vec<(Id, Option<Box<str>>, Expression)>,
}
impl Body {
    #[must_use]
    pub fn return_value_id(&self) -> Id {
        self.expressions.last().unwrap().0
    }
    #[must_use]
    pub fn return_type(&self) -> &str {
        &self.expressions.last().unwrap().2.type_
    }

    fn collect_defined_and_referenced_ids(
        &self,
        defined_ids: &mut FxHashSet<Id>,
        referenced_ids: &mut FxHashSet<Id>,
    ) {
        for (id, _, expression) in &self.expressions {
            defined_ids.insert(*id);
            match &expression.kind {
                ExpressionKind::Int(_) | ExpressionKind::Text(_) => {}
                ExpressionKind::CreateStruct { fields, .. } => {
                    referenced_ids.extend(fields.iter());
                }
                ExpressionKind::StructAccess { struct_, .. } => {
                    referenced_ids.insert(*struct_);
                }
                ExpressionKind::CreateEnum { value, .. } => {
                    referenced_ids.extend(value.iter());
                }
                ExpressionKind::GlobalAssignmentReference(_) => {}
                ExpressionKind::LocalReference(referenced_id) => {
                    referenced_ids.insert(*referenced_id);
                }
                ExpressionKind::CallFunction { arguments, .. } => {
                    referenced_ids.extend(arguments.iter());
                }
                ExpressionKind::CallLambda {
                    lambda, arguments, ..
                } => {
                    referenced_ids.insert(*lambda);
                    referenced_ids.extend(arguments.iter());
                }
                ExpressionKind::Switch { value, cases, .. } => {
                    referenced_ids.insert(*value);
                    for case in &**cases {
                        if let Some((value_id, _)) = &case.value {
                            referenced_ids.insert(*value_id);
                        }
                        case.body
                            .collect_defined_and_referenced_ids(defined_ids, referenced_ids);
                    }
                }
                ExpressionKind::Lambda(Lambda { parameters, body }) => {
                    defined_ids.extend(parameters.iter().map(|it| it.id));
                    body.collect_defined_and_referenced_ids(defined_ids, referenced_ids);
                }
            }
        }
    }
    #[must_use]
    pub fn find_expression(&self, id: Id) -> Option<(&str, Option<&Expression>)> {
        self.expressions.iter().find_map(|(it_id, _, expression)| {
            if *it_id == id {
                return Some((expression.type_.as_ref(), Some(expression)));
            }

            match &expression.kind {
                ExpressionKind::Switch { cases, .. } => cases.iter().find_map(|it| {
                    if let Some((value_id, box value_type)) = &it.value
                        && *value_id == id
                    {
                        return Some((value_type, None));
                    }
                    it.body.find_expression(id)
                }),
                ExpressionKind::Lambda(Lambda { parameters, body }) => parameters
                    .iter()
                    .find(|it| it.id == id)
                    .map(|it| (it.type_.as_ref(), None))
                    .or_else(|| body.find_expression(id)),
                _ => None,
            }
        })
    }
}
impl ToText for Body {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push("{");
        builder.push_children_custom_multiline(
            self.expressions.iter(),
            |builder, (id, name, expression)| {
                id.build_text(builder);
                builder.push(format!(": {} = ", expression.type_));
                if let Some(name) = name {
                    builder.push(format!("{name} = "));
                }
                expression.kind.build_text(builder);
            },
        );
        builder.push_newline();
        builder.push("}");
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub type_: Box<str>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ExpressionKind {
    Int(i64),
    Text(Box<str>),
    CreateStruct {
        struct_: Box<str>,
        fields: Box<[Id]>,
    },
    StructAccess {
        struct_: Id,
        field: Box<str>,
    },
    CreateEnum {
        enum_: Box<str>,
        variant: Box<str>,
        value: Option<Id>,
    },
    GlobalAssignmentReference(Box<str>),
    LocalReference(Id),
    CallFunction {
        function: Box<str>,
        arguments: Box<[Id]>,
    },
    CallLambda {
        lambda: Id,
        arguments: Box<[Id]>,
    },
    Switch {
        value: Id,
        enum_: Box<str>,
        cases: Box<[SwitchCase]>,
    },
    Lambda(Lambda),
}
impl ToText for ExpressionKind {
    fn build_text(&self, builder: &mut TextBuilder) {
        match self {
            Self::Int(value) => builder.push(value.to_string()),
            Self::Text(value) => builder.push(format!("\"{value}\"")),
            Self::CreateStruct { struct_, fields } => {
                builder.push(format!("{struct_} {{"));
                builder.push_children(fields.iter(), ", ");
                builder.push("}");
            }
            Self::StructAccess { struct_, field } => {
                builder.push(format!("{struct_}.{field}"));
            }
            Self::CreateEnum {
                enum_,
                variant,
                value,
            } => {
                builder.push(format!("{enum_}.{variant}"));
                if let Some(value) = value {
                    builder.push(format!("({value})"));
                }
            }
            Self::GlobalAssignmentReference(name) => builder.push(name),
            Self::LocalReference(id) => id.build_text(builder),
            Self::CallFunction {
                function,
                arguments,
            } => {
                builder.push(function);
                builder.push("(");
                builder.push_children(arguments.iter(), ", ");
                builder.push(")");
            }
            Self::CallLambda { lambda, arguments } => {
                lambda.build_text(builder);
                builder.push("(");
                builder.push_children(arguments.iter(), ", ");
                builder.push(")");
            }
            Self::Switch {
                value,
                enum_,
                cases,
            } => {
                builder.push("switch ");
                value.build_text(builder);
                builder.push(format!(": {enum_} {{"));
                builder.push_children_multiline(cases.iter());
                if !cases.is_empty() {
                    builder.push_newline();
                }
                builder.push("}");
            }
            Self::Lambda(lambda) => lambda.build_text(builder),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SwitchCase {
    pub variant: Box<str>,
    pub value: Option<(Id, Box<str>)>,
    pub body: Body,
}
impl ToText for SwitchCase {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("{}", self.variant));
        if let Some((value_id, value_type)) = &self.value {
            builder.push(format!("({value_id}: {value_type})"));
        }
        builder.push(" => ");
        self.body.build_text(builder);
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Lambda {
    pub parameters: Box<[Parameter]>,
    pub body: Body,
}
impl Lambda {
    #[must_use]
    pub fn closure_with_types(
        &self,
        declaration_parameters: &[Parameter],
        declaration_body: &Body,
    ) -> FxHashMap<Id, Box<str>> {
        self.closure()
            .into_iter()
            .map(|id| {
                (
                    id,
                    declaration_parameters.iter().find(|it| it.id == id).map_or_else(
                        || {
                            declaration_body
                                .find_expression(id)
                                .unwrap_or_else(|| {
                                    panic!(
                                        "Couldn't find expression {id} in declaration body {declaration_body:?}"
                                    )
                                })
                                .0
                                .to_string()
                                .into_boxed_str()
                        },
                        |it| it.type_.clone(),
                    )
                )
            })
            .collect()
    }
    #[must_use]
    pub fn closure(&self) -> FxHashSet<Id> {
        let mut defined_ids = self.parameters.iter().map(|it| it.id).collect();
        let mut referenced_ids = FxHashSet::default();
        self.body
            .collect_defined_and_referenced_ids(&mut defined_ids, &mut referenced_ids);
        referenced_ids.retain(|id| !defined_ids.contains(id));
        referenced_ids
    }
}
impl ToText for Lambda {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push("(");
        builder.push_children(self.parameters.iter(), ", ");
        builder.push(") => ");
        self.body.build_text(builder);
    }
}
