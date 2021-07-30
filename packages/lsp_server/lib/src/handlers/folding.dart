import 'dart:async';

import 'package:compiler/compiler.dart';
import 'package:parser/parser.dart' as ast;

import '../analysis_server.dart';
import '../generated/lsp_protocol/protocol_generated.dart';
import '../generated/lsp_protocol/protocol_special.dart';
import '../utils.dart';
import 'handlers.dart';

class FoldingHandler
    extends MessageHandler<FoldingRangeParams, List<FoldingRange>> {
  FoldingHandler(AnalysisServer server) : super(server);

  @override
  Method get handlesMessage => Method.textDocument_foldingRange;
  @override
  LspJsonHandler<FoldingRangeParams> get jsonHandler =>
      FoldingRangeParams.jsonHandler;

  @override
  Future<ErrorOr<List<FoldingRange>>> handle(
    FoldingRangeParams params,
    CancellationToken token,
  ) async {
    final resourceId = server.fileUriToResourceId(params.textDocument.uri);

    final fileAst = server.queryConfig.callQuery(getAst, resourceId);
    if (fileAst.second.isNotEmpty) {
      return error(
        ErrorCodes.InternalError,
        "Couldn't parse AST of `$resourceId`: ${fileAst.second}",
      );
    }

    final foldingRanges =
        _FoldingAstVisitor.visit(server, resourceId, fileAst.first.value);
    return success(foldingRanges);
  }
}

class _FoldingAstVisitor extends ast.TraversingAstVisitor {
  _FoldingAstVisitor._(this.server, this.resourceId)
      : assert(server != null),
        assert(resourceId != null);
  static List<FoldingRange> visit(
    AnalysisServer server,
    ResourceId resourceId,
    ast.CandyFile fileAst,
  ) {
    final visitor = _FoldingAstVisitor._(server, resourceId);
    fileAst.accept(visitor);
    return visitor._ranges;
  }

  final AnalysisServer server;
  final ResourceId resourceId;

  final _ranges = <FoldingRange>[];
  void _addRange(
    ast.SourceSpan span, [
    FoldingRangeKind kind = FoldingRangeKind.Region,
  ]) {
    final range = span.toRange(server, resourceId);
    _ranges.add(FoldingRange(
      range.start.line,
      range.start.character,
      range.end.line,
      range.end.character,
      kind,
    ));
  }

  @override
  void visitCandyFile(ast.CandyFile node) {
    if (node.useLines.isNotEmpty) {
      _addRange(
        ast.SourceSpan(
          node.useLines.first.span.start,
          node.useLines.last.span.end,
        ),
        FoldingRangeKind.Imports,
      );
    }
    super.visitCandyFile(node);
  }

  @override
  void visitBlockDeclarationBody(ast.BlockDeclarationBody node) {
    if (node.leftBrace.span != null) _addRange(node.span);
    super.visitBlockDeclarationBody(node);
  }

  @override
  void visitLambdaLiteral(ast.LambdaLiteral node) {
    _addRange(node.span);
    super.visitLambdaLiteral(node);
  }
}
