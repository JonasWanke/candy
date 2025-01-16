use crate::{
    ast_to_hir::AstToHir,
    builtin_functions::BuiltinFunction,
    error::CompilerError,
    impl_countable_id, impl_display_via_richir,
    module::{Module, ModuleKind, Package},
    rich_ir::{ReferenceKey, RichIrBuilder, ToRichIr, TokenType},
};
use derive_more::From;
use enumset::EnumSet;
use itertools::Itertools;
use lazy_static::lazy_static;
use linked_hash_map::LinkedHashMap;
use num_bigint::BigUint;
use rustc_hash::FxHashMap;
use std::{
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
    sync::Arc,
};
use tracing::info;

#[salsa::query_group(HirDbStorage)]
pub trait HirDb: AstToHir {
    fn find_expression(&self, id: Id) -> Option<Expression>;
    fn containing_body_of(&self, id: Id) -> Arc<Body>;
    fn all_hir_ids(&self, module: Module) -> Vec<Id>;
}
#[allow(clippy::needless_pass_by_value)]
fn find_expression(db: &dyn HirDb, id: Id) -> Option<Expression> {
    let (hir, _) = db.hir(id.module.clone()).ok()?;
    assert!(
        !id.is_root(),
        "You can't get the root because that got lowered into multiple IDs.",
    );

    hir.find(&id).cloned()
}
fn containing_body_of(db: &dyn HirDb, id: Id) -> Arc<Body> {
    let parent_id = id.parent().expect("The root scope has no parent.");

    if parent_id.is_root() {
        db.hir(id.module).unwrap().0
    } else {
        match db.find_expression(parent_id).unwrap() {
            Expression::Match { cases, .. } => {
                let body = cases
                    .into_iter()
                    .flat_map(
                        |MatchCase {
                             condition, body, ..
                         }| condition.into_iter().chain([body]),
                    )
                    .find(|body| body.expressions.contains_key(&id))
                    .unwrap();
                Arc::new(body)
            }
            Expression::Function(function) => Arc::new(function.body),
            _ => panic!("Parent of an expression must be a function (or root scope)."),
        }
    }
}
fn all_hir_ids(db: &dyn HirDb, module: Module) -> Vec<Id> {
    let Ok((hir, _)) = db.hir(module) else {
        return vec![];
    };
    let mut ids = vec![];
    hir.collect_all_ids(&mut ids);
    info!("All HIR IDs: {ids:?}");
    ids
}

impl Expression {
    pub fn collect_all_ids(&self, ids: &mut Vec<Id>) {
        match self {
            Self::Int(_) => {}
            Self::Text(_) => {}
            Self::Reference(id) => {
                ids.push(id.clone());
            }
            Self::Symbol(_) => {}
            Self::List(items) => {
                ids.extend_from_slice(items);
            }
            Self::Struct(entries) => {
                for (key_id, value_id) in entries {
                    ids.push(key_id.clone());
                    ids.push(value_id.clone());
                }
            }
            Self::Destructure {
                expression,
                pattern: _,
            } => ids.push(expression.clone()),
            Self::PatternIdentifierReference(_) => {}
            Self::Match { expression, cases } => {
                ids.push(expression.clone());
                for MatchCase {
                    condition, body, ..
                } in cases
                {
                    if let Some(condition) = condition {
                        condition.collect_all_ids(ids);
                    }
                    body.collect_all_ids(ids);
                }
            }
            Self::Function(Function {
                parameters, body, ..
            }) => {
                for parameter in parameters {
                    ids.push(parameter.clone());
                }
                body.collect_all_ids(ids);
            }
            Self::Call {
                function,
                arguments,
            } => {
                ids.push(function.clone());
                ids.extend_from_slice(arguments);
            }
            Self::UseModule { relative_path, .. } => {
                ids.push(relative_path.clone());
            }
            Self::Builtin(_) => {}
            Self::Needs { condition, reason } => {
                ids.push(condition.clone());
                ids.push(reason.clone());
            }
            Self::Error { .. } => {}
        }
    }
}
impl Body {
    fn collect_all_ids(&self, ids: &mut Vec<Id>) {
        ids.extend(self.expressions.keys().cloned());
        for expression in self.expressions.values() {
            expression.collect_all_ids(ids);
        }
    }
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id {
    pub module: Module,
    pub keys: IdPath,
}
#[derive(Clone, Eq, From, Hash, Ord, PartialEq, PartialOrd, Debug, Default)]
pub struct IdPath(String);
#[derive(Clone, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
pub enum IdKey {
    Named { name: String, disambiguator: usize },
    Positional(usize),
}
impl IdPath {
    #[must_use]
    fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    fn child(&self, key: IdKey) -> Self {
        if self.is_root() {
            Self::from(key)
        } else {
            Self(format!("{}:{key}", self.0))
        }
    }

