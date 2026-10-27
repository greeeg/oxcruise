import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const root = join(dirname(fileURLToPath(import.meta.url)), "..");
export const fixtures = join(root, "tests/fixtures");

/** Fails with an actionable message instead of an obscure loader error. */
export async function loadNative() {
  try {
    return await import("../src-js/bindings.js");
  } catch (e) {
    throw new Error(
      `the native addon is not built (${e instanceof Error ? e.message : e}); run \`pnpm build:binding:debug\` in apps/oxcruise`,
      { cause: e },
    );
  }
}

export interface CliOutput {
  code: number;
  stdout: string;
  stderr: string;
}

/** Runs the CLI in-process through the real napi addon and the real JS host. */
export async function runCli(cwd: string, args: string[]): Promise<CliOutput> {
  const { runCaptured } = await loadNative();
  const { createHost } = await import("../src-js/host.ts");
  const host = createHost();
  // `migrate` is a subcommand, so it goes first and takes no --color
  const argv =
    args[0] === "migrate"
      ? ["migrate", "--cwd", cwd, ...args.slice(1)]
      : ["--cwd", cwd, "--color", "never", ...args];
  const r = await runCaptured(
    argv,
    (req) => host.loadPlugin(req),
    (req) => host.runRules(req),
    (req) => host.renderReport(req),
    (path) => host.loadJsConfig(path),
  );
  const scrub = (s: string) =>
    s
      .replaceAll(cwd, "<cwd>")
      .split("\n")
      .filter((l) => !l.startsWith("finished in"))
      .join("\n");
  return { code: r.exitCode, stdout: scrub(r.stdout), stderr: scrub(r.stderr) };
}

/** A throwaway project directory. */
export function project(files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), "oxcruise-e2e-"));
  for (const [path, content] of Object.entries(files)) {
    const full = join(dir, path);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, content);
  }
  return dir;
}

export const nativeBuilt = existsSync(
  join(
    root,
    "src-js",
    `oxcruise.${process.platform === "linux" ? "linux" : process.platform}-${process.arch}-${process.platform === "linux" ? "gnu" : ""}.node`.replace(
      /-$/,
      "",
    ),
  ),
);
