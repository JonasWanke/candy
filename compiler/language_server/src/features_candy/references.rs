use crate::{features::Reference, utils::LspPositionConversion};
use candy_frontend::{
    ast_to_hir::AstToHir,
    cst::{CstDb, CstKind},
    hir::{self, Body, Expression, Function, HirDb, MatchCase},
    module::{Module, ModuleDb},
    position::{Offset, PositionConversionDb},
};
use num_bigint::BigUint;
use rustc_hash::FxHashSet;
use std::ops::Range;
use tracing::{debug, info};

pub fn references<DB>(
    db: &DB,
    module: Module,
    offset: Offset,
    include_declaration: bool,
) -> Vec<Reference>
where
    DB: HirDb + ModuleDb + PositionConversionDb,
{
    let Some((query, _)) = reference_query_for_offset(db, module, offset) else {
        return vec![];
    };
    find_references(db, query, include_declaration)
}

pub fn reference_query_for_offset<DB>(
    db: &DB,
    module: Module,
    offset: Offset,
) -> Option<(ReferenceQuery, Range<Offset>)>
where
    DB: CstDb + HirDb,
{
    let origin_cst = db.find_cst_by_offset(module.clone(), offset);
    info!("Finding references for {origin_cst:?}");
    let query = match origin_cst.kind {
        CstKind::Identifier(identifier) if identifier == "needs" => {
            Some(ReferenceQuery::Needs(module))
        }
        CstKind::Identifier { .. } => {
            let hir_id = db.cst_to_last_hir_id(module, origin_cst.data.id)?;
            debug!("HIR ID: {hir_id}");
            let target_id: Option<hir::Id> =
                if let Some(hir_expr) = db.find_expression(hir_id.clone()) {
                    let containing_body = db.containing_body_of(hir_id.clone());
                    if containing_body.identifiers.contains_key(&hir_id) {
                        // A local variable was declared. Find references to that variable.
                        Some(hir_id)
                    } else {
                        // An intermediate reference. Find references to its target.
                        match hir_expr {
                            Expression::Reference(target_id) => Some(target_id),
                            Expression::Symbol(_) => {
                                // TODO: Handle struct access
                                None
                            }
                            Expression::Error { .. } => None,
                            _ => panic!("Expected a reference, got {hir_expr}."),
                        }
                    }
                } else {
                    // Parameter
                    Some(hir_id)
                };
            target_id.map(ReferenceQuery::Id)
        }
        CstKind::Symbol(symbol) => Some(ReferenceQuery::Symbol(module, symbol)),
        CstKind::Int { value, .. } => Some(ReferenceQuery::Int(module, value)),
        _ => None,
    };
    let query = query.map(|it| (it, origin_cst.data.span));
    debug!("Reference query: {query:?}");
    query
}

fn find_references<DB>(db: &DB, query: ReferenceQuery, include_declaration: bool) -> Vec<Reference>
where
    DB: AstToHir + HirDb + PositionConversionDb,
{
    // TODO: search all files
    let module = match &query {
        ReferenceQuery::Id(id) => id.module.clone(),
        ReferenceQuery::Int(module, _) => module.clone(),
        ReferenceQuery::Symbol(module, _) => module.clone(),
        ReferenceQuery::Needs(module) => module.clone(),
    };
    let (hir, _) = db.hir(module).unwrap();

    let mut context = Context::new(db, query, include_declaration);
    context.visit_body(hir.as_ref());
    context.references
}

struct Context<'a, DB: PositionConversionDb + ?Sized> {
    db: &'a DB,
    query: ReferenceQuery,
    include_declaration: bool,
    discovered_references: FxHashSet<hir::Id>,
    references: Vec<Reference>,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReferenceQuery {
    Id(hir::Id),
    Int(Module, BigUint),
    Symbol(Module, String),
    Needs(Module),
}
impl<'a, DB> Context<'a, DB>
where
    DB: PositionConversionDb + HirDb + ?Sized,
{
    fn new(db: &'a DB, query: ReferenceQuery, include_declaration: bool) -> Self {
        Self {
            db,
            query,
            include_declaration,
            discovered_references: FxHashSet::default(),
            references: vec![],
        }
    }

    fn visit_body(&mut self, body: &Body) {
        if let ReferenceQuery::Id(id) = &self.query.clone() {
            if body.identifiers.contains_key(id) {
                self.add_reference(id.clone(), true);
            }
        }
        for (id, expression) in &body.expressions {
            self.visit_expression(id.clone(), expression);
        }
    }
    fn visit_ids(&mut self, ids: &[hir::Id]) {
        for id in ids {
            self.visit_id(id.clone());
        }
    }
    fn visit_id(&mut self, id: hir::Id) {
        let Some(expression) = self.db.find_expression(id.clone()) else {
            // Generated code
            return;
        };
        self.visit_expression(id, &expression);
    }
    fn visit_expression(&mut self, id: hir::Id, expression: &Expression) {
        match expression {
            Expression::Int(int) => {
                if let ReferenceQuery::Int(_, target) = &self.query
                    && int == target
                {
                    self.add_reference(id, false);
                }
            }
            Expression::Text(_) => {}
            Expression::Reference(target) => {
                if let ReferenceQuery::Id(target_id) = &self.query
                    && target == target_id
                {
                    self.add_reference(id, false);
                }
            }
            Expression::Symbol(symbol) => {
                if let ReferenceQuery::Symbol(_, target) = &self.query
                    && symbol == target
                {
                    self.add_reference(id, false);
                }
            }
            Expression::List(_)
            | Expression::Struct(_)
            | Expression::Destructure { .. }
            | Expression::PatternIdentifierReference(_) => {}
            Expression::Match { cases, .. } => {
                for MatchCase{condition, body, ..} in cases {
                    if let Some(condition) = condition {
                        self.visit_body(condition);
                    }
                    self.visit_body(body);
                }
            }
            Expression::Function(Function { body, .. }) => {
                // We don't need to visit the parameters: They can only be the
                // declaration of an identifier and don't reference it any other
                // way. Therfore, we already visit them in [visit_body].
                self.visit_body(body);
            }
            Expression::Builtin(_) => {}
            Expression::Call {
                function,
                arguments,
            } => {
                if let ReferenceQuery::Id(target_id) = &self.query
                    && function == target_id
                {
                    self.add_reference(id, false);
                }
                self.visit_ids(arguments);
            }
            Expression::UseModule { .. } => {} // only occurs in generated code
            Expression::Needs { .. } => {
                if let ReferenceQuery::Needs(_) = &self.query {
                    self.add_reference(id, false);
                }
            }
            Expression::Error { .. } => {}
        }
    }

    fn add_reference(&mut self, id: hir::Id, is_write: bool) {
        if let ReferenceQuery::Id(target_id) = &self.query {
            if &id == target_id && !self.include_declaration {
                return;
            }
        }

        if self.discovered_references.contains(&id) {
            return;
        }
        self.discovered_references.insert(id.clone());

        if let Some(span) = self.db.hir_id_to_span(&id) {
            self.references.push(Reference {
                range: self.db.range_to_lsp_range(id.module, span),
                is_write,
            });
        }
    }
}