    #[must_use]
    pub fn last_as_str(&self) -> Option<&str> {
        self.0.rsplit(':').next()
    }

    #[must_use]
    fn parent(&self) -> Option<Self> {
        self.0
            .rfind(':')
            .map(|i| Self(self.0[..i].to_string()))
            .or_else(|| {
                if self.is_root() {
                    None
                } else {
                    Some(Self::default())
                }
            })
    }
}
impl From<IdKey> for IdPath {
    fn from(value: IdKey) -> Self {
        Self(format!("{value}"))
    }
}
impl From<Vec<IdKey>> for IdPath {
    fn from(value: Vec<IdKey>) -> Self {
        Self(value.iter().join(":"))
    }
}
impl Display for IdPath {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn tooling_module(name: String) -> Module {
    Module::new(Package::Tooling(name), vec![], ModuleKind::Code)
}
lazy_static! {
    static ref USER_MODULE: Module = tooling_module("user".to_string());
    static ref PLATFORM_MODULE: Module = tooling_module("platform".to_string());
    static ref FUZZER_MODULE: Module = tooling_module("fuzzer".to_string());
    static ref DUMMY_MODULE: Module = tooling_module("dummy".to_string());
    static ref NEEDS_MODULE: Module = Module::new(
        Package::Anonymous {
            url: "$generated".to_string(),
        },
        vec![],
        ModuleKind::Code,
    );
}

impl Id {
    #[must_use]
    pub fn new(module: Module, keys: Vec<IdKey>) -> Self {
        Self {
            module,
            keys: keys.into(),
        }
    }

    /// An ID that can be used to blame the tooling. For example, when calling
    /// the `main` function, we want to be able to blame the platform for
    /// passing a wrong environment.
    fn tooling(module: Module) -> Self {
        Self {
            module,
            keys: IdPath::default(),
        }
    }
    /// The user of the Candy tooling is responsible. For example, when the user
    /// instructs the tooling to run a non-existent module, then the program
    /// will panic with this responsibility.
    #[must_use]
    pub fn user() -> Self {
        Self::tooling(USER_MODULE.clone())
    }
    /// Refers to the platform (non-Candy code).
    #[must_use]
    pub fn platform() -> Self {
        Self::tooling(PLATFORM_MODULE.clone())
    }
    #[must_use]
    pub fn fuzzer() -> Self {
        Self::tooling(FUZZER_MODULE.clone())
    }
    /// A dummy ID that is guaranteed to never be responsible for a panic.
    #[must_use]
    pub fn dummy() -> Self {
        Self::tooling(DUMMY_MODULE.clone())
    }

    #[must_use]
    pub fn needs() -> Self {
        Self {
            module: NEEDS_MODULE.clone(),
            keys: IdKey::from("needs").into(),
        }
    }

    #[must_use]
    pub fn to_short_debug_string(&self) -> String {
        format!("${}", self.keys)
    }

    #[must_use]
    pub fn is_root(&self) -> bool {
        self.keys.is_root()
    }

    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        self.keys.parent().map(|keys| Self {
            module: self.module.clone(),
            keys,
        })
    }

