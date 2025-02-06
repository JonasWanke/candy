/* eslint-disable @typescript-eslint/no-unsafe-call */
import * as child_process from "child_process";
import linebyline from "linebyline";
import * as vscode from "vscode";

let diagnosticCollection: vscode.DiagnosticCollection;

// eslint-disable-next-line @typescript-eslint/no-unused-vars
export function activate(context: vscode.ExtensionContext) {
  console.info("Activated the 🍭 Candy extension!");

  diagnosticCollection = vscode.languages.createDiagnosticCollection("candy");
  context.subscriptions.push(diagnosticCollection);

  vscode.window.onDidChangeVisibleTextEditors(() => onlyRunOneAtATime(update));
  vscode.workspace.onDidChangeTextDocument(() => onlyRunOneAtATime(update));
}

// Updates can be triggered very frequently (on every keystroke), but they can
// take long – for example, when editing the Candy compiler itself, simply
// analyzing the files takes some time. Thus, here we make sure that only one
// update runs at a time.
let generation = 0;
let currentRun = Promise.resolve(null);
async function onlyRunOneAtATime(callback: () => Promise<void>) {
  console.log("Scheduling update");
  const myGeneration = ++generation;
  await currentRun;
  if (generation != myGeneration) return; // a newer update exists and will run
  // eslint-disable-next-line @typescript-eslint/no-misused-promises, no-async-promise-executor
  currentRun = new Promise(async (resolve) => {
    await callback();
    resolve(null);
  });
}

async function update() {
  console.log("Updating");
  const promises = [];
  for (const editor of vscode.window.visibleTextEditors) {
    const uri = editor.document.uri.toString();
    if (!uri.endsWith(".candy")) continue;

    const analysis = analyze(uri);
    promises.push(analysis);
    analysis
      .then((diagnostics) => {
        diagnosticCollection.clear();
        const diagnosticMap = new Map<string, vscode.Diagnostic[]>();
        for (const diagnostic of diagnostics) {
          console.info(`Error: ${JSON.stringify(diagnostic)}`);
          if (diagnostic.source.file != uri) continue;
          const range = new vscode.Range(
            new vscode.Position(
              diagnostic.source.start.line,
              diagnostic.source.start.character,
            ),
            new vscode.Position(
              diagnostic.source.end.line,
              diagnostic.source.end.character,
            ),
          );
          const diagnostics = diagnosticMap.get(diagnostic.source.file) ?? [];
          diagnostics.push(
            new vscode.Diagnostic(
              range,
              diagnostic.message,
              vscode.DiagnosticSeverity.Error,
            ),
          );
          diagnosticMap.set(diagnostic.source.file, diagnostics);
        }
        diagnosticMap.forEach((diags, file) => {
          diagnosticCollection.set(vscode.Uri.parse(file), diags);
        });
      })
      .catch((error) => {
        console.error(`Analyzing failed: ${error}`);
      });
  }
  await Promise.all(promises);
}

/// Communication with the Candy language server works using the following
/// schema.

type AnalyzeMessage = ReadFileMessage | DiagnosticsMessage; // candy tooling-analyze ...
interface ReadFileMessage {
  type: "read_file";
  path: string;
}
interface DiagnosticsMessage {
  type: "diagnostics";
  diagnostics: Diagnostic[];
}
interface Diagnostic {
  message: string;
  source: {
    file: string;
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
}

async function analyze(path: string): Promise<Diagnostic[]> {
  console.log(`Analyzing ${path}`);
  const candy = child_process.spawn(
    vscode.workspace
      .getConfiguration("candy")
      .get<string>("compilerExecutablePath") ?? "candy",
    ["tooling-analyze", path],
    { env: { ...process.env, RUST_BACKTRACE: "1" } },
  );
  candy.on("error", (error) => {
    console.error(`Failed to spawn: ${error.name}: ${error.message}`);
  });
  linebyline(candy.stderr).on("line", (line: string) => {
    console.log(line);
  });

  let diagnostics: Diagnostic[] = [];
  // eslint-disable-next-line @typescript-eslint/no-misused-promises
  linebyline(candy.stdout).on("line", async function (line: string) {
    console.info("Line: " + line);
    const message = JSON.parse(line) as AnalyzeMessage;
    switch (message.type) {
      case "read_file":
        candy.stdin.write(await handleReadFileMessage(message));
        break;
      case "diagnostics":
        diagnostics = message.diagnostics;
        break;
    }
  });

  const exitCode: number | null = await new Promise((resolve) =>
    candy.on("close", (exitCode) => {
      resolve(exitCode);
    }),
  );
  console.info(`\`candy tooling-analyze\` exited with exit code ${exitCode}.`);

  return diagnostics;
}

async function handleReadFileMessage(
  message: ReadFileMessage,
): Promise<string> {
  const uri = vscode.Uri.parse(message.path);
  const content = await readFile(uri);
  const response = content
    ? { type: "read_file", success: true, content: content }
    : { type: "read_file", success: false };
  return `${JSON.stringify(response)}\n`;
}

/**
 * Returns the source code of the given URI. Prefers the content of open text
 * documents, even if they're not saved yet. If none exists, asks the file
 * system.
 */
async function readFile(uri: vscode.Uri): Promise<string | null> {
  for (const doc of vscode.workspace.textDocuments) {
    if (doc.uri.toString() == uri.toString()) return doc.getText();
  }

  try {
    const bytes = await vscode.workspace.fs.readFile(uri);
    return new TextDecoder("utf8").decode(bytes);
  } catch (e) {
    return null;
  }
}
