use candy_frontend::{
    ast::AstDbStorage,
    ast_to_hir::AstToHirStorage,
    cst::CstDbStorage,
    cst_to_ast::CstToAstStorage,
    hir::{self, HirDbStorage},
    hir_to_mir::HirToMirStorage,
    mir_optimize::OptimizeMirStorage,
    module::{
        GetModuleContentQuery, InMemoryModuleProvider, Module, ModuleDbStorage, ModuleKind,
        ModuleProvider, ModuleProviderOwner, MutableModuleProviderOwner, Package,
    },
    position::PositionConversionStorage,
    rcst_to_cst::RcstToCstStorage,
    string_to_rcst::StringToRcstStorage,
    TracingConfig,
};
use candy_vm::{
    heap::{Heap, HirId, InlineObject, Struct},
    lir::Lir,
    mir_to_lir::compile_lir,
    tracer::DummyTracer,
    PopulateInMemoryProviderFromFileSystem, Vm, VmFinished,
};
use lazy_static::lazy_static;
use rustc_hash::FxHashMap;
use std::borrow::Borrow;
use tracing::warn;

const TRACING: TracingConfig = TracingConfig::off();
lazy_static! {
    static ref PACKAGE: Package = Package::User("/".into());
    static ref MODULE: Module = Module {
        package: PACKAGE.clone(),
        path: vec!["benchmark".to_string()],
        kind: ModuleKind::Code,
    };
}

#[salsa::database(
    AstDbStorage,
    AstToHirStorage,
    CstDbStorage,
    CstToAstStorage,
    HirDbStorage,
    HirToMirStorage,
    ModuleDbStorage,
    OptimizeMirStorage,
    PositionConversionStorage,
    RcstToCstStorage,
    StringToRcstStorage
)]
#[derive(Default)]
pub struct Database {
    storage: salsa::Storage<Self>,
    module_provider: InMemoryModuleProvider,
}
impl salsa::Database for Database {}
impl ModuleProviderOwner for Database {
    fn get_module_provider(&self) -> &dyn ModuleProvider {
        &self.module_provider
    }
}
impl MutableModuleProviderOwner for Database {
    fn get_in_memory_module_provider(&mut self) -> &mut InMemoryModuleProvider {
        &mut self.module_provider
    }
    fn invalidate_module(&mut self, module: &Module) {
        GetModuleContentQuery.in_db_mut(self).invalidate(module);
    }
}

pub fn setup_and_compile(source_code: &str) -> Lir {
    let mut db = setup();
    compile(&mut db, source_code)
}

pub fn setup() -> Database {
    let mut db = Database::default();
    db.module_provider.load_package_from_file_system("Builtins");
    db.module_provider.load_package_from_file_system("Core");
    db.module_provider.add_str(&MODULE, r#"_ = use "Core""#);

    // Load `Core` into the cache.
    let errors = compile_lir(&db, MODULE.clone(), TRACING.clone()).1;
    if !errors.is_empty() {
        for error in errors.iter() {
            warn!("{}", error.to_string_with_location(&db));
        }
        panic!("There are errors in the benchmarking code.");
    }

    db
}

pub fn compile(db: &mut Database, source_code: &str) -> Lir {
    db.did_open_module(&MODULE, source_code.as_bytes().to_owned());
    compile_lir(db, MODULE.clone(), TRACING.clone()).0
}

pub fn run(lir: impl Borrow<Lir>) -> (Heap, InlineObject) {
    let VmFinished {
        mut heap,
        tracer,
        result,
    } = Vm::for_module(lir.borrow(), DummyTracer).run_forever_without_handles();
    let main = result
        .expect("Module panicked.")
        .into_main_function(&lir.borrow().symbol_table)
        .unwrap();

    // Run the `main` function.
    let environment = Struct::create(&mut heap, true, &FxHashMap::default());
    let responsible = HirId::create(&mut heap, true, hir::Id::user());
    let VmFinished { heap, result, .. } = Vm::for_function(
        lir,
        heap,
        main,
        &[environment.into(), responsible.into()],
        tracer,
    )
    .run_forever_without_handles();
    match result {
        Ok(return_value) => (heap, return_value),
        Err(panic) => {
            panic!("The main function panicked: {}", panic.reason)
        }
    }
}
