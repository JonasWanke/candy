import 'dart:convert';
import 'dart:core';
import 'dart:core' as core;
import 'dart:developer';
import 'dart:io';

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
  // GlobalQueryContext createContext() => GlobalQueryContext(this);
  Tuple2<Option<R>, List<ReportedCompilerError>> callQuery<K, R>(
    Query<K, R> query,
    K key,
  ) {
    return Timeline.timeSync(
      'top-level $name($key)',
      () => _RedGreenAlgorithm.runQuery(this, query, key),
    );
  }
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

    final mapKey = Tuple2(query.name, key);
    assert(!_dependencies.containsKey(mapKey));
    _dependencies[mapKey] =
        result.innerCalls.map((it) => Tuple2(it.query, it.key)).toList();

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

  final _results = <Tuple2<String, dynamic>, dynamic>{};
  void _reportResult(String queryName, Object key, Object result) {
    final mapKey = Tuple2(queryName, key);
    assert(!_results.containsKey(mapKey));
    _results[mapKey] = result;
  }

  final _dependencies = <Tuple2<String, dynamic>,
      List<Tuple2<Query<dynamic, dynamic>, dynamic>>>{};

  Option<R> getResult<R>(String queryName, Object key) {
    final mapKey = Tuple2(queryName, key);
    return Option.of(_results[mapKey] as R);
  }

  final _reportedErrors =
      <Tuple2<String, dynamic>, List<ReportedCompilerError>>{};
  List<ReportedCompilerError> get reportedErrors =>
      _reportedErrors.values.flatten().toList();
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

  final _queryStack = <Tuple2<String, dynamic>>[];
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
        query: query,
        name: query.name,
        key: key,
        innerCalls: _innerCalls,
        thrownErrors: errors,
      );
    }

    try {
      final result = query.execute(this, key);
      globalContext._reportResult(query.name, key, result);
      reportErrors();
      return RecordedQueryCall(
        query: query,
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
  const _QueryFailedException._();

  Map<String, dynamic> toJson() => recordedCall.toJson();
}

@freezed
abstract class RecordedQueryCall implements _$RecordedQueryCall {
  const factory RecordedQueryCall({
    @required Query<dynamic, dynamic> query,
    @required String name,
    @required Object key,
    @required List<RecordedQueryCall> innerCalls,
    Object result,
    List<ReportedCompilerError> thrownErrors,
  }) = _RecordedQueryCall;
  const RecordedQueryCall._();

  Map<String, dynamic> toJson() {
    return <String, dynamic>{
      'name': name,
      'key': key,
      'innerCalls': innerCalls.map((it) => it.toJson()).toList(),
      'result': result,
      'thrownErrors': thrownErrors,
    };
  }
}

class _RedGreenAlgorithm {
  // The input `oldContext` contains the old results to reuse, whereas the
  // newly created one is used to run the new queries.
  _RedGreenAlgorithm._(this.config)
      : assert(config != null),
        results = _previousContext?._results ?? {},
        errors = _previousContext?._reportedErrors ?? {},
        dependencies = _previousContext?._dependencies ?? {},
        globalContext = GlobalQueryContext(config);

  static Tuple2<Option<R>, List<ReportedCompilerError>> runQuery<K, R>(
    QueryConfig config,
    Query<K, R> query,
    Object key,
  ) {
    final algorithm = _RedGreenAlgorithm._(config);
    if (!algorithm._tryMarkGreen(Tuple2(query, key))) {
      algorithm._runQueryInternal(Tuple2(query, key));
    }
    _previousContext = algorithm.globalContext;
    final mapKey = Tuple2(query.name, key);
    return Tuple2(
      Option.of(_previousContext._results[mapKey] as R),
      _previousContext.reportedErrors,
    );
  }

  static GlobalQueryContext _previousContext;

  final QueryConfig config;
  final Map<Tuple2<String, dynamic>, dynamic> results;
  final Map<Tuple2<String, dynamic>, List<ReportedCompilerError>> errors;
  final Map<Tuple2<String, dynamic>,
      List<Tuple2<Query<dynamic, dynamic>, dynamic>>> dependencies;
  final colors = <Tuple2<String, dynamic>, Color>{};

  // Adapted from https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html

  bool _tryMarkGreen(Tuple2<Query<dynamic, dynamic>, dynamic> key) {
    final dependencies = this.dependencies[key];
    // The query didn't run before.
    if (dependencies == null) return false;

    for (final dependency in dependencies) {
      final dependencyColor = colors[dependency];
      if (dependencyColor == Color.green) {
        // This input has already been checked before and it has not changed; so
        // we can go on to check the next one.
      } else if (dependencyColor == Color.red) {
        // We found an input that has changed. We cannot mark `current_node` as
        // green without re-running the corresponding query.
        return false;
      } else {
        // This is the first time we look at this node. Let's try to mark it
        // green by calling `_tryMarkGreen()` recursively.
        if (_tryMarkGreen(dependency)) {
          // We successfully marked the input as green, on to the next.
        } else {
          // We could *not* mark the input as green. This means we don't know if
          // its value has changed. In order to find out, we re-run the
          // corresponding query now!
          _runQueryInternal(dependency);

          // Fetch and check the node color again. Running the query has
          // forced it to either red (if it yielded a different result than we
          // have in the cache) or green (if it yielded the same result).
          final newColor = colors[dependency];
          if (newColor == Color.green) {
            // Re-running the query paid off! The result is the same as before,
            // so this particular input does not invalidate `current_node`.
          } else if (newColor == Color.red) {
            // The input turned out to be red, so we cannot mark `current_node`
            // as green.
            return false;
          } else {
            // There is no way a node has no color after
            // re-running the query.
            throw StateError('unreachable');
          }
        }
      }
    }

    if (key.first.evaluateAlways) return false;

    // If we have gotten through the entire loop, it means that all inputs have
    // turned out to be green. If all inputs are unchanged, it means that the
    // query result corresponding to `current_node` cannot have changed either.
    final mapKey = Tuple2(key.first.name, key.second);
    colors[mapKey] = Color.green;
    globalContext._results[mapKey] = results[mapKey];
    globalContext._reportedErrors[mapKey] = errors[mapKey];
    return true;
  }

  final GlobalQueryContext globalContext;
  void _runQueryInternal(Tuple2<Query<dynamic, dynamic>, dynamic> key) {
    final result = globalContext.callQuery(key.first, key.second);
    final mapKey = Tuple2(key.first.name, key.second);

    if ((result.isSome && result.value == results[mapKey] ||
            result.isNone && !results.containsKey(mapKey)) &&
        DeepCollectionEquality()
            .equals(globalContext._reportedErrors[mapKey], errors[mapKey])) {
      colors[mapKey] = Color.green;
    } else {
      colors[mapKey] = Color.red;
      if (result.isSome) {
        results[mapKey] = result.value;
      } else {
        results.remove(mapKey);
      }
      errors[mapKey] = globalContext._reportedErrors[mapKey];
    }
  }
}

enum Color { red, green }
