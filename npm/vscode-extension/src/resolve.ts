// Which oxcruise to run as the server. No `vscode` import, so it can be exercised on its own.

import { existsSync, readFileSync, statSync } from "node:fs";
import { delimiter, dirname, extname, join, resolve } from "node:path";

export type Server =
  /** The npm package's bin script, run with `node`; supports `jsPlugins`. */
  | { kind: "script"; script: string; source: string }
  /** A native executable; has no JS runtime, so no `jsPlugins`. */
  | { kind: "binary"; command: string; source: string };

export interface Inputs {
  /** `oxcruise.path.server`, already trimmed; empty when unset. */
  setting: string;
  workspaceFolders: string[];
  /** The extension's install directory. */
  extensionPath: string;
  platform: NodeJS.Platform;
  arch: string;
  /** `process.env.PATH`. */
  path: string | undefined;
}

const isFile = (p: string) => {
  try {
    return statSync(p).isFile();
  } catch {
    return false;
  }
};

/** `node_modules/oxcruise/bin/oxcruise` of the nearest install at or above `folder`. */
export function localInstall(folder: string): string | undefined {
  for (let dir = resolve(folder); ; dir = dirname(dir)) {
    const script = join(dir, "node_modules", "oxcruise", "bin", "oxcruise");
    if (isFile(script)) return script;
    if (dirname(dir) === dir) return undefined;
  }
}

export function bundledBinary(extensionPath: string, platform: string, arch: string) {
  const exe = platform === "win32" ? ".exe" : "";
  return join(extensionPath, "bin", `oxcruise-${platform}-${arch}${exe}`);
}

export function findOnPath(name: string, path: string | undefined, platform: string) {
  const names = platform === "win32" ? [`${name}.exe`] : [name];
  for (const dir of (path ?? "").split(delimiter).filter(Boolean)) {
    for (const n of names) if (isFile(join(dir, n))) return join(dir, n);
  }
  return undefined;
}

const SCRIPT_EXTENSIONS = new Set([".js", ".mjs", ".cjs"]);

/**
 * In order: the setting, a project install (needs `node` on the PATH), the bundled binary.
 * `Error` carries what to tell the user when none exists.
 */
export function resolveServer(i: Inputs): Server | Error {
  if (i.setting) {
    if (!existsSync(i.setting))
      return new Error(`oxcruise.path.server: ${i.setting} does not exist`);
    const source = "the oxcruise.path.server setting";
    const ext = extname(i.setting);
    const script = SCRIPT_EXTENSIONS.has(ext) || (!ext && isNodeScript(i.setting));
    return script
      ? { kind: "script", script: i.setting, source }
      : { kind: "binary", command: i.setting, source };
  }
  if (findOnPath("node", i.path, i.platform)) {
    for (const folder of i.workspaceFolders) {
      const script = localInstall(folder);
      if (script) return { kind: "script", script, source: "the project's node_modules" };
    }
  }
  const bundled = bundledBinary(i.extensionPath, i.platform, i.arch);
  if (isFile(bundled)) {
    return { kind: "binary", command: bundled, source: "the binary bundled with the extension" };
  }
  return new Error(
    "no oxcruise found: install it in the project (`npm i -D oxcruise`) or set oxcruise.path.server",
  );
}

/** npm bins have a `#!/usr/bin/env node` first line and no extension. */
function isNodeScript(path: string): boolean {
  try {
    return (
      statSync(path).size < 4096 && readFileSync(path, "utf8").startsWith("#!/usr/bin/env node")
    );
  } catch {
    return false;
  }
}
