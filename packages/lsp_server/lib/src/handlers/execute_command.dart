import 'dart:async';

import 'package:compiler/compiler.dart';
import 'package:compiler_dart/compiler_dart.dart' as dart;

import '../analysis_server.dart';
import '../error_codes.dart';
import '../generated/lsp_protocol/protocol_generated.dart';
import '../generated/lsp_protocol/protocol_special.dart';
import 'handlers.dart';

abstract class Commands {
  static const all = [build, run];
  static const build = 'build';
  static const run = 'run';
}

class ExecuteCommandHandler
    extends MessageHandler<ExecuteCommandParams, Object> {
  ExecuteCommandHandler(AnalysisServer server)
      : commandHandlers = {
          Commands.build: BuildCommandHandler(server),
          Commands.run: RunCommandHandler(server),
        },
        super(server);

  final Map<String, CommandHandler<ExecuteCommandParams, Object>>
      commandHandlers;

  @override
  Method get handlesMessage => Method.workspace_executeCommand;

  @override
  LspJsonHandler<ExecuteCommandParams> get jsonHandler =>
      ExecuteCommandParams.jsonHandler;

  @override
  Future<ErrorOr<Object>> handle(
      ExecuteCommandParams params, CancellationToken token) async {
    final handler = commandHandlers[params.command];
    if (handler == null) {
      return error(
        ServerErrorCodes.UnknownCommand,
        '${params.command} is not a valid command identifier',
        null,
      );
    }
    return handler.handle(params.arguments);
  }
}

class BuildCommandHandler extends CommandHandler<ExecuteCommandParams, Object> {
  BuildCommandHandler(AnalysisServer server) : super(server);

  String get commandName => 'Build';

  @override
  Future<ErrorOr<void>> handle(List<dynamic> arguments) async {
    return _build(server) ?? success();
  }
}

class RunCommandHandler extends CommandHandler<ExecuteCommandParams, Object> {
  RunCommandHandler(AnalysisServer server) : super(server);

  String get commandName => 'Run';

  @override
  Future<ErrorOr<void>> handle(List<dynamic> arguments) async {
    final buildResult = _build(server);
    if (buildResult != null) return buildResult;

    final result = server.queryConfig.callQuery(dart.run, Unit());
    if (result.second.isNotEmpty) {
      return error(
        ErrorCodes.InternalError,
        'Failed to run Dart program.',
        result.second.join(', '),
      );
    } else {
      server.sendLogMessage(result.first.value, MessageType.Log);
    }
    return success();
  }
}

ErrorOr<T> _build<T>(AnalysisServer server) {
  final result = server.queryConfig.callQuery(dart.compile, Unit());
  if (result.second.isNotEmpty) {
    return error(
      ErrorCodes.InternalError,
      'Failed to build to Dart.',
      result.second.join(', '),
    );
  } else {
    server.sendLogMessage('Build succeeded 🎉');
  }
  return null;
}
