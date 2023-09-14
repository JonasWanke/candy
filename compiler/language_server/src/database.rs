use candy_frontend::{
    ast::AstDbStorage,
    ast_to_hir::AstToHirStorage,
    cst::CstDbStorage,
    cst_to_ast::CstToAstStorage,
    hir::HirDbStorage,
    hir_to_mir::HirToMirStorage,
    lir_optimize::OptimizeLirStorage,
    mir_optimize::OptimizeMirStorage,
    mir_to_lir::MirToLirStorage,
    module::{
        FileSystemModuleProvider, GetModuleContentQuery, InMemoryModuleProvider, Module,
        ModuleDbStorage, ModuleProvider, ModuleProviderOwner, MutableModuleProviderOwner,
        OverlayModuleProvider, PackagesPath,
    },
    position::PositionConversionStorage,
    rcst_to_cst::RcstToCstStorage,
    string_to_rcst::StringToRcstStorage,
};

#[salsa::database(
    AstDbStorage,
    AstToHirStorage,
    CstDbStorage,
    CstToAstStorage,
    HirDbStorage,
    HirToMirStorage,
    MirToLirStorage,
    ModuleDbStorage,
    OptimizeMirStorage,
    OptimizeLirStorage,
    PositionConversionStorage,
    RcstToCstStorage,
    StringToRcstStorage
)]
pub struct Database {
    storage: salsa::Storage<Self>,
    pub packages_path: PackagesPath,
    module_provider: OverlayModuleProvider<InMemoryModuleProvider, Box<dyn ModuleProvider + Send>>,
}
impl salsa::Database for Database {}

impl Database {
    #[must_use]
    pub fn new_with_file_system_module_provider(packages_path: PackagesPath) -> Self {
        Self::new(
            packages_path.clone(),
            Box::new(FileSystemModuleProvider { packages_path }),
        )
    }

    #[must_use]
    pub fn new(
        packages_path: PackagesPath,
        module_provider: Box<dyn ModuleProvider + Send>,
    ) -> Self {
        Self {
            storage: salsa::Storage::default(),
            packages_path,
            module_provider: OverlayModuleProvider::new(
                InMemoryModuleProvider::default(),
                module_provider,
            ),
        }
    }
}

impl ModuleProviderOwner for Database {
    fn get_module_provider(&self) -> &dyn ModuleProvider {
        &self.module_provider
    }
}
impl MutableModuleProviderOwner for Database {
    fn get_in_memory_module_provider(&mut self) -> &mut InMemoryModuleProvider {
        &mut self.module_provider.overlay
    }
    fn invalidate_module(&mut self, module: &Module) {
        GetModuleContentQuery.in_db_mut(self).invalidate(module);
    }
}
