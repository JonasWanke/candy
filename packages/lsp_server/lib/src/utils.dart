import 'package:compiler/compiler.dart';
import 'package:compiler/compiler.dart' as hir;
import 'package:parser/parser.dart' hide Token;
import 'package:parser/parser.dart' as ast;
import 'package:petitparser/petitparser.dart' hide Result;

import 'analysis_server.dart';
import 'generated/lsp_protocol/protocol_generated.dart';

bool isCandyDocument(String uri) => uri.endsWith('.candy');

extension PositionToOffset on Position {
  int toOffset(AnalysisServer server, ResourceId resourceId) {
    final context = QueryContext(GlobalQueryContext(server.queryConfig));
    final source = server.resourceProvider.getContent(context, resourceId);

    final lineOffset = line == 0
        ? 0
        : Token.newlineParser().token().matchesSkipping(source)[line - 1].stop;
    return lineOffset + character;
  }
}

extension OffsetToPosition on int {
  Position toPosition(AnalysisServer server, ResourceId resourceId) {
    final context = QueryContext(GlobalQueryContext(server.queryConfig));
    final source = server.resourceProvider.getContent(context, resourceId);

    var line = 0;
    var column = 0;
    for (final lineToken
        in Token.newlineParser().token().matchesSkipping(source)) {
      if (this < lineToken.stop) return Position(line, this - column);

      line++;
      column = lineToken.stop;
    }
    return Position(line, this - column);
  }
}

extension SourceSpanToRange on SourceSpan {
  Range toRange(AnalysisServer server, ResourceId resourceId) {
    return Range(
      start.toPosition(server, resourceId),
      end.toPosition(server, resourceId),
    );
  }
}

extension ErrorLocationConversion on ErrorLocation {
  Location toLocation(AnalysisServer server) =>
      Location(server.resourceIdToFileUri(resourceId), toRange(server));
  Range toRange(AnalysisServer server) {
    return span == null
        ? Range(Position(0, 0), Position(0, 0))
        : Range(
            span.start.toPosition(server, resourceId),
            span.end.toPosition(server, resourceId),
          );
  }
}

Result<ast.AstNode, String> getAstNodeAtPosition(
  AnalysisServer server,
  ResourceId resourceId,
  Position position,
) {
  final fileAst = server.queryConfig.callQuery(getAst, resourceId);
  if (fileAst.second.isNotEmpty) {
    return Error(
      "Couldn't parse AST of `$resourceId`: ${fileAst.second}",
    );
  }

  final offset = position.toOffset(server, resourceId);
  return Ok(NodeFinderVisitor.find(fileAst.first.value, offset));
}

Result<Option<hir.Expression>, String> getExpressionHirAtPosition(
  AnalysisServer server,
  ResourceId resourceId,
  Position position,
) {
  final astNodeResult = getAstNodeAtPosition(server, resourceId, position);
  if (astNodeResult is Error) return Error(astNodeResult.error);
  final astNode = astNodeResult.value;
  if (astNode is! ast.Expression) return Ok(None());
  final expressionAst = astNode as ast.Expression;

  final nodeHirResult = server.queryConfig.callQuery(
    getExpressionFromAstId,
    Tuple2(resourceId, expressionAst.id),
  );
  if (nodeHirResult.second.isNotEmpty) {
    return Error(
      "Couldn't get HIR of expression ${expressionAst.id}: ${nodeHirResult.second}",
    );
  }
  final nodeHir = nodeHirResult.first.value;
  return Ok(nodeHir);
}

/// Combines the [Object.hashCode] values of an arbitrary number of objects
/// from an [Iterable] into one value. This function will return the same
/// value if given `null` as if given an empty list.
// Borrowed from dart:ui.
int hashList(Iterable<Object> arguments) {
  var result = 0;
  if (arguments != null) {
    for (final argument in arguments) {
      var hash = result;
      hash = 0x1fffffff & (hash + argument.hashCode);
      hash = 0x1fffffff & (hash + ((0x0007ffff & hash) << 10));
      result = hash ^ (hash >> 6);
    }
  }
  result = 0x1fffffff & (result + ((0x03ffffff & result) << 3));
  result = result ^ (result >> 11);
  return 0x1fffffff & (result + ((0x00003fff & result) << 15));
}