    #[must_use]
    pub fn child(&self, key: impl Into<IdKey>) -> Self {
        Self {
            module: self.module.clone(),
            keys: self.keys.child(key.into()),
        }
    }

    #[must_use]
    pub fn is_same_module_and_any_parent_of(&self, other: &Self) -> bool {
        self.module == other.module
            && other
                .keys
                .0
                .starts_with(format!("{}:", self.keys.0).as_str())
    }

    #[must_use]
    pub fn function_name(&self) -> String {
        self.keys
            .0
            .split(':')
            .map(|it| {
                let first_char = it.chars().next().unwrap();
                if first_char.is_numeric() {
                    return format!("<anonymous {it}>");
                }
                let last_char = it.chars().last().unwrap();
                if last_char.is_numeric() {
                    return it.rfind('#').map_or_else(|| it, |i| &it[..i]).to_string();
                }
                it.to_string()
            })
            .join(" → ")
    }
}
impl Debug for Id {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.module, self.keys)
    }
}
impl Display for Id {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl ToRichIr for Id {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        let range = builder.push(
            self.to_short_debug_string(),
            TokenType::Variable,
            EnumSet::empty(),
        );
        builder.push_reference(self.clone(), range);
    }
}
impl Debug for IdKey {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match &self {
            Self::Named {
                name,
                disambiguator,
            } => {
                write!(f, "{name}")?;
                if disambiguator > &0 {
                    write!(f, "#{disambiguator}")?;
                }
                Ok(())
            }
            Self::Positional(index) => write!(f, "{index}"),
        }
    }
}
impl Display for IdKey {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl From<String> for IdKey {
    fn from(value: String) -> Self {
        Self::Named {
            name: value,
            disambiguator: 0,
        }
    }
}
impl From<&str> for IdKey {
    fn from(value: &str) -> Self {
        value.to_string().into()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Expression {
    Int(BigUint),
    Text(String),
    Reference(Id),
    Symbol(String),
    List(Vec<Id>),
    Struct(FxHashMap<Id, Id>),
    Destructure {
        expression: Id,
        pattern: Pattern,
    },
    PatternIdentifierReference(PatternIdentifierId),
    Match {
        expression: Id,
        /// Each case consists of the pattern to match against, and the body
        /// which starts with [PatternIdentifierReference]s for all identifiers
        /// in the pattern.
        cases: Vec<MatchCase>,
    },
    Function(Function),
    Builtin(BuiltinFunction),
    Call {
        function: Id,
        arguments: Vec<Id>,
    },
    UseModule {
        current_module: Module,
        relative_path: Id,
    },
    Needs {
        condition: Id,
        reason: Id,
    },
    Error {
        errors: Vec<CompilerError>,
    },
}
impl Expression {
    #[must_use]
    pub fn nothing() -> Self {
        Self::Symbol("Nothing".to_string())
    }
}
#[allow(clippy::derived_hash_with_manual_eq)]
impl Hash for Expression {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PatternIdentifierId(pub usize);
impl_countable_id!(PatternIdentifierId);
impl Debug for PatternIdentifierId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pattern_identifier_{:x}", self.0)
    }
}
impl Display for PatternIdentifierId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p${}", self.0)
    }
}
impl ToRichIr for PatternIdentifierId {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        // TODO: convert to actual reference
        builder.push(self.to_string(), TokenType::Variable, EnumSet::empty());
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub identifier_expressions: Body,
    pub condition: Option<Body>,
    pub body: Body,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Pattern {
    NewIdentifier(PatternIdentifierId),
    Int(BigUint),
    Text(String),
    Tag {
        symbol: String,
        value: Option<Box<Pattern>>,
    },
    List(Vec<Pattern>),
    // Keys may not contain `NewIdentifier`.
    Struct(Vec<(Pattern, Pattern)>),
    Or(Vec<Pattern>),
    Error {
        errors: Vec<CompilerError>,
    },
}
#[allow(clippy::derived_hash_with_manual_eq)]
impl Hash for Pattern {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
    }
}
impl Pattern {
    pub fn contains_captured_identifiers(&self) -> bool {
        match self {
            Self::NewIdentifier(_) => true,
            Self::Int(_) | Self::Text(_) => false,
            Self::Tag { value, .. } => value
                .as_ref()
                .is_some_and(|value| value.contains_captured_identifiers()),
            Self::List(list) => list.iter().any(Self::contains_captured_identifiers),
            Self::Struct(struct_) => struct_
                .iter()
                .any(|(_, value_pattern)| value_pattern.contains_captured_identifiers()),
            Self::Or(patterns) => patterns.first().unwrap().contains_captured_identifiers(),
            Self::Error { .. } => false,
        }
    }
    pub fn captured_identifier_count(&self) -> usize {
        match self {
            Self::NewIdentifier(_) => 1,
            Self::Int(_) | Self::Text(_) => 0,
            Self::Tag { value, .. } => value
                .as_ref()
                .map(|value| value.captured_identifier_count())
                .unwrap_or_default(),
            Self::List(list) => list.iter().map(Self::captured_identifier_count).sum(),
            Self::Struct(struct_) => struct_
                .iter()
                .map(|(key, value)| {
                    key.captured_identifier_count() + value.captured_identifier_count()
                })
                .sum(),
            // If the number or captured identifiers isn't the same in both
            // sides, the pattern is invalid and the generated code will panic.
            Self::Or(patterns) => patterns.first().unwrap().captured_identifier_count(),
            Self::Error { .. } => 0,
        }
    }

