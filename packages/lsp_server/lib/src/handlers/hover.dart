import 'dart:async';

import 'package:compiler/compiler.dart';

import '../analysis_server.dart';
import '../generated/lsp_protocol/protocol_generated.dart';
import '../generated/lsp_protocol/protocol_special.dart';
import '../utils.dart';
import 'handlers.dart';

class HoverHandler extends MessageHandler<TextDocumentPositionParams, Hover> {
  HoverHandler(AnalysisServer server) : super(server);

  @override
  Method get handlesMessage => Method.textDocument_hover;
  @override
  LspJsonHandler<TextDocumentPositionParams> get jsonHandler =>
      TextDocumentPositionParams.jsonHandler;

  @override
  Future<ErrorOr<Hover>> handle(
    TextDocumentPositionParams params,
    CancellationToken token,
  ) async {
    final resourceId = server.fileUriToResourceId(params.textDocument.uri);

    final fileAst = server.queryConfig.callQuery(getAst, resourceId);
    if (fileAst.second.isNotEmpty) {
      return error(
        ErrorCodes.InternalError,
        "Couldn't parse AST of `$resourceId`.",
      );
    }

    final astNodeResult =
        getAstNodeAtPosition(server, resourceId, params.position);
    if (astNodeResult is Error) {
      return error(ErrorCodes.InternalError, astNodeResult.error);
    }
    final astNode = astNodeResult.value;
    var content = 'AST: $astNode';

    final expressionHirResult =
        getExpressionHirAtPosition(server, resourceId, params.position);
    if (expressionHirResult is Error) {
      return error(ErrorCodes.InternalError, expressionHirResult.error);
    }
    final expressionHir = expressionHirResult.value;
    if (expressionHir is Some) {
      content = '```candy\n'
          '${expressionHir.value.type}\n'
          '```\n'
          '\n'
          '---\n'
          '\n'
          'HIR: ${expressionHir.value}\n'
          '\n'
          '---\n'
          '\n'
          '$content';
    }

    final range = astNode.span.toRange(server, resourceId);
    return success(Hover(
      Either2.t2(MarkupContent(MarkupKind.Markdown, content)),
      range,
    ));
  }
}
