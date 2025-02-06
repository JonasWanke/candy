pub use crate::hir::Result::{Err, Ok};
use crate::{
    id::CountableId,
    impl_countable_id,
    to_text::{TextBuilder, ToText},
    type_solver::goals::{Environment, SolverGoal},
};
use derive_more::{Deref, From};
use extension_trait::extension_trait;
use itertools::Itertools;
use rustc_hash::FxHashMap;
use std::fmt::{self, Display, Formatter};
use strum::VariantArray;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Result<T> {
    Ok(T),
    Err,
}
impl<T> Result<T> {
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Result<U> {
        match self {
            Self::Ok(value) => Result::Ok(f(value)),
            Self::Err => Result::Err,
        }
    }
    #[must_use]
    pub const fn as_ref(&self) -> Result<&T> {
        match *self {
            Self::Ok(ref value) => Result::Ok(value),
            Self::Err => Result::Err,
        }
    }
}
impl<T: Display> Display for Result<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok(value) => write!(f, "{value}"),
            Self::Err => write!(f, "<error>"),
        }
    }
}

// TODO: split assignment/function and expression IDs
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
pub struct Hir {
    pub solver_environment: Environment,
    pub type_declarations: FxHashMap<Box<str>, TypeDeclaration>,
    pub traits: FxHashMap<Box<str>, TraitDefinition>,
    pub impls: Box<[Impl]>,
    pub assignments: FxHashMap<Id, Assignment>,
    pub functions: FxHashMap<Id, Function>,
    pub main_function_id: Id,
}
impl ToText for Hir {
    fn build_text(&self, builder: &mut TextBuilder) {
        for (name, declaration) in self
            .type_declarations
            .iter()
            .sorted_by_key(|(name, _)| *name)
        {
            match &declaration.kind {
                TypeDeclarationKind::Struct { fields } => {
                    builder.push(format!("struct {name}"));
                    declaration.type_parameters.build_text(builder);
                    if let Some(fields) = fields {
                        builder.push(" {");
                        builder.push_children_custom_multiline(fields.iter(), |builder, it| {
                            builder.push(format!("{}: {},", it.name, it.type_));
                        });
                        if !fields.is_empty() {
                            builder.push_newline();
                        }
                        builder.push("}");
                    } else {
                        builder.push(" = builtin");
                    }
                }
                TypeDeclarationKind::Enum { variants } => {
                    builder.push(format!("enum {name}"));
                    declaration.type_parameters.build_text(builder);
                    builder.push(" {");
                    builder.push_children_custom_multiline(
                        variants.iter(),
                        |builder, (name, type_)| {
                            builder.push(name);
                            if let Some(type_) = type_ {
                                builder.push(format!(": {type_}"));
                            }
                            builder.push(",");
                        },
                    );
                    if !variants.is_empty() {
                        builder.push_newline();
                    }
                    builder.push("}");
                }
            }
            builder.push_newline();
        }
        for (name, definition) in &self.traits {
            (name.as_ref(), definition).build_text(builder);
            builder.push_newline();
        }
        for impl_ in &self.impls {
            impl_.build_text(builder);
            builder.push_newline();
        }

        for (id, assignment) in self
            .assignments
            .iter()
            .sorted_by_key(|(_, it)| it.name.clone())
        {
            (id, assignment).build_text(builder);
            builder.push_newline();
        }
        for (id, function) in self
            .functions
            .iter()
            .sorted_by_key(|(_, it)| it.signature.name.clone())
        {
            (id, function).build_text(builder);
            builder.push_newline();
        }

        builder.push(format!("main: ${}", self.main_function_id));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeDeclaration {
    pub type_parameters: Box<[TypeParameter]>,
    pub kind: TypeDeclarationKind,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeDeclarationKind {
    Struct {
        /// `None` if the struct is a builtin.
        fields: Option<Box<[StructField]>>,
    },
    Enum {
        variants: Box<[(Box<str>, Option<Type>)]>,
    },
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StructField {
    pub name: Box<str>,
    pub type_: Type,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraitDefinition {
    pub type_parameters: Box<[TypeParameter]>,
    pub solver_goal: SolverGoal,
    pub solver_subgoals: Box<[SolverGoal]>,
    pub functions: FxHashMap<Id, TraitFunction>,
}
impl ToText for (&str, &TraitDefinition) {
    fn build_text(&self, builder: &mut TextBuilder) {
        let (name, definition) = self;
        builder.push(format!("trait {name}"));
        definition.type_parameters.build_text(builder);
        builder.push(" {");
        builder.push_children_custom_multiline(
            definition
                .functions
                .iter()
                .sorted_by_key(|(_, it)| it.signature.name.clone()),
            |builder, (id, function)| (*id, *function).build_text(builder),
        );
        if !definition.functions.is_empty() {
            builder.push_newline();
        }
        builder.push("}");
    }
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Trait {
    pub name: Box<str>,
    pub type_arguments: Box<[Type]>,
}
impl Trait {
    #[must_use]
    pub fn substitute(&self, environment: &FxHashMap<ParameterType, Type>) -> Self {
        Self {
            name: self.name.clone(),
            type_arguments: self
                .type_arguments
                .iter()
                .map(|it| it.substitute(environment))
                .collect(),
        }
    }
}
impl Display for Trait {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        NamedType {
            name: self.name.clone(),
            type_arguments: self.type_arguments.clone(),
        }
        .fmt(f)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraitFunction {
    pub signature: FunctionSignature,
    pub body: Option<Body>,
}
impl ToText for (&Id, &TraitFunction) {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("{}@", self.0));
        self.1.signature.build_text(builder);
        if let Some(body) = &self.1.body {
            builder.push(" ");
            body.build_text(builder);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Impl {
    pub type_parameters: Box<[TypeParameter]>,
    pub type_: Type,
    pub trait_: Trait,

    /// The function IDs match the IDs of the parent function in the implemented
    /// trait.
    pub functions: FxHashMap<Id, Function>,
}
impl ToText for Impl {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push("impl");
        self.type_parameters.build_text(builder);
        builder.push(format!(" {}: {} {{", self.type_, self.trait_));
        builder.push_children_custom_multiline(
            self.functions
                .iter()
                .sorted_by_key(|(_, it)| it.signature.name.clone()),
            |builder, (id, function)| {
                (*id, *function).build_text(builder);
            },
        );
        if !self.functions.is_empty() {
            builder.push_newline();
        }
        builder.push("}");
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TypeParameter {
    pub name: Box<str>,
    pub upper_bound: Option<Result<Trait>>,
}
impl TypeParameter {
    #[must_use]
    pub fn type_(&self) -> ParameterType {
        ParameterType {
            name: self.name.clone(),
        }
    }
}
impl Display for TypeParameter {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}
#[extension_trait]
pub impl SliceOfTypeParameter for [TypeParameter] {
    #[must_use]
    fn type_(&self) -> Box<[Type]> {
        self.iter().map(|it| it.type_().into()).collect()
    }
}
impl ToText for [TypeParameter] {
    fn build_text(&self, builder: &mut TextBuilder) {
        if self.is_empty() {
            return;
        }

        builder.push("[");
        builder.push_children(self, ", ");
        builder.push("]");
    }
}
impl ToText for TypeParameter {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("{}", self.name));
        if let Some(upper_bound) = self.upper_bound.as_ref() {
            builder.push(format!(": {upper_bound}"));
        }
    }
}

// Type

#[derive(Clone, Debug, Eq, From, Hash, PartialEq)]
pub enum Type {
    // TODO: encode ADT, trait, or builtin type here
    #[from]
    Named(NamedType),
    // TODO: merge parameter and named types?
    #[from]
    Parameter(ParameterType),
    Self_ {
        base_type: Box<Type>,
    },
    #[from]
    Function(FunctionType),
    Error,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct NamedType {
    pub name: Box<str>,
    pub type_arguments: Box<[Type]>,
}
impl NamedType {
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, type_arguments: impl Into<Box<[Type]>>) -> Self {
        Self {
            name: name.into(),
            type_arguments: type_arguments.into(),
        }
    }

    // Builtin types
    #[must_use]
    pub fn int() -> Self {
        Self::new("Int", [])
    }
    #[must_use]
    pub fn list(t: impl Into<Type>) -> Self {
        Self::new("List", [t.into()])
    }
    #[must_use]
    pub fn text() -> Self {
        Self::new("Text", [])
    }

    // Standard library types
    #[must_use]
    pub fn maybe(t: impl Into<Type>) -> Self {
        Self::new("Maybe", [t.into()])
    }
    #[must_use]
    pub fn never() -> Self {
        Self::new("Never", [])
    }
    #[must_use]
    pub fn nothing() -> Self {
        Self::new("Nothing", [])
    }
    #[must_use]
    pub fn ordering() -> Self {
        Self::new("Ordering", [])
    }
    #[must_use]
    pub fn result(t: impl Into<Type>, e: impl Into<Type>) -> Self {
        Self::new("Result", [t.into(), e.into()])
    }
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ParameterType {
    pub name: Box<str>,
}
impl ParameterType {
    #[must_use]
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self { name: name.into() }
    }

    const SELF_TYPE_NAME: &'static str = "Self";

    #[must_use]
    pub fn self_type() -> Self {
        Self {
            name: Self::SELF_TYPE_NAME.into(),
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn is_self_type(&self) -> bool {
        self.name.as_ref() == Self::SELF_TYPE_NAME
    }
}
impl Display for ParameterType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FunctionType {
    pub parameter_types: Box<[Type]>,
    pub return_type: Box<Type>,
}
impl FunctionType {
    #[must_use]
    pub fn new(parameter_types: impl Into<Box<[Type]>>, return_type: impl Into<Type>) -> Self {
        Self {
            parameter_types: parameter_types.into(),
            return_type: Box::new(return_type.into()),
        }
    }
}
impl Display for FunctionType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}) {}",
            self.parameter_types.iter().join(", "),
            self.return_type,
        )
    }
}
impl Type {
    pub fn nothing() -> Self {
        NamedType::nothing().into()
    }

    #[must_use]
    pub fn build_environment(
        type_parameters: &[TypeParameter],
        type_arguments: &[Self],
    ) -> FxHashMap<ParameterType, Self> {
        type_parameters
            .iter()
            .map(TypeParameter::type_)
            .zip_eq(type_arguments.iter().cloned())
            .collect()
    }
    #[must_use]
    pub fn substitute(&self, environment: &FxHashMap<ParameterType, Self>) -> Self {
        match self {
            Self::Named(NamedType { name, type_arguments }) => Self::Named(NamedType {
                name: name.clone(),
                type_arguments: type_arguments.iter().map(|it| it.substitute(environment)).collect(),
            }),
            Self::Parameter (type_) => environment.get(type_).unwrap_or_else(|| panic!("Missing substitution for type parameter {type_} (environment: {environment:?})")).clone(),
            Self::Self_ { base_type } => environment.get(&ParameterType::self_type()).cloned().unwrap_or_else(|| Self::Self_ { base_type: base_type.clone() }),
            Self::Function(FunctionType{parameter_types, return_type }) => Self::Function(FunctionType::new(
                parameter_types.iter().map(|it| it.substitute(environment)).collect_vec(),
                return_type.substitute(environment),
            )),
            Self::Error => Self::Error,
        }
    }

    #[must_use]
    pub fn equals_lenient(&self, other: &Self) -> bool {
        #[allow(clippy::redundant_guards)]
        match (self, other) {
            (Self::Error, _) | (_, Self::Error) => true,
            (Self::Named(NamedType { box name, .. }), _) if name == "Never" => true,
            (Self::Named(from), Self::Named(to)) => {
                from.name == to.name
                    && from
                        .type_arguments
                        .iter()
                        .zip_eq(to.type_arguments.iter())
                        .all(|(this, other)| this.equals_lenient(other))
            }
            (Self::Parameter(from), Self::Parameter(to)) => from == to,
            (Self::Self_ { base_type: from }, Self::Self_ { base_type: to }) => from == to,
            _ => false,
        }
    }
}
impl Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self {
            Self::Named(type_) => write!(f, "{type_}"),
            Self::Parameter(ParameterType { name }) => write!(f, "{name}"),
            Self::Self_ { base_type } => write!(f, "Self<{base_type}>"),
            Self::Function(type_) => write!(f, "{type_}"),
            Self::Error => write!(f, "<error>"),
        }
    }
}
impl Display for NamedType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if !self.type_arguments.is_empty() {
            write!(f, "[{}]", self.type_arguments.iter().join(", "))?;
        }
        std::result::Result::Ok(())
    }
}