    /// Returns a mapping from `PatternIdentifierId` to the position of the
    /// corresponding identifier in the `(Match, …)` result (zero-based,
    /// ignoring the `Match` symbol).
    #[must_use]
    pub fn captured_identifiers(&self) -> Vec<PatternIdentifierId> {
        let mut ids = vec![];
        self.collect_captured_identifiers(&mut ids);
        ids
    }
    fn collect_captured_identifiers(&self, ids: &mut Vec<PatternIdentifierId>) {
        match self {
            Self::NewIdentifier(identifier_id) => ids.push(*identifier_id),
            Self::Int(_) | Self::Text(_) => {}
            Self::Tag { value, .. } => {
                if let Some(value) = value {
                    value.collect_captured_identifiers(ids);
                }
            }
            Self::List(list) => {
                for pattern in list {
                    pattern.collect_captured_identifiers(ids);
                }
            }
            Self::Struct(struct_) => {
                for (_, value_pattern) in struct_ {
                    // Keys can't capture identifiers.
                    value_pattern.collect_captured_identifiers(ids);
                }
            }
            // If the number or captured identifiers isn't the same in both
            // sides, the pattern is invalid and the generated code will panic.
            Self::Or(patterns) => patterns.first().unwrap().collect_captured_identifiers(ids),
            Self::Error { .. } => {
                // Since generated code panics in this case, it doesn't matter
                // whether the child captured any identifiers since they can't
                // be accessed anyway.
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Function {
    pub parameters: Vec<Id>,
    pub body: Body,
    pub kind: FunctionKind,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FunctionKind {
    /// A normal function (e.g., `foo a = …`).
    Normal,

    /// The generated use function, which is not fuzzable but otherwise behaves
    /// like a normal function (i.e., it passes on its responsibility
    /// parameter).
    Use,

    /// A function defined using curly braces (e.g., `foo = { a -> … }`).
    CurlyBraces,
}
impl FunctionKind {
    #[must_use]
    pub const fn is_fuzzable(self) -> bool {
        match self {
            Self::Normal => true,
            Self::Use | Self::CurlyBraces => false,
        }
    }
    /// For functions with curly braces, whoever is responsible for `needs` in
    /// the outer scope is also responsible for `needs` in this function.
    #[must_use]
    pub const fn uses_own_responsibility(self) -> bool {
        match self {
            Self::Normal | Self::Use => true,
            Self::CurlyBraces => false,
        }
    }
}

#[derive(Clone, Debug, Eq, Default, PartialEq)]
pub struct Body {
    pub expressions: LinkedHashMap<Id, Expression>,
    pub identifiers: FxHashMap<Id, String>,
}
#[allow(clippy::derived_hash_with_manual_eq)]
impl Hash for Body {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.expressions.hash(state);
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum HirError {
    NeedsWithWrongNumberOfArguments { num_args: usize },
    PatternContainsCall,
    PublicAssignmentInNotTopLevel,
    PublicAssignmentWithSameName { name: String },
    UnknownReference { name: String },
}

impl Body {
    pub fn push(&mut self, id: Id, expression: Expression, identifier: Option<String>) {
        self.expressions.insert(id.clone(), expression);
        if let Some(identifier) = identifier {
            self.identifiers.insert(id, identifier);
        }
    }
}

impl_display_via_richir!(Expression);
impl ToRichIr for Expression {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        match self {
            Self::Int(int) => {
                int.build_rich_ir(builder);
            }
            Self::Text(text) => {
                let range = builder.push(format!(r#""{text}""#), TokenType::Text, EnumSet::empty());
                builder.push_reference(text.clone(), range);
            }
            Self::Reference(reference) => {
                reference.build_rich_ir(builder);
            }
            Self::Symbol(symbol) => {
                let range = builder.push(symbol, TokenType::Symbol, EnumSet::empty());
                builder.push_reference(ReferenceKey::Symbol(symbol.clone()), range);
            }
            Self::List(items) => {
                builder.push("(", None, EnumSet::empty());
                builder.push_children(items, ", ");
                builder.push(")", None, EnumSet::empty());
            }
            Self::Struct(fields) => {
                builder.push("[", None, EnumSet::empty());
                builder.push_children_custom(
                    fields.iter().collect_vec(),
                    |builder, (key, value)| {
                        key.build_rich_ir(builder);
                        builder.push(": ", None, EnumSet::empty());
                        value.build_rich_ir(builder);
                    },
                    ", ",
                );
                builder.push("]", None, EnumSet::empty());
            }
            Self::Destructure {
                expression,
                pattern,
            } => {
                builder.push("destructure ", None, EnumSet::empty());
                expression.build_rich_ir(builder);
                builder.push(" into ", None, EnumSet::empty());
                pattern.build_rich_ir(builder);
            }
            Self::PatternIdentifierReference(identifier_id) => {
                identifier_id.build_rich_ir(builder);
            }
            Self::Match { expression, cases } => {
                expression.build_rich_ir(builder);
                builder.push(" %", None, EnumSet::empty());
                builder.push_children_custom_multiline(
                    cases,
                    |builder,
                     MatchCase {
                         pattern,
                         condition,
                         body,
                         ..
                     }| {
                        pattern.build_rich_ir(builder);
                        if let Some(condition) = condition {
                            builder.push(", ", None, EnumSet::empty());
                            builder.indent();
                            builder.push_newline();
                            condition.build_rich_ir(builder);
                            builder.dedent();
                            builder.push_newline();
                        }
                        builder.push(" ->", None, EnumSet::empty());
                        builder.push_indented_foldable(|builder| {
                            if !body.expressions.is_empty() {
                                builder.push_newline();
                            }
                            body.build_rich_ir(builder);
                        });
                    },
                );
            }
            Self::Function(function) => {
                builder.push(
                    format!(
                        "{{ ({}) ",
                        match function.kind {
                            FunctionKind::Normal => "fuzzable",
                            FunctionKind::Use => "non-fuzzable, but passes on responsibility",
                            FunctionKind::CurlyBraces => "non-fuzzable",
                        },
                    ),
                    None,
                    EnumSet::empty(),
                );
                function.build_rich_ir(builder);
                builder.push("}", None, EnumSet::empty());
            }
            Self::Builtin(builtin) => {
                builtin.build_rich_ir(builder);
            }
            Self::Call {
                function,
                arguments,
            } => {
                assert!(!arguments.is_empty(), "A call needs to have arguments.");
                builder.push("call ", None, EnumSet::empty());
                function.build_rich_ir(builder);
                builder.push(" with ", None, EnumSet::empty());
                builder.push_children(arguments, " ");
            }
            Self::UseModule {
                current_module,
                relative_path,
            } => {
                builder.push("relative to module ", None, EnumSet::empty());
                current_module.build_rich_ir(builder);
                builder.push(", use ", None, EnumSet::empty());
                relative_path.build_rich_ir(builder);
            }
            Self::Needs { condition, reason } => {
                builder.push("needs ", None, EnumSet::empty());
                condition.build_rich_ir(builder);
                builder.push(" with reason ", None, EnumSet::empty());
                reason.build_rich_ir(builder);
            }
            Self::Error { errors } => {
                build_errors_rich_ir(builder, errors);
            }
        }
    }
}
impl ToRichIr for Pattern {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        match self {
            Self::Int(int) => {
                builder.push(format!("{int}"), TokenType::Int, EnumSet::empty());
            }
            Self::Text(text) => {
                builder.push(format!(r#""{text}""#), TokenType::Text, EnumSet::empty());
            }
            Self::NewIdentifier(reference) => reference.build_rich_ir(builder),
            Self::Tag { symbol, value } => {
                builder.push(symbol, TokenType::Symbol, EnumSet::empty());
                if let Some(value) = value {
                    builder.push(" (", None, EnumSet::empty());
                    value.build_rich_ir(builder);
                    builder.push(")", None, EnumSet::empty());
                }
            }
            Self::List(items) => {
                builder.push("(", None, EnumSet::empty());
                builder.push_children(items, ", ");
                if items.len() <= 1 {
                    builder.push(",", None, EnumSet::empty());
                }
                builder.push(")", None, EnumSet::empty());
            }
            Self::Struct(entries) => {
                builder.push("[", None, EnumSet::empty());
                builder.push_children_custom(
                    entries,
                    |builder, (key, value)| {
                        key.build_rich_ir(builder);
                        builder.push(": ", None, EnumSet::empty());
                        value.build_rich_ir(builder);
                    },
                    ", ",
                );
                builder.push("]", None, EnumSet::empty());
            }
            Self::Or(patterns) => builder.push_children(patterns, " | "),
            Self::Error { errors } => {
                build_errors_rich_ir(builder, errors);
            }
        }
    }
}
fn build_errors_rich_ir(builder: &mut RichIrBuilder, errors: &[CompilerError]) {
    builder.push(
        if errors.len() == 1 { "error" } else { "errors" },
        None,
        EnumSet::empty(),
    );
    builder.push_foldable(|builder| {
        builder.push_children_multiline(errors);
    });
}
impl ToRichIr for Function {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        for parameter in &self.parameters {
            let range = builder.push(
                parameter.to_short_debug_string(),
                TokenType::Parameter,
                EnumSet::empty(),
            );
            builder.push_definition(parameter.clone(), range);
            builder.push(" ", None, EnumSet::empty());
        }
        builder.push("->", None, EnumSet::empty());
        builder.push_indented_foldable(|builder| {
            builder.push_newline();
            self.body.build_rich_ir(builder);
        });
        builder.push_newline();
    }
}
impl ToRichIr for Body {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        fn push(builder: &mut RichIrBuilder, id: &Id, expression: &Expression) {
            let range = builder.push(
                id.to_short_debug_string(),
                TokenType::Variable,
                EnumSet::empty(),
            );
            builder.push_definition(id.clone(), range);

            builder.push(" = ", None, EnumSet::empty());
            expression.build_rich_ir(builder);
        }

        let mut iterator = self.expressions.iter();
        if let Some((id, expression)) = iterator.next() {
            push(builder, id, expression);
        }
        for (id, expression) in iterator {
            builder.push_newline();
            push(builder, id, expression);
        }
    }
}

impl Expression {
    fn find(&self, id: &Id) -> Option<&Self> {
        match self {
            Self::Int { .. } => None,
            Self::Text { .. } => None,
            Self::Reference { .. } => None,
            Self::Symbol { .. } => None,
            Self::List(_) => None,
            Self::Struct(_) => None,
            Self::Destructure { .. } => None,
            Self::PatternIdentifierReference { .. } => None,
            // TODO: use binary search
            Self::Match { cases, .. } => cases.iter().find_map(
                |MatchCase {
                     condition, body, ..
                 }| {
                    condition
                        .as_ref()
                        .and_then(|condition| condition.find(id))
                        .or_else(|| body.find(id))
                },
            ),
            Self::Function(Function { body, .. }) => body.find(id),
            Self::Builtin(_) => None,
            Self::Call { .. } => None,
            Self::UseModule { .. } => None,
            Self::Needs { .. } => None,
            Self::Error { .. } => None,
        }
    }
}
impl Body {
    #[must_use]
    pub fn find(&self, id: &Id) -> Option<&Expression> {
        if let Some(expression) = self.expressions.get(id) {
            Some(expression)
        } else {
            self.expressions
                .iter()
                .filter(|(it, _)| it <= &id)
                .max_by_key(|(id, _)| id.keys.clone())?
                .1
                .find(id)
        }
    }
}

pub trait CollectErrors {
    fn collect_errors(&self, errors: &mut Vec<CompilerError>);
}
impl CollectErrors for Expression {
    fn collect_errors(&self, errors: &mut Vec<CompilerError>) {
        match self {
            Self::Int(_)
            | Self::Text(_)
            | Self::Reference(_)
            | Self::Symbol(_)
            | Self::List(_)
            | Self::Struct(_)
            | Self::PatternIdentifierReference { .. } => {}
            Self::Match { cases, .. } => {
                for MatchCase {
                    pattern,
                    identifier_expressions: _,
                    condition,
                    body,
                } in cases
                {
                    pattern.collect_errors(errors);
                    if let Some(condition) = condition {
                        condition.collect_errors(errors);
                    }
                    body.collect_errors(errors);
                }
            }
            Self::Builtin(_) | Self::Call { .. } | Self::UseModule { .. } | Self::Needs { .. } => {}
            Self::Function(function) => function.body.collect_errors(errors),
            Self::Destructure { pattern, .. } => pattern.collect_errors(errors),
            Self::Error {
                errors: the_errors, ..
            } => {
                errors.append(&mut the_errors.clone());
            }
        }
    }
}
impl CollectErrors for Pattern {
    fn collect_errors(&self, errors: &mut Vec<CompilerError>) {
        match self {
            Self::NewIdentifier(_) | Self::Int(_) | Self::Text(_) => {}
            Self::Tag { value, .. } => {
                if let Some(value) = value {
                    value.collect_errors(errors);
                }
            }
            Self::List(patterns) => {
                for item_pattern in patterns {
                    item_pattern.collect_errors(errors);
                }
            }
            Self::Struct(patterns) => {
                for (key_pattern, value_pattern) in patterns {
                    key_pattern.collect_errors(errors);
                    value_pattern.collect_errors(errors);
                }
            }
            Self::Or(patterns) => {
                for pattern in patterns {
                    pattern.collect_errors(errors);
                }
            }
            Self::Error {
                errors: the_errors, ..
            } => {
                errors.append(&mut the_errors.clone());
            }
        }
    }
}
impl CollectErrors for Body {
    fn collect_errors(&self, errors: &mut Vec<CompilerError>) {
        for (_id, expression) in &self.expressions {
            expression.collect_errors(errors);
        }
    }
}
impl<T: CollectErrors> CollectErrors for Option<T> {
    fn collect_errors(&self, errors: &mut Vec<CompilerError>) {
        if let Some(this) = self {
            this.collect_errors(errors);
        }
    }
}
