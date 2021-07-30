import 'dart:io';

import 'package:args/args.dart';
import 'package:path/path.dart' as p;
import 'package:args/command_runner.dart';
import 'package:compiler/compiler.dart';
import 'package:compiler_dart/compiler_dart.dart';

const _optionCandyDirectory = 'core-path';

Future<void> main(List<String> arguments) async {
  final parser = ArgParser()..addOption(_optionCandyDirectory);
  final result = parser.parse(arguments);

  try {
    final rest = result.rest;
    if (rest.length != 1) {
      throw UsageException(
        'Please enter the project directory to compile.',
        'candy2dart --core-path=/path/to/candy .',
      );
    }

    final candyDirectoryRaw = result[_optionCandyDirectory] as String;
    if (candyDirectoryRaw == null) {
      throw UsageException(
        'Please enter the directory of the Candy standard library.',
        'candy2dart --core-path=/path/to/candy .',
      );
    }
    final candyDirectory = Directory(candyDirectoryRaw);
    if (!candyDirectory.existsSync()) {
      throw UsageException(
        "Candy directory `${candyDirectory.absolute.path}` doesn't exist.",
        'candy2dart --core-path=/path/to/candy .',
      );
    }

    final projectDirectory = Directory(rest[0]);
    final validationResult =
        SimpleResourceProvider.isValidProjectDirectory(projectDirectory);
    if (validationResult != null) {
      throw UsageException(
        '${projectDirectory.absolute.path} is not a valid project directory:\n$validationResult',
        'candy2dart --core-path=/path/to/candy .',
      );
    }

    final config = QueryConfig(
      packageName: p.basename(projectDirectory.absolute.path),
      resourceProvider: ResourceProvider.default_(
        candyDirectory: candyDirectory,
        projectDirectory: projectDirectory,
      ),
      buildArtifactManager: BuildArtifactManager(projectDirectory),
    );
    final queryResult = config.callQuery(compile, Unit());

    if (queryResult.second.isNotEmpty) {
      print("❌ Compilation didn't succeed due to the following errors:");
      for (final error in queryResult.second) {
        print('• ${error.message}');

        if (error.location != null) print('  📍 Location: ${error.location}');

        if (error.relatedInformation.isNotEmpty) {
          print('  ℹ Related information:');
          for (final related in error.relatedInformation) {
            print('    • ${related.message}');
            if (error.location != null) {
              print('      📍 Location: ${related.location}');
            }
          }
        }
      }
      exit(HttpStatus.badRequest);
    }

    print('✅ Compilation succeeded.');
  } on UsageException catch (e) {
    print(e);
    exit(HttpStatus.badRequest);
  }
}
