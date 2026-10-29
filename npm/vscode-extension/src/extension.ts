import { arch, platform } from "node:os";
import * as vscode from "vscode";
import {
  type Executable,
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
} from "vscode-languageclient/node";
import { type Server, resolveServer } from "./resolve.ts";

const LANGUAGES = [
  "javascript",
  "javascriptreact",
  "typescript",
  "typescriptreact",
  "vue",
  "svelte",
  "astro",
];

let client: LanguageClient | undefined;
let output: vscode.OutputChannel;
let extensionPath = "";

export async function activate(context: vscode.ExtensionContext) {
  output = vscode.window.createOutputChannel("oxcruise");
  extensionPath = context.extensionPath;
  context.subscriptions.push(
    output,
    vscode.commands.registerCommand("oxcruise.restartServer", restart),
    vscode.commands.registerCommand("oxcruise.showOutputChannel", () => output.show()),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("oxcruise") && !e.affectsConfiguration("oxcruise.trace")) {
        void restart();
      }
    }),
  );
  await start();
  context.subscriptions.push({ dispose: () => void stop() });
}

export async function deactivate() {
  await stop();
}

function settings() {
  return vscode.workspace.getConfiguration("oxcruise");
}

function folders() {
  return (vscode.workspace.workspaceFolders ?? []).map((f) => f.uri.fsPath);
}

function executable(server: Server, args: string[]): Executable {
  return server.kind === "script"
    ? { command: "node", args: [server.script, ...args] }
    : { command: server.command, args };
}

async function start() {
  if (!settings().get<boolean>("enable", true)) return;

  const server = resolveServer({
    setting: (settings().get<string>("path.server") ?? "").trim(),
    workspaceFolders: folders(),
    extensionPath,
    platform: platform(),
    arch: arch(),
    path: process.env.PATH,
  });
  if (server instanceof Error) {
    output.appendLine(server.message);
    void vscode.window.showErrorMessage(`oxcruise: ${server.message}`);
    return;
  }

  const args = ["--lsp"];
  const configPath = (settings().get<string>("configPath") ?? "").trim();
  if (configPath) {
    const first = folders()[0] ?? "";
    args.push("--config", configPath.replaceAll("${workspaceFolder}", first));
  }
  const run = executable(server, args);
  output.appendLine(`starting ${server.source}: ${run.command} ${(run.args ?? []).join(" ")}`);
  if (server.kind === "binary" && !(settings().get<string>("path.server") ?? "").trim()) {
    output.appendLine(
      "the bundled binary has no JS runtime: `jsPlugins` and JS/TS configs need `npm i -D oxcruise`",
    );
  }

  const serverOptions: ServerOptions = { run, debug: run };
  const clientOptions: LanguageClientOptions = {
    documentSelector: LANGUAGES.map((language) => ({ scheme: "file", language })),
    outputChannel: output,
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher(
        "**/{oxcruise.config.*,.oxcruiserc.json,package.json,tsconfig*.json}",
      ),
    },
  };
  client = new LanguageClient("oxcruise", "oxcruise", serverOptions, clientOptions);
  try {
    await client.start();
  } catch (e) {
    client = undefined;
    const message = e instanceof Error ? e.message : String(e);
    output.appendLine(`the server did not start: ${message}`);
    void vscode.window.showErrorMessage(`oxcruise: the server did not start (${message})`);
  }
}

async function stop() {
  const c = client;
  client = undefined;
  if (c?.needsStop()) await c.stop().catch(() => {});
}

async function restart() {
  await stop();
  await start();
}
