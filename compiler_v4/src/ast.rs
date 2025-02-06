use crate::{error::CompilerError, position::Offset};
use derive_more::Deref;
use std::{ops::Range, path::PathBuf};

#[derive(Clone, Debug, Deref, Eq, Hash, PartialEq)]
pub struct AstString {
    #[deref]
    pub string: Box<str>,
    pub file: PathBuf,
    pub span: Range<Offset>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstResult<T> {
    value: Option<T>,
    errors: Vec<AstError>,
}
impl<T> AstResult<T> {
    #[must_use]
    pub const fn ok(value: T) -> Self {
        Self {
            value: Some(value),
            errors: vec![],
        }
    }
    #[must_use]
    pub fn error(value: impl Into<Option<T>>, error: AstError) -> Self {
        Self::errors(value, vec![error])
    }
    #[must_use]
    pub fn errors(value: impl Into<Option<T>>, errors: Vec<AstError>) -> Self {
        Self {
            value: value.into(),
            errors,
        }
    }

    #[must_use]
    pub const fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    #[must_use]
    pub fn map<U>(self, op: impl FnOnce(T) -> U) -> AstResult<U> {
        AstResult {
            value: self.value.map(op),
            errors: self.errors,
        }
    }
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstError {
    pub unparsable_input: AstString,
    pub error: String,
}

pub type Ast = Vec<AstDeclaration>;

// Declarations

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AstDeclaration {
    Struct(AstStruct),
    Enum(AstEnum),
    Trait(AstTrait),
    Impl(AstImpl),
    Assignment(AstAssignment),
    Function(AstFunction),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstStruct {
    pub name: AstResult<AstString>,
    pub type_parameters: Option<AstTypeParameters>,
    pub kind: AstStructKind,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AstStructKind {
    Builtin {
        builtin_keyword_error: Option<AstError>,
    },
    UserDefined {
        opening_curly_brace_error: Option<AstError>,
        fields: Vec<AstStructField>,
        closing_curly_brace_error: Option<AstError>,
    },
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstStructField {
    pub name: AstResult<AstString>,
    pub colon_error: Option<AstError>,
    pub type_: AstResult<AstType>,
    pub comma_error: Option<AstError>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstEnum {
    pub name: AstResult<AstString>,
    pub type_parameters: Option<AstTypeParameters>,
    pub opening_curly_brace_error: Option<AstError>,
    pub variants: Vec<AstEnumVariant>,
    pub closing_curly_brace_error: Option<AstError>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstEnumVariant {
    pub name: AstResult<AstString>,
    pub type_: Option<AstResult<AstType>>,
    pub comma_error: Option<AstError>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstTrait {
    pub name: AstResult<AstString>,
    pub type_parameters: Option<AstTypeParameters>,
    pub opening_curly_brace_error: Option<AstError>,
    pub functions: Vec<AstFunction>,
    pub closing_curly_brace_error: Option<AstError>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstImpl {
    pub impl_keyword_span: Range<Offset>,
    pub type_parameters: Option<AstTypeParameters>,
    pub type_: AstResult<AstType>,
    pub colon_error: Option<AstError>,
    pub trait_: AstResult<AstType>,
    pub opening_curly_brace_error: Option<AstError>,
    pub functions: Vec<AstFunction>,
    pub closing_curly_brace_error: Option<AstError>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstAssignment {
    pub display_span: Range<Offset>,
    pub name: AstResult<AstString>,
    pub type_: Option<AstResult<AstType>>,
    pub equals_sign_error: Option<AstError>,
    pub value: AstResult<AstExpression>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstFunction {
    pub display_span: Range<Offset>,
    pub name: AstResult<AstString>,
    pub type_parameters: Option<AstTypeParameters>,
    pub parameters: AstResult<AstParameters>,
    pub return_type: Option<AstType>,
    pub body: Option<AstBody>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstParameters {
    pub parameters: Vec<AstParameter>,
    pub closing_parenthesis_error: Option<AstError>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstParameter {
    pub name: AstResult<AstString>,
    pub colon_error: Option<AstError>,
    pub type_: AstResult<AstType>,
    pub comma_error: Option<AstError>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstTypeParameters {
    pub parameters: Vec<AstTypeParameter>,
    pub empty_parameters_error: Option<AstError>,
    pub closing_bracket_error: Option<AstError>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstTypeParameter {
    pub name: AstResult<AstString>,
    pub upper_bound: Option<AstResult<AstType>>,
    pub comma_error: Option<AstError>,
}

// Types

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AstType {
    Named(AstNamedType),
    Function(AstFunctionType),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstNamedType {
    pub name: AstResult<AstString>,
    pub type_arguments: Option<AstTypeArguments>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstFunctionType {
    pub parameter_types: Vec<AstFunctionTypeParameterType>,
    pub closing_parenthesis_error: Option<AstError>,
    pub return_type: AstResult<Box<AstType>>,
    pub span: Range<Offset>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstFunctionTypeParameterType {
    pub type_: Box<AstType>,
    pub comma_error: Option<AstError>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstTypeArguments {
    pub span: Range<Offset>,
    pub arguments: Vec<AstTypeArgument>,
    pub empty_arguments_error: Option<AstError>,
    pub closing_bracket_error: Option<AstError>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstTypeArgument {
    pub type_: AstType,
    pub comma_error: Option<AstError>,
}

// Expressions

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstExpression {
    pub span: Range<Offset>,
    pub kind: AstExpressionKind,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AstExpressionKind {
    // TODO: differentiate values and types
    Identifier(AstIdentifier),
    Int(AstInt),
    Text(AstText),
    Parenthesized(AstParenthesized),
    Call(AstCall),
    Navigation(AstNavigation),
    Lambda(AstLambda),
    Body(AstBody),
    Switch(AstSwitch),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstIdentifier {
    pub identifier: AstResult<AstString>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstInt {
    pub value: AstResult<i64>,
    pub string: AstString,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstText {
    pub parts: Vec<AstTextPart>,
    pub closing_double_quote_error: Option<AstError>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AstTextPart {
    Text(AstResult<Box<str>>),
    Interpolation {
        expression: AstResult<Box<AstExpression>>,
        closing_curly_brace_error: Option<AstError>,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstParenthesized {
    pub inner: AstResult<Box<AstExpression>>,
    pub closing_parenthesis_error: Option<AstError>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstCall {
    pub receiver: Box<AstExpression>,
    pub type_arguments: Option<AstTypeArguments>,
    pub arguments: AstResult<AstArguments>,
}
impl AstResult<AstArguments> {
    #[must_use]
    pub fn arguments_or_default(&self) -> &[AstArgument] {
        self.value().map_or::<&[_], _>(&[], |it| &it.arguments)
    }
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstArguments {
    pub opening_parenthesis_span: Range<Offset>,
    pub arguments: Vec<AstArgument>,
    pub closing_parenthesis_error: Option<AstError>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstArgument {
    pub value: AstExpression,
    pub comma_error: Option<AstError>,
    pub span: Range<Offset>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstNavigation {
    pub receiver: Box<AstExpression>,
    pub key: AstResult<AstString>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstLambda {
    pub parameters: AstParameters,
    pub body: AstBody,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstBody {
    pub statements: Vec<AstStatement>,
    pub closing_curly_brace_error: Option<AstError>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AstStatement {
    Assignment(AstAssignment),
    Expression(AstExpression),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstSwitch {
    pub value: AstResult<Box<AstExpression>>,
    pub opening_curly_brace_error: Option<AstError>,
    pub cases: Vec<AstSwitchCase>,
    pub closing_curly_brace_error: Option<AstError>,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AstSwitchCase {
    pub variant: AstResult<AstString>,
    pub value_name: Option<(AstResult<AstString>, Option<AstError>)>,
    pub arrow_error: Option<AstError>,
    pub expression: AstResult<AstExpression>,
    pub comma_error: Option<AstError>,
}

pub trait CollectAstErrors {
    fn collect_errors(&self) -> Vec<CompilerError> {
        let mut errors = vec![];
        self.collect_errors_to(&mut errors);
        errors
    }
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>);
}
#[allow(clippy::mismatching_type_param_order)]
impl<A: CollectAstErrors> CollectAstErrors for Vec<A> {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        for cst in self {
            cst.collect_errors_to(errors);
        }
    }
}
impl<A: CollectAstErrors> CollectAstErrors for Option<A> {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        if let Some(cst) = self {
            cst.collect_errors_to(errors);
        }
    }
}
#[allow(clippy::mismatching_type_param_order)]
impl<A: CollectAstErrors> CollectAstErrors for Box<A> {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.as_ref().collect_errors_to(errors);
    }
}
impl<A0: CollectAstErrors, A1: CollectAstErrors> CollectAstErrors for (A0, A1) {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.0.collect_errors_to(errors);
        self.1.collect_errors_to(errors);
    }
}
impl<A: CollectAstErrors> CollectAstErrors for AstResult<A> {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.errors.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstError {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        errors.push(CompilerError {
            path: self.unparsable_input.file.clone(),
            span: self.unparsable_input.span.clone(),
            message: self.error.clone(),
        });
    }
}
impl CollectAstErrors for AstString {
    fn collect_errors_to(&self, _errors: &mut Vec<CompilerError>) {}
}
impl CollectAstErrors for str {
    fn collect_errors_to(&self, _errors: &mut Vec<CompilerError>) {}
}
impl CollectAstErrors for i64 {
    fn collect_errors_to(&self, _errors: &mut Vec<CompilerError>) {}
}

impl CollectAstErrors for AstDeclaration {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        match &self {
            Self::Struct(struct_) => struct_.collect_errors_to(errors),
            Self::Enum(enum_) => enum_.collect_errors_to(errors),
            Self::Trait(trait_) => trait_.collect_errors_to(errors),
            Self::Impl(impl_) => impl_.collect_errors_to(errors),
            Self::Assignment(assignment_) => assignment_.collect_errors_to(errors),
            Self::Function(function_) => function_.collect_errors_to(errors),
        }
    }
}
impl CollectAstErrors for AstStruct {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.type_parameters.collect_errors_to(errors);
        self.kind.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstStructKind {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        match self {
            Self::Builtin {
                builtin_keyword_error,
            } => {
                builtin_keyword_error.collect_errors_to(errors);
            }
            Self::UserDefined {
                opening_curly_brace_error,
                fields,
                closing_curly_brace_error,
            } => {
                opening_curly_brace_error.collect_errors_to(errors);
                fields.collect_errors_to(errors);
                closing_curly_brace_error.collect_errors_to(errors);
            }
        }
    }
}
impl CollectAstErrors for AstStructField {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.colon_error.collect_errors_to(errors);
        self.type_.collect_errors_to(errors);
        self.comma_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstEnum {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.type_parameters.collect_errors_to(errors);
        self.opening_curly_brace_error.collect_errors_to(errors);
        self.variants.collect_errors_to(errors);
        self.closing_curly_brace_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstEnumVariant {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.type_.collect_errors_to(errors);
        self.comma_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstTrait {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.type_parameters.collect_errors_to(errors);
        self.opening_curly_brace_error.collect_errors_to(errors);
        self.functions.collect_errors_to(errors);
        self.closing_curly_brace_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstImpl {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.type_parameters.collect_errors_to(errors);
        self.type_.collect_errors_to(errors);
        self.colon_error.collect_errors_to(errors);
        self.trait_.collect_errors_to(errors);
        self.opening_curly_brace_error.collect_errors_to(errors);
        self.functions.collect_errors_to(errors);
        self.closing_curly_brace_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstAssignment {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.type_.collect_errors_to(errors);
        self.equals_sign_error.collect_errors_to(errors);
        self.value.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstFunction {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.type_parameters.collect_errors_to(errors);
        self.parameters.collect_errors_to(errors);
        self.return_type.collect_errors_to(errors);
        self.body.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstParameters {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.parameters.collect_errors_to(errors);
        self.closing_parenthesis_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstParameter {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.colon_error.collect_errors_to(errors);
        self.type_.collect_errors_to(errors);
        self.comma_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstTypeParameters {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.parameters.collect_errors_to(errors);
        self.empty_parameters_error.collect_errors_to(errors);
        self.closing_bracket_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstTypeParameter {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.upper_bound.collect_errors_to(errors);
        self.comma_error.collect_errors_to(errors);
    }
}

impl CollectAstErrors for AstType {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        match &self {
            Self::Named(named) => named.collect_errors_to(errors),
            Self::Function(function) => function.collect_errors_to(errors),
        }
    }
}
impl CollectAstErrors for AstNamedType {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.name.collect_errors_to(errors);
        self.type_arguments.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstFunctionType {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.parameter_types.collect_errors_to(errors);
        self.return_type.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstFunctionTypeParameterType {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.type_.collect_errors_to(errors);
        self.comma_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstTypeArguments {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.arguments.collect_errors_to(errors);
        self.empty_arguments_error.collect_errors_to(errors);
        self.closing_bracket_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstTypeArgument {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.type_.collect_errors_to(errors);
        self.comma_error.collect_errors_to(errors);
    }
}

impl CollectAstErrors for AstExpression {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.kind.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstExpressionKind {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        match &self {
            Self::Identifier(identifier) => identifier.collect_errors_to(errors),
            Self::Int(int) => int.collect_errors_to(errors),
            Self::Text(text) => text.collect_errors_to(errors),
            Self::Parenthesized(parenthesized) => parenthesized.collect_errors_to(errors),
            Self::Call(call) => call.collect_errors_to(errors),
            Self::Navigation(navigation) => navigation.collect_errors_to(errors),
            Self::Lambda(lambda) => lambda.collect_errors_to(errors),
            Self::Body(body) => body.collect_errors_to(errors),
            Self::Switch(switch) => switch.collect_errors_to(errors),
        }
    }
}
impl CollectAstErrors for AstIdentifier {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.identifier.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstInt {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.value.collect_errors_to(errors);
        self.string.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstText {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.parts.collect_errors_to(errors);
        self.closing_double_quote_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstTextPart {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        match &self {
            Self::Text(text) => {
                text.errors.collect_errors_to(errors);
            }
            Self::Interpolation {
                expression,
                closing_curly_brace_error,
            } => {
                expression.collect_errors_to(errors);
                closing_curly_brace_error.collect_errors_to(errors);
            }
        }
    }
}
impl CollectAstErrors for AstParenthesized {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.inner.collect_errors_to(errors);
        self.closing_parenthesis_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstCall {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.receiver.collect_errors_to(errors);
        self.type_arguments.collect_errors_to(errors);
        self.arguments.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstArguments {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.arguments.collect_errors_to(errors);
        self.closing_parenthesis_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstArgument {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.value.collect_errors_to(errors);
        self.comma_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstNavigation {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.receiver.collect_errors_to(errors);
        self.key.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstLambda {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.parameters.collect_errors_to(errors);
        self.body.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstBody {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.statements.collect_errors_to(errors);
        self.closing_curly_brace_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstStatement {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        match &self {
            Self::Expression(expression) => expression.collect_errors_to(errors),
            Self::Assignment(assignment) => assignment.collect_errors_to(errors),
        }
    }
}
impl CollectAstErrors for AstSwitch {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.value.collect_errors_to(errors);
        self.opening_curly_brace_error.collect_errors_to(errors);
        self.cases.collect_errors_to(errors);
        self.closing_curly_brace_error.collect_errors_to(errors);
    }
}
impl CollectAstErrors for AstSwitchCase {
    fn collect_errors_to(&self, errors: &mut Vec<CompilerError>) {
        self.variant.collect_errors_to(errors);
        self.value_name.collect_errors_to(errors);
        self.arrow_error.collect_errors_to(errors);
        self.expression.collect_errors_to(errors);
        self.comma_error.collect_errors_to(errors);
    }
}
