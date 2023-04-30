use crate::{
    database::Database,
    utils::{module_for_path, packages_path},
    CandyDebugOptions, Exit, ProgramResult,
};
use candy_frontend::{
    ast_to_hir::AstToHir,
    cst_to_ast::CstToAst,
    hir_to_mir::HirToMir,
    mir_optimize::OptimizeMir,
    position::Offset,
    rich_ir::{RichIr, RichIrAnnotation, TokenType},
    string_to_rcst::StringToRcst,
};
use candy_vm::{lir::RichIrForLir, mir_to_lir::MirToLir};
use colored::Colorize;

pub(crate) fn debug(options: CandyDebugOptions) -> ProgramResult {
    let packages_path = packages_path();
    let db = Database::new_with_file_system_module_provider(packages_path);

    let rich_ir = match options {
        CandyDebugOptions::Cst(options) => {
            let module = module_for_path(options.path)?;
            let rcst = db.rcst(module.clone());
            RichIr::for_rcst(&module, &rcst)
        }
        CandyDebugOptions::Ast(options) => {
            let module = module_for_path(options.path)?;
            let ast = db.ast(module.clone());
            ast.map(|(ast, _)| RichIr::for_ast(&module, &ast))
        }
        CandyDebugOptions::Hir(options) => {
            let module = module_for_path(options.path)?;
            let hir = db.hir(module.clone());
            hir.map(|(hir, _)| RichIr::for_hir(&module, &hir))
        }
        CandyDebugOptions::Mir(options) => {
            let module = module_for_path(options.path.clone())?;
            let tracing = options.to_tracing_config();
            let mir = db.mir(module.clone(), tracing.clone());
            mir.map(|mir| RichIr::for_mir(&module, &mir, &tracing))
        }
        CandyDebugOptions::OptimizedMir(options) => {
            let module = module_for_path(options.path.clone())?;
            let tracing = options.to_tracing_config();
            let mir = db.mir_with_obvious_optimized(module.clone(), tracing.clone());
            mir.map(|mir| RichIr::for_mir(&module, &mir, &tracing))
        }
        CandyDebugOptions::Lir(options) => {
            let module = module_for_path(options.path.clone())?;
            let tracing = options.to_tracing_config();
            let lir = db.lir(module.clone(), tracing.clone());
            lir.map(|lir| RichIr::for_lir(&module, &lir, &tracing))
        }
    };

    let Some(rich_ir) = rich_ir else {
        return Err(Exit::FileNotFound);
    };

    let bytes = rich_ir.text.as_bytes().to_vec();
    let annotations = rich_ir.annotations.iter();
    let mut displayed_byte = Offset(0);

    for RichIrAnnotation {
        range, token_type, ..
    } in annotations
    {
        if range.start < displayed_byte {
            continue;
        }
        let before_annotation = std::str::from_utf8(&bytes[*displayed_byte..*range.start]).unwrap();
        print!("{before_annotation}");

        let in_annotation = std::str::from_utf8(&bytes[*range.start..*range.end]).unwrap();

        if let Some(token_type) = token_type {
            let color = match token_type {
                TokenType::Module => Color::Yellow,
                TokenType::Parameter => Color::Red,
                TokenType::Variable => Color::Yellow,
                TokenType::Symbol => Color::Magenta,
                TokenType::Function => Color::Blue,
                TokenType::Comment => Color::Green,
                TokenType::Text => Color::Cyan,
                TokenType::Int => Color::Red,
            };
            print!("{}", in_annotation.color(color));
        } else {
            print!("{}", in_annotation)
        }

        displayed_byte = range.end;
    }
    let rest = std::str::from_utf8(&bytes[*displayed_byte..]).unwrap();
    println!("{rest}");

    Ok(())
}