pub trait ContainsError {
    fn contains_error(&self) -> bool;
}
impl ContainsError for Type {
    fn contains_error(&self) -> bool {
        match self {
            Self::Named(named_type) => named_type.contains_error(),
            Self::Parameter(_) => false,
            Self::Self_ { base_type } => base_type.contains_error(),
            Self::Function(function_type) => function_type.contains_error(),
            Self::Error => true,
        }
    }
}
impl ContainsError for NamedType {
    fn contains_error(&self) -> bool {
        self.type_arguments
            .iter()
            .any(ContainsError::contains_error)
    }
}
impl ContainsError for FunctionType {
    fn contains_error(&self) -> bool {
        self.parameter_types
            .iter()
            .any(ContainsError::contains_error)
            || self.return_type.contains_error()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Assignment {
    pub name: Box<str>,
    pub type_: Type,
    pub body: Body,
}
impl ToText for (&Id, &Assignment) {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("{}@{}: {} = ", self.0, self.1.name, self.1.type_));
        self.1.body.build_text(builder);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Function {
    pub signature: FunctionSignature,
    pub body: BodyOrBuiltin,
}
impl ToText for (&Id, &Function) {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("{}@", self.0));
        self.1.signature.build_text(builder);
        builder.push(" ");
        match &self.1.body {
            BodyOrBuiltin::Body(body) => body.build_text(builder),
            BodyOrBuiltin::Builtin(builtin_function) => {
                builder.push(format!("= {builtin_function:?}"));
            }
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionSignature {
    pub name: Box<str>,
    pub type_parameters: Box<[TypeParameter]>,
    pub parameters: Box<[Parameter]>,
    pub return_type: Type,
}
impl ToText for (&Id, &FunctionSignature) {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("{}@", self.0));
        self.1.build_text(builder);
    }
}
impl ToText for FunctionSignature {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("{}", self.name));
        self.type_parameters.build_text(builder);
        builder.push("(");
        builder.push_children_custom(
            self.parameters.iter(),
            |builder, parameter| {
                builder.push(format!("{}: {}", parameter.name, parameter.type_));
            },
            ", ",
        );
        builder.push(format!(") {}", self.return_type));
    }
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Parameter {
    pub id: Id,
    pub name: Box<str>,
    pub type_: Type,
}
impl ToText for Parameter {
    fn build_text(&self, builder: &mut TextBuilder) {
        self.id.build_text(builder);
        builder.push(format!(": {} = {}", self.type_, self.name));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodyOrBuiltin {
    Body(Body),
    Builtin(BuiltinFunction),
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Body {
    pub expressions: Vec<(Id, Option<Box<str>>, Expression)>,
}
impl Body {
    #[allow(dead_code)]
    #[must_use]
    pub fn return_value_id(&self) -> Id {
        self.expressions.last().unwrap().0
    }
    #[must_use]
    pub fn return_type(&self) -> &Type {
        &self.expressions.last().unwrap().2.type_
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub type_: Type,
}
impl Expression {
    #[allow(dead_code)]
    #[must_use]
    pub fn nothing() -> Self {
        Self {
            kind: ExpressionKind::CreateStruct {
                struct_: NamedType::nothing(),
                fields: [].into(),
            },
            type_: Type::nothing(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpressionKind {
    Int(i64),
    Text(Box<str>),
    CreateStruct {
        struct_: NamedType,
        fields: Box<[Id]>,
    },
    StructAccess {
        struct_: Id,
        field: Box<str>,
    },
    CreateEnum {
        enum_: NamedType,
        variant: Box<str>,
        value: Option<Id>,
    },
    Reference(Id),
    Call {
        function: Id,
        substitutions: FxHashMap<ParameterType, Type>,
        arguments: Box<[Id]>,
    },
    Switch {
        value: Id,
        enum_: Type,
        cases: Box<[SwitchCase]>,
    },
    Lambda {
        parameters: Box<[Parameter]>,
        body: Body,
    },
    Error,
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
            Self::Reference(id) => id.build_text(builder),
            Self::Call {
                function,
                substitutions,
                arguments,
            } => {
                function.build_text(builder);
                if !substitutions.is_empty() {
                    builder.push("[");
                    builder.push_children_custom(
                        substitutions.iter(),
                        |builder, (type_parameter, type_argument)| {
                            builder.push(format!("{type_parameter} = {type_argument}"));
                        },
                        ", ",
                    );
                    builder.push("]");
                }
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
            Self::Lambda { parameters, body } => {
                builder.push("(");
                builder.push_children(parameters.iter(), ", ");
                builder.push(") => ");
                body.build_text(builder);
            }
            Self::Error => builder.push("<error>"),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchCase {
    pub variant: Box<str>,
    pub value_id: Option<Id>,
    pub body: Body,
}
impl ToText for SwitchCase {
    fn build_text(&self, builder: &mut TextBuilder) {
        builder.push(format!("{}", self.variant));
        if let Some(value_id) = self.value_id {
            builder.push(format!("({value_id})"));
        }
        builder.push(" => ");
        self.body.build_text(builder);
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, VariantArray)]
#[strum(serialize_all = "camelCase")]
pub enum BuiltinFunction {
    IntAdd,
    IntBitwiseAnd,
    IntBitwiseOr,
    IntBitwiseXor,
    IntCompareTo,
    IntDivideTruncating,
    IntMultiply,
    IntParse,
    IntRemainder,
    IntShiftLeft,
    IntShiftRight,
    IntSubtract,
    IntToText,
    ListFilled,
    ListGenerate,
    ListGet,
    ListInsert,
    ListLength,
    ListOf0,
    ListOf1,
    ListOf2,
    ListOf3,
    ListOf4,
    ListOf5,
    ListRemoveAt,
    ListReplace,
    Panic,
    Print,
    TextCompareTo,
    TextConcat,
    TextGetRange,
    TextIndexOf,
    TextLength,
}
impl BuiltinFunction {
    #[must_use]
    pub fn id(self) -> Id {
        Id::from_usize(self as usize)
    }

    #[must_use]
    pub fn signature(self) -> BuiltinFunctionSignature {
        match self {
            Self::IntAdd => BuiltinFunctionSignature {
                name: "builtinIntAdd".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("a".into(), NamedType::int().into()),
                    ("b".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntBitwiseAnd => BuiltinFunctionSignature {
                name: "builtinIntBitwiseAnd".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("a".into(), NamedType::int().into()),
                    ("b".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntBitwiseOr => BuiltinFunctionSignature {
                name: "builtinIntBitwiseOr".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("a".into(), NamedType::int().into()),
                    ("b".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntBitwiseXor => BuiltinFunctionSignature {
                name: "builtinIntBitwiseXor".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("a".into(), NamedType::int().into()),
                    ("b".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntCompareTo => BuiltinFunctionSignature {
                name: "builtinIntCompareTo".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("a".into(), NamedType::int().into()),
                    ("b".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::ordering().into(),
            },
            Self::IntDivideTruncating => BuiltinFunctionSignature {
                name: "builtinIntDivideTruncating".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("dividend".into(), NamedType::int().into()),
                    ("divisor".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntMultiply => BuiltinFunctionSignature {
                name: "builtinIntMultiply".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("factorA".into(), NamedType::int().into()),
                    ("factorB".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntParse => BuiltinFunctionSignature {
                name: "builtinIntParse".into(),
                type_parameters: Box::default(),
                parameters: [("text".into(), NamedType::text().into())].into(),
                return_type: NamedType::result(NamedType::int(), NamedType::text()).into(),
            },
            Self::IntRemainder => BuiltinFunctionSignature {
                name: "builtinIntRemainder".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("dividend".into(), NamedType::int().into()),
                    ("divisor".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntShiftLeft => BuiltinFunctionSignature {
                name: "builtinIntShiftLeft".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("value".into(), NamedType::int().into()),
                    ("amount".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntShiftRight => BuiltinFunctionSignature {
                name: "builtinIntShiftRight".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("value".into(), NamedType::int().into()),
                    ("amount".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntSubtract => BuiltinFunctionSignature {
                name: "builtinIntSubtract".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("minuend".into(), NamedType::int().into()),
                    ("subtrahend".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::IntToText => BuiltinFunctionSignature {
                name: "builtinIntToText".into(),
                type_parameters: Box::default(),
                parameters: [("int".into(), NamedType::int().into())].into(),
                return_type: NamedType::text().into(),
            },
            Self::ListFilled => BuiltinFunctionSignature {
                name: "builtinListFilled".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    ("length".into(), NamedType::int().into()),
                    ("item".into(), ParameterType::new("T").into()),
                ]
                .into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListGenerate => BuiltinFunctionSignature {
                name: "builtinListGenerate".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    ("length".into(), NamedType::int().into()),
                    (
                        "itemGetter".into(),
                        FunctionType::new([NamedType::int().into()], ParameterType::new("T"))
                            .into(),
                    ),
                ]
                .into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListGet => BuiltinFunctionSignature {
                name: "builtinListGet".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    (
                        "list".into(),
                        NamedType::list(ParameterType::new("T")).into(),
                    ),
                    ("index".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::maybe(ParameterType::new("T")).into(),
            },
            Self::ListInsert => BuiltinFunctionSignature {
                name: "builtinListInsert".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    (
                        "list".into(),
                        NamedType::list(ParameterType::new("T")).into(),
                    ),
                    ("index".into(), NamedType::int().into()),
                    ("item".into(), ParameterType::new("T").into()),
                ]
                .into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListLength => BuiltinFunctionSignature {
                name: "builtinListLength".into(),
                type_parameters: ["T".into()].into(),
                parameters: [(
                    "list".into(),
                    NamedType::list(ParameterType::new("T")).into(),
                )]
                .into(),
                return_type: NamedType::int().into(),
            },
            Self::ListOf0 => BuiltinFunctionSignature {
                name: "builtinListOf".into(),
                type_parameters: ["T".into()].into(),
                parameters: Box::default(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListOf1 => BuiltinFunctionSignature {
                name: "builtinListOf".into(),
                type_parameters: ["T".into()].into(),
                parameters: [("item0".into(), ParameterType::new("T").into())].into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListOf2 => BuiltinFunctionSignature {
                name: "builtinListOf".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    ("item0".into(), ParameterType::new("T").into()),
                    ("item1".into(), ParameterType::new("T").into()),
                ]
                .into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListOf3 => BuiltinFunctionSignature {
                name: "builtinListOf".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    ("item0".into(), ParameterType::new("T").into()),
                    ("item1".into(), ParameterType::new("T").into()),
                    ("item2".into(), ParameterType::new("T").into()),
                ]
                .into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListOf4 => BuiltinFunctionSignature {
                name: "builtinListOf".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    ("item0".into(), ParameterType::new("T").into()),
                    ("item1".into(), ParameterType::new("T").into()),
                    ("item2".into(), ParameterType::new("T").into()),
                    ("item3".into(), ParameterType::new("T").into()),
                ]
                .into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListOf5 => BuiltinFunctionSignature {
                name: "builtinListOf".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    ("item0".into(), ParameterType::new("T").into()),
                    ("item1".into(), ParameterType::new("T").into()),
                    ("item2".into(), ParameterType::new("T").into()),
                    ("item3".into(), ParameterType::new("T").into()),
                    ("item4".into(), ParameterType::new("T").into()),
                ]
                .into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListRemoveAt => BuiltinFunctionSignature {
                name: "builtinListRemoveAt".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    (
                        "list".into(),
                        NamedType::list(ParameterType::new("T")).into(),
                    ),
                    ("index".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::ListReplace => BuiltinFunctionSignature {
                name: "builtinListReplace".into(),
                type_parameters: ["T".into()].into(),
                parameters: [
                    (
                        "list".into(),
                        NamedType::list(ParameterType::new("T")).into(),
                    ),
                    ("index".into(), NamedType::int().into()),
                    ("newItem".into(), ParameterType::new("T").into()),
                ]
                .into(),
                return_type: NamedType::list(ParameterType::new("T")).into(),
            },
            Self::Panic => BuiltinFunctionSignature {
                name: "builtinPanic".into(),
                type_parameters: Box::default(),
                parameters: [("message".into(), NamedType::text().into())].into(),
                return_type: NamedType::never().into(),
            },
            Self::Print => BuiltinFunctionSignature {
                name: "builtinPrint".into(),
                type_parameters: Box::default(),
                parameters: [("message".into(), NamedType::text().into())].into(),
                return_type: NamedType::nothing().into(),
            },
            Self::TextCompareTo => BuiltinFunctionSignature {
                name: "builtinTextCompareTo".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("a".into(), NamedType::text().into()),
                    ("b".into(), NamedType::text().into()),
                ]
                .into(),
                return_type: NamedType::ordering().into(),
            },
            Self::TextConcat => BuiltinFunctionSignature {
                name: "builtinTextConcat".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("a".into(), NamedType::text().into()),
                    ("b".into(), NamedType::text().into()),
                ]
                .into(),
                return_type: NamedType::text().into(),
            },
            Self::TextGetRange => BuiltinFunctionSignature {
                name: "builtinTextGetRange".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("text".into(), NamedType::text().into()),
                    ("startInclusive".into(), NamedType::int().into()),
                    ("endExclusive".into(), NamedType::int().into()),
                ]
                .into(),
                return_type: NamedType::text().into(),
            },
            Self::TextIndexOf => BuiltinFunctionSignature {
                name: "builtinTextIndexOf".into(),
                type_parameters: Box::default(),
                parameters: [
                    ("a".into(), NamedType::text().into()),
                    ("b".into(), NamedType::text().into()),
                ]
                .into(),
                return_type: NamedType::maybe(NamedType::int()).into(),
            },
            Self::TextLength => BuiltinFunctionSignature {
                name: "builtinTextLength".into(),
                type_parameters: Box::default(),
                parameters: [("text".into(), NamedType::text().into())].into(),
                return_type: NamedType::int().into(),
            },
        }
    }
}
#[derive(Debug)]
pub struct BuiltinFunctionSignature {
    pub name: Box<str>,
    pub type_parameters: Box<[Box<str>]>,
    pub parameters: Box<[(Box<str>, Type)]>,
    pub return_type: Type,
}
