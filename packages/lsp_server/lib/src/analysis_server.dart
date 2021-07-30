import 'dart:async';
import 'dart:io';

import 'package:analyzer/exception/exception.dart';
import 'package:collection/collection.dart';
import 'package:compiler/compiler.dart';
import 'package:dartx/dartx.dart' hide Range;
import 'package:path/path.dart' as p;

import 'channel.dart';
import 'error_codes.dart';
import 'generated/lsp_protocol/protocol_generated.dart';
import 'generated/lsp_protocol/protocol_special.dart';
import 'handlers/handlers.dart';
import 'handlers/states.dart';
import 'overlay_resource_provider.dart';
import 'type_labels.dart';
import 'utils.dart';

class AnalysisServer {
  // Section: Lifecycle

  /// Initialize a newly created server to send and receive messages to the
  /// given [channel].
  AnalysisServer(this.channel, this.candyDirectory)
      : assert(channel != null),
        assert(candyDirectory != null) {
    messageHandler = UninitializedStateMessageHandler(this);

    channel.listen(
      _handleMessage,
      onDone: () {},
      onError: sendSocketErrorNotification,
    );
    sendLogMessage('Started Candy LSP');
  }

  /// The channel from which messages are received and to which responses should
  /// be sent.
  final LspServerCommunicationChannel channel;
  final Directory candyDirectory;

  ServerStateMessageHandler messageHandler;

  ClientCapabilities _clientCapabilities;

  /// The capabilities of the LSP client. Will be null prior to initialization.
  ClientCapabilities get clientCapabilities => _clientCapabilities;

  LspInitializationOptions _initializationOptions;

  /// Initialization options provided by the LSP client. Allows opting in/out of
  /// specific server functionality. Will be null prior to initialization.
  LspInitializationOptions get initializationOptions => _initializationOptions;
  void handleClientConnection(
    ClientCapabilities capabilities,
    dynamic initializationOptions,
    Directory projectDirectory,
  ) {
    _clientCapabilities = capabilities;
    _initializationOptions = LspInitializationOptions(initializationOptions);
    _projectDirectory = projectDirectory;
    _resourceProvider = OverlayResourceProvider(
      ResourceProvider.default_(
        candyDirectory: candyDirectory,
        projectDirectory: projectDirectory,
      ),
    );
    _queryConfig = QueryConfig(
      packageName: p.basename(projectDirectory.absolute.path),
      resourceProvider: resourceProvider,
      buildArtifactManager: BuildArtifactManager(projectDirectory),
    );
  }

  /// Capabilities of the server. Will be null prior to initialization as
  /// the server capabilities depend on the client capabilities.
  ServerCapabilities capabilities;

  /// Whether or not the server is controlling the shutdown and will exit
  /// automatically.
  bool willExit = false;

  Future<void> get exited => channel.closed;
  Future<void> shutdown() {
    // Defer closing the channel so that the shutdown response can be sent and
    // logged.
    Future(channel.close);

    return Future.value();
  }

  // Section: Notifications

  int nextRequestId = 1;
  final completers = <int, Completer<ResponseMessage>>{};

  void sendMessageToUser(MessageType type, String message) {
    sendNotification(NotificationMessage(
      Method.window_showMessage,
      ShowMessageParams(type, message),
      jsonRpcVersion,
    ));
  }

  void sendErrorMessageToUser(String message) =>
      sendMessageToUser(MessageType.Error, message);

  void sendLogMessage(String message, [MessageType type = MessageType.Info]) {
    sendNotification(NotificationMessage(
      Method.window_logMessage,
      LogMessageParams(type, message),
      jsonRpcVersion,
    ));
  }

  void sendNotification(NotificationMessage notification) =>
      channel.sendNotification(notification);

  // Section: Requests & Responses

  Future<ResponseMessage> sendRequest(Method method, Object params) {
    final requestId = nextRequestId++;
    final completer = Completer<ResponseMessage>();
    completers[requestId] = completer;

    channel.sendRequest(RequestMessage(
      Either2<num, String>.t1(requestId),
      method,
      params,
      jsonRpcVersion,
    ));

    return completer.future;
  }

  void sendResponse(ResponseMessage response) => channel.sendResponse(response);
  void sendErrorResponse(Message message, ResponseError<dynamic> error) {
    if (message is RequestMessage) {
      channel.sendResponse(
        ResponseMessage(message.id, null, error, jsonRpcVersion),
      );
    } else if (message is ResponseMessage) {
      // For bad response messages where we can't respond with an error, send it
      // as show instead of log.
      sendErrorMessageToUser(error.message);
    } else {
      // For notifications where we couldn't respond with an error, send it as
      // show instead of log.
      sendErrorMessageToUser(error.message);
    }

    // Handle fatal errors where the client/server state is out of sync and we
    // should not continue.
    if (error.code == ServerErrorCodes.ClientServerInconsistentState) {
      // Do not process any further messages.
      messageHandler = FailureStateMessageHandler(this);

      logErrorToClient(
        'An unrecoverable error occurred.\n\n'
        '${error.message}\n\n${error.code}\n\n${error.data}',
      );
      shutdown();
    }
  }

