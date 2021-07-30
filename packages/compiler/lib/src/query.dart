import 'dart:convert';
import 'dart:core';
import 'dart:core' as core;
import 'dart:developer';

import 'package:dartx/dartx.dart';
import 'package:freezed_annotation/freezed_annotation.dart';
import 'package:meta/meta.dart';

import 'build_artifacts.dart';
import 'compilation/ast.dart';
import 'compilation/ids.dart';
import 'errors.dart';
import 'resource_provider.dart';
import 'utils.dart';

part 'query.freezed.dart';
part 'query.g.dart';

@immutable
class QueryConfig {
  const QueryConfig({
    @required this.packageName,
    @required this.resourceProvider,
    @required this.buildArtifactManager,
  })  : assert(packageName != null),
        assert(resourceProvider != null),
        assert(buildArtifactManager != null);

  final String packageName;
  PackageId get packageId => PackageId(packageName);

  final ResourceProvider resourceProvider;
  final BuildArtifactManager buildArtifactManager;

  // ignore: use_to_and_as_if_applicable
  GlobalQueryContext createContext() => GlobalQueryContext(this);
}

typedef QueryProvider<K, R> = R Function(QueryContext context, K key);

class Query<K, R> {
  Query(
    this.name, {
    bool persist = true,
    this.evaluateAlways = false,
    @required this.provider,
  })  : assert(name != null),
        assert(persist != null),
        assert(evaluateAlways != null),
        persist = persist && !evaluateAlways,
        assert(provider != null);

  final String name;

  // Modifiers:
  /// Results of this query won't be persisted.
  final bool persist;

  /// The result of this query isn't cached.
  ///
  /// This allows the query to read inputs (e.g., files).
  final bool evaluateAlways;

  final QueryProvider<K, R> provider;

  R call(QueryContext context, K key) {
    final result = context.callQuery(this, key);
    assert(result != null);
    return result;
  }

  R execute(QueryContext context, K key) {
    final result = Timeline.timeSync(
      '$name($key)',
      () => provider(context, key),
    );
    assert(result != null);
    return result;
  }
}

@immutable
class GlobalQueryContext {
  GlobalQueryContext(this.config) : assert(config != null);

  final QueryConfig config;

  Option<R> callQuery<K, R>(Query<K, R> query, K key) {
    final cachedResult = getResult<R>(query.name, key);
    if (cachedResult is Some) return cachedResult;

    RecordedQueryCall result;
    try {
      result = QueryContext(this)._execute(query, key);
    } on _QueryFailedException catch (e) {
      result = e.recordedCall;
    }

    if (query.name.startsWith('dart.')) {
      var dateTime = DateTime.now().toIso8601String();
      dateTime =
          dateTime.substring(0, dateTime.indexOf('.')).replaceAll(':', '-');
      final encoder = JsonEncoder.withIndent('  ', (object) {
        try {
          return object.toString();
        } catch (_) {
          return core.Error.safeToString(object);
        }
      });
      config.buildArtifactManager.setContent(
        QueryContext(this),
        BuildArtifactId(
          config.packageId,
          'query-traces/$dateTime ${query.name}.json',
        ),
        encoder.convert(result.toJson()),
      );
    }

    return result.result != null ? Some(result.result as R) : None();
  }

  final Map<Tuple2<String, dynamic>, dynamic> _results = {};
  void _reportResult(String queryName, Object key, Object result) {
    final mapKey = Tuple2(queryName, key);
    assert(!_results.containsKey(mapKey));
    _results[mapKey] = result;
  }

  Option<R> getResult<R>(String queryName, Object key) {
    final mapKey = Tuple2(queryName, key);
    return Option.of(_results[mapKey] as R);
  }

  final Map<Tuple2<String, dynamic>, List<ReportedCompilerError>>
      _reportedErrors = {};
  Iterable<ReportedCompilerError> get reportedErrors =>
      _reportedErrors.values.flatten();
  Map<ResourceId, List<ReportedCompilerError>> get reportedErrorsByResourceId =>
      reportedErrors.groupBy((e) => e.location?.resourceId);
  void _reportErrors(
    String queryName,
    Object key,
    List<ReportedCompilerError> errors,
  ) {
    final mapKey = Tuple2(queryName, key);
    if (errors.isNotEmpty) {
      _reportedErrors[mapKey] = errors;
    } else {
      _reportedErrors.remove(mapKey);
    }
  }

