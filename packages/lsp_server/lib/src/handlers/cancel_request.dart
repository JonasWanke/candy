// Copyright (c) 2019, the Dart project authors. Please see the AUTHORS file
// for details. All rights reserved. Use of this source code is governed by a
// BSD-style license that can be found in the LICENSE file.

import '../analysis_server.dart';
import '../generated/lsp_protocol/protocol_generated.dart';
import '../generated/lsp_protocol/protocol_special.dart';
import 'handlers.dart';

class CancelRequestHandler extends MessageHandler<CancelParams, void> {
  CancelRequestHandler(AnalysisServer server) : super(server);

  final _tokens = <String, CancelableToken>{};

  @override
  Method get handlesMessage => Method.cancelRequest;

  @override
  LspJsonHandler<CancelParams> get jsonHandler => CancelParams.jsonHandler;

  void clearToken(RequestMessage message) =>
      _tokens.remove(message.id.toString());

  CancelableToken createToken(RequestMessage message) {
    final token = CancelableToken();
    _tokens[message.id.toString()] = token;
    return token;
  }

  @override
  ErrorOr<void> handle(CancelParams params, CancellationToken token) {
    // Don't assume this is in the map as it's possible the client sent a
    // cancellation that we processed after already starting to send the
    // response and cleared the token.
    _tokens[params.id.toString()]?.cancel();
    return success();
  }
}