  /// Handle a [message] that was read from the communication channel.
  void _handleMessage(Message message) {
    runZonedGuarded(() async {
      try {
        if (message is ResponseMessage) {
          _handleClientResponse(message);
        } else if (message is RequestMessage) {
          final result = await messageHandler.handleMessage(message);
          if (result.isError) {
            sendErrorResponse(message, result.error);
          } else {
            channel.sendResponse(ResponseMessage(
              message.id,
              result.result,
              null,
              jsonRpcVersion,
            ));
          }
        } else if (message is NotificationMessage) {
          final result = await messageHandler.handleMessage(message);
          if (result.isError) {
            sendErrorResponse(message, result.error);
          }
        } else {
          sendErrorMessageToUser('Unknown message type');
        }
      } catch (error, stackTrace) {
        final errorMessage = message is ResponseMessage
            ? 'An error occurred while handling the response to request ${message.id}'
            : message is RequestMessage
                ? 'An error occurred while handling ${message.method} request'
                : message is NotificationMessage
                    ? 'An error occurred while handling ${message.method} notification'
                    : 'Unknown message type';
        sendErrorResponse(
          message,
          ResponseError<dynamic>(
            ServerErrorCodes.UnhandledError,
            errorMessage,
            null,
          ),
        );
        logException(errorMessage, error, stackTrace);
      }
    }, sendSocketErrorNotification);
  }

  /// Handles a response from the client by invoking the completer that the
  /// outbound request created.
  void _handleClientResponse(ResponseMessage message) {
    // The ID from the client is an Either2<num, String>, though it's not valid
    // for it to be a string because it should match a request we sent to the
    // client (and we always use numeric IDs for outgoing requests).
    message.id.map(
      (id) {
        // It's possible that even if we got a numeric ID that it's not valid.
        // If it's not in our completers list (which is a list of the
        // outstanding requests we've sent) then show an error.
        final completer = completers[id];
        if (completer == null) {
          sendErrorMessageToUser('Response with ID $id was unexpected');
        } else {
          completers.remove(id);
          completer.complete(message);
        }
      },
      (stringID) {
        sendErrorMessageToUser('Unexpected String ID for response $stringID');
      },
    );
  }

  // Section: Error logging

  /// Logs the error on the client using window/logMessage.
  void logErrorToClient(String message) {
    channel.sendNotification(NotificationMessage(
      Method.window_logMessage,
      LogMessageParams(MessageType.Error, message),
      jsonRpcVersion,
    ));
  }

  /// Logs an exception by sending it to the client (window/logMessage) and
  /// recording it in a buffer on the server for diagnostics.
  void logException(String message, dynamic exception, StackTrace stackTrace) {
    if (exception is CaughtException) {
      stackTrace ??= exception.stackTrace;
      message = '$message: ${exception.exception}';
    } else if (exception != null) {
      message = '$message: $exception';
    }

    final fullError = stackTrace == null ? message : '$message\n$stackTrace';

    // Log the full message since showMessage above may be truncated or
    // formatted badly (eg. VS Code takes the newlines out).
    logErrorToClient(fullError);
  }

  void sendServerErrorNotification(
    String message,
    dynamic exception, {
    StackTrace stackTrace,
    bool fatal = false,
  }) {
    message = exception == null ? message : '$message: $exception';

    // Show message (without stack) to the user.
    sendErrorMessageToUser(message);

    logException(message, exception, stackTrace);
  }

  /// There was an error related to the socket from which messages are being
  /// read.
  void sendSocketErrorNotification(dynamic error, StackTrace stack) {
    // Don't send to instrumentation service; not an internal error.
    sendServerErrorNotification('Socket error', error, stackTrace: stack);
  }

  // Section: Analysis

  Directory _projectDirectory;
  Directory get projectDirectory => _projectDirectory;
  OverlayResourceProvider _resourceProvider;
  OverlayResourceProvider get resourceProvider => _resourceProvider;
  QueryConfig _queryConfig;
  QueryConfig get queryConfig => _queryConfig;

  Map<ResourceId, List<ReportedCompilerError>> _errors = {};
  void onFileChanged(ResourceId resourceId) {
    final result = queryConfig.callQuery(calculateFullHir, resourceId);
    _updateErrors(result.second.groupBy((e) => e.location?.resourceId));
    updateTypeLabels(this, resourceId);
  }

  void _updateErrors(Map<ResourceId, List<ReportedCompilerError>> errors) {
    sendLogMessage('New errors: $errors');
    final equality = DeepCollectionEquality.unordered();
    for (final resourceId in (_errors.keys + errors.keys).toSet()) {
      // TODO(JonasWanke): handle errors without a location
      if (resourceId == null) continue;

      final oldErrors = _errors[resourceId];
      final newErrors = errors[resourceId] ?? [];
      if (equality.equals(oldErrors, newErrors)) continue;

      final diagnostics = newErrors
          .where((e) => e.location.span != null)
          .map((e) => Diagnostic(
                e.location.toRange(this),
                DiagnosticSeverity.Error,
                e.error.id,
                'candy',
                e.message,
                [
                  for (final related in e.relatedInformation)
                    DiagnosticRelatedInformation(
                      related.location.toLocation(this),
                      related.message,
                    ),
                ],
              ))
          .toList();
      sendNotification(NotificationMessage(
        Method.textDocument_publishDiagnostics,
        PublishDiagnosticsParams(resourceIdToFileUri(resourceId), diagnostics),
        jsonRpcVersion,
      ));
    }
    _errors = errors;
  }

  ResourceId fileUriToResourceId(String uri) {
    final file = File.fromUri(Uri.parse(uri));
    assert(p.extension(file.path) == candyFileExtension);

    assert(p.isWithin(projectDirectory.path, file.path));

    final relativePath = p
        .relative(file.path, from: projectDirectory.path)
        .split('\\')
        .join('/');
    return ResourceId(queryConfig.packageId, relativePath);
  }

  String resourceIdToFileUri(ResourceId resourceId) {
    final path = p.join(projectDirectory.absolute.path, resourceId.path);
    return File(path).absolute.uri.toString();
  }
}

class LspInitializationOptions {
  // ignore: avoid_unused_constructor_parameters
  LspInitializationOptions(dynamic options);
}