  final List<Tuple2<String, dynamic>> _queryStack = [];
  void recordQueryEnter(String name, dynamic key) {
    final tuple = Tuple2(name, key);
    final hasCycle = _queryStack.contains(tuple);
    _queryStack.add(tuple);
    if (hasCycle) {
      final stack = _queryStack.reversed
          .map((it) => '${it.first}(${it.second})')
          .join('\n');
      throw CompilerError.internalError(
        '🔁 Cycle detected.\n'
        'Query stack:\n$stack\n\n'
        'Stack trace:\n${StackTrace.current}',
      );
    }
  }

  void recordQueryExit() => _queryStack.removeLast();
}

class QueryContext {
  QueryContext(this.globalContext) : assert(globalContext != null);

  final GlobalQueryContext globalContext;
  QueryConfig get config => globalContext.config;

  R callQuery<K, R>(Query<K, R> query, K key) {
    globalContext.recordQueryEnter(query.name, key);
    final cachedResult = globalContext.getResult<R>(query.name, key);
    if (cachedResult is Some) {
      globalContext.recordQueryExit();
      return cachedResult.value;
    }

    final result = QueryContext(globalContext)._execute(query, key);
    _innerCalls.add(result);
    if (result.result == null) {
      globalContext.recordQueryExit();
      throw _QueryFailedException(result);
    }

    globalContext.recordQueryExit();
    return result.result as R;
  }

  RecordedQueryCall _execute<K, R>(Query<K, R> query, K key) {
    void reportErrors() =>
        globalContext._reportErrors(query.name, key, _reportedErrors);
    RecordedQueryCall onErrors(
      dynamic error,
      StackTrace stackTrace, {
      bool shouldReport = true,
    }) {
      var errors = error is _QueryFailedException
          ? error.recordedCall.thrownErrors
          : error is Iterable<ReportedCompilerError>
              ? error.toList()
              : [error as ReportedCompilerError];
      errors = errors
          .map((e) => e.error == CompilerError.internalError
              ? e.copyWith(message: '${e.message}\n\n$stackTrace')
              : e)
          .toList();
      if (shouldReport) this.reportErrors(errors);
      reportErrors();

      return RecordedQueryCall(
        name: query.name,
        key: key,
        innerCalls: _innerCalls,
        thrownErrors: errors,
      );
    }

    try {
      final result = query.execute(this, key);
      if (!query.evaluateAlways) {
        globalContext._reportResult(query.name, key, result);
      }
      reportErrors();
      return RecordedQueryCall(
        name: query.name,
        key: key,
        innerCalls: _innerCalls,
        result: result,
      );
    } on ReportedCompilerError catch (e, st) {
      return onErrors(e, st);
    } on Iterable<ReportedCompilerError> catch (e, st) {
      return onErrors(e, st);
    } on _QueryFailedException catch (e, st) {
      return onErrors(e, st, shouldReport: false);
    } catch (e, st) {
      return onErrors(CompilerError.internalError(e.toString()), st);
    }
  }

  final _reportedErrors = <ReportedCompilerError>[];
  void reportError(ReportedCompilerError error) => _reportedErrors.add(error);
  void reportErrors(List<ReportedCompilerError> errors) =>
      _reportedErrors.addAll(errors);

  final _innerCalls = <RecordedQueryCall>[];
}

@freezed
abstract class _QueryFailedException
    implements _$_QueryFailedException, Exception {
  const factory _QueryFailedException(RecordedQueryCall recordedCall) =
      __QueryFailedException;
  factory _QueryFailedException.fromJson(Map<String, dynamic> json) =>
      _$_QueryFailedExceptionFromJson(json);
  const _QueryFailedException._();
}

@freezed
abstract class RecordedQueryCall implements _$RecordedQueryCall {
  const factory RecordedQueryCall({
    @required String name,
    @required Object key,
    @required List<RecordedQueryCall> innerCalls,
    Object result,
    List<ReportedCompilerError> thrownErrors,
  }) = _RecordedQueryCall;
  factory RecordedQueryCall.fromJson(Map<String, dynamic> json) =>
      _$RecordedQueryCallFromJson(json);
  const RecordedQueryCall._();
}
