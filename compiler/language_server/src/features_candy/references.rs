use crate::{features::Reference, utils::LspPositionConversion};
use candy_frontend::{
    ast_to_hir::AstToHir,
    cst::{CstDb, CstKind},
    hir::{self, Body, Expression, HirDb, Lambda},
    module::{Module, ModuleDb},
    position::{Offset, PositionConversionDb},
    rich_ir::ToRichIr,
};
use num_bigint::BigUint;
use rustc_hash::FxHashSet;
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
    let Some(query) = query_for_offset(db, module, offset) else { return vec![]; };
    find_references(db, query, include_declaration)
}

fn query_for_offset<DB: CstDb>(db: &DB, module: Module, offset: Offset) -> Option<ReferenceQuery>
where
    DB: HirDb,
{
    let origin_cst = db.find_cst_by_offset(module.clone(), offset);
    info!("Finding references for {origin_cst:?}");
    let query = match origin_cst.kind {
        CstKind::Identifier(identifier) if identifier == "needs" => {
            Some(ReferenceQuery::Needs(module))
        }
        CstKind::Identifier { .. } => {
            let hir_id = db.cst_to_hir_id(module, origin_cst.data.id)?;
            let target_id = if let Some(hir_expr) = db.find_expression(hir_id.clone()) {
                let containing_body = db.containing_body_of(hir_id.clone());
                if containing_body.identifiers.contains_key(&hir_id) {
                    // A local variable was declared. Find references to that variable.
                    hir_id
                } else {
                    // An intermediate reference. Find references to its target.
                    match hir_expr {
                        Expression::Reference(target_id) => target_id,
                        Expression::Symbol(_) => {
                            // TODO: Handle struct access
                            return None;
                        }
                        Expression::Error { .. } => return None,
                        _ => panic!("Expected a reference, got {}.", hir_expr.to_rich_ir().text),
                    }
                }
            } else {
                // Parameter
                hir_id
            };
            Some(ReferenceQuery::Id(target_id))
        }
        CstKind::Symbol(symbol) => Some(ReferenceQuery::Symbol(module, symbol)),
        CstKind::Int { value, .. } => Some(ReferenceQuery::Int(module, value)),
        _ => None,
    };
    debug!("Reference query: {query:?}");
    query
}

fn find_references<DB: AstToHir + HirDb + PositionConversionDb>(
    db: &DB,
    query: ReferenceQuery,
    include_declaration: bool,
) -> Vec<Reference> {
    // TODO: search all files
    let module = match &query {
        ReferenceQuery::Id(id) => id.module.clone(),
        ReferenceQuery::Int(module, _) => module.to_owned(),
        ReferenceQuery::Symbol(module, _) => module.to_owned(),
        ReferenceQuery::Needs(module) => module.to_owned(),
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
            self.visit_expression(id.to_owned(), expression);
        }
    }
    fn visit_ids(&mut self, ids: &[hir::Id]) {
        for id in ids {
            self.visit_id(id.to_owned());
        }
    }
    fn visit_id(&mut self, id: hir::Id) {
        let expression = match self.db.find_expression(id.to_owned()) {
            Some(expression) => expression,
            None => return, // Generated code
        };
        self.visit_expression(id, &expression);
    }
    fn visit_expression(&mut self, id: hir::Id, expression: &Expression) {
        match expression {
            Expression::Int(int) =>{
                if let ReferenceQuery::Int(_, target) = &self.query && int == target {
                    self.add_reference(id, false);
                }
            },
            Expression::Text(_) => {},
            Expression::Reference(target) => {
                if let ReferenceQuery::Id(target_id) = &self.query && target == target_id {
                    self.add_reference(id, false);
                }
            }
            Expression::Symbol(symbol) => {
                if let ReferenceQuery::Symbol(_, target) = &self.query && symbol == target {
                    self.add_reference(id, false);
                }
            }
            Expression::List(_)
            | Expression::Struct(_)
            | Expression::Destructure { .. }
            | Expression::PatternIdentifierReference (_) => {},
            Expression::Match { cases, .. } => {
                for (_, body) in cases {
                    self.visit_body(body);
                }
            },
            Expression::Lambda(Lambda { body, .. }) => {
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
                if let ReferenceQuery::Id(target_id) = &self.query && function == target_id {
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
            Expression::Error { child, .. } => {
                if let Some(child) = child {
                    self.visit_id(child.clone());
                }
            }
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

        if let Some(span) = self.db.hir_id_to_span(id.clone()) {
            self.references.push(Reference {
                range: self.db.range_to_lsp_range(id.module, span),
                is_write,
            });
        }
    }
}
