import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { afterEach, describe, expect, test } from "vitest";
import { fixtures, project, root } from "./helpers.ts";

const bin = join(root, "../../npm/oxcruise/bin/oxcruise");

/** The bundled bin looks for the addon in a platform package; use the one `build:binding` made. */
function localAddon() {
  const file = readdirSync(join(root, "src-js")).find((f) => f.endsWith(".node"));
  if (!file)
    throw new Error("the native addon is not built; run `build:binding:debug` and `build:js`");
  return join(root, "src-js", file);
}

interface Message {
  id?: number;
  method?: string;
  params?: any;
  result?: any;
}

/** A client for `oxcruise --lsp` with just enough JSON-RPC framing for these tests. */
class Client {
  private child: ChildProcessWithoutNullStreams;
  private buffer = Buffer.alloc(0);
  private waiters: (() => void)[] = [];
  readonly messages: Message[] = [];
  stderr = "";
  private nextId = 0;

  constructor() {
    this.child = spawn("node", [bin, "--lsp"], {
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env, NAPI_RS_NATIVE_LIBRARY_PATH: localAddon() },
    });
    this.child.stdout.on("data", (chunk: Buffer) => {
      this.buffer = Buffer.concat([this.buffer, chunk]);
      this.drain();
    });
    this.child.stderr.on("data", (chunk: Buffer) => (this.stderr += chunk));
  }

  private drain() {
    for (;;) {
      const end = this.buffer.indexOf("\r\n\r\n");
      if (end < 0) return;
      const length = /Content-Length: (\d+)/i.exec(this.buffer.subarray(0, end).toString())?.[1];
      if (!length) throw new Error(`not an LSP frame: ${this.buffer.toString()}`);
      const start = end + 4;
      if (this.buffer.length < start + Number(length)) return;
      this.messages.push(
        JSON.parse(this.buffer.subarray(start, start + Number(length)).toString()),
      );
      this.buffer = this.buffer.subarray(start + Number(length));
      for (const wake of this.waiters.splice(0)) wake();
    }
  }

  private send(message: object) {
    const body = JSON.stringify({ jsonrpc: "2.0", ...message });
    this.child.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
  }

  notify(method: string, params: object) {
    this.send({ method, params });
  }

  async request(method: string, params: object | null) {
    const id = ++this.nextId;
    this.send({ id, method, params });
    return (await this.until((m) => m.id === id)).result;
  }

  /** Resolves with the first message (already received or yet to come) that satisfies `test`. */
  async until(test: (m: Message) => boolean, ms = 30_000): Promise<Message> {
    const deadline = Date.now() + ms;
    for (;;) {
      const found = this.messages.find(test);
      if (found) return found;
      const left = deadline - Date.now();
      if (left <= 0)
        throw new Error(`timed out; got ${JSON.stringify(this.messages)}\n${this.stderr}`);
      await new Promise<void>((resolve) => {
        this.waiters.push(resolve);
        setTimeout(resolve, left);
      });
    }
  }

  /** Latest diagnostics published for `file`, once any have been. */
  async diagnosticsFor(file: string, ready: (list: any[]) => boolean = () => true) {
    const uri = pathToFileURL(file).href;
    let list: any[] = [];
    await this.until((m) => {
      const latest = this.messages.findLast(
        (x) => x.method === "textDocument/publishDiagnostics" && x.params.uri === uri,
      );
      list = latest?.params.diagnostics ?? [];
      return latest !== undefined && ready(list);
    });
    return list;
  }

  async start(dir: string) {
    await this.request("initialize", {
      processId: null,
      capabilities: { general: { positionEncodings: ["utf-8"] } },
      workspaceFolders: [{ uri: pathToFileURL(dir).href, name: "p" }],
    });
    this.notify("initialized", {});
  }

  async stop() {
    if (this.child.exitCode !== null) return;
    await this.request("shutdown", null).catch(() => {});
    this.notify("exit", {});
    await new Promise((resolve) => this.child.once("exit", resolve));
  }
}

let client: Client | undefined;
afterEach(async () => {
  await client?.stop();
  client = undefined;
});

describe("oxcruise --lsp through the npm bin", () => {
  test("a plugin rule's violation arrives as a diagnostic on the import", async () => {
    const dir = join(fixtures, "plugin-project");
    client = new Client();
    await client.start(dir);
    const list = await client.diagnosticsFor(join(dir, "src/ui/page.ts"), (l) => l.length > 0);
    const d = list.find((x) => x.code === "acme/no-direct-db");
    expect(d.source).toBe("oxcruise");
    expect(d.severity).toBe(1);
    expect(d.message).toContain("UI must not import the database layer");
    expect(d.range.start.line).toBeGreaterThanOrEqual(0);
    expect(d.range.end.character).toBeGreaterThan(d.range.start.character);
  });

  test("a saved config is evaluated again, and what it prints cannot corrupt the protocol", async () => {
    const forbid = (name: string) =>
      `console.log("noise from the config");\n` +
      `export default { forbid: [{ name: "${name}", from: "src/a.ts", to: "src/b.ts" }] };\n`;
    const dir = project({
      "oxcruise.config.ts": forbid("first-rule"),
      "src/a.ts": "import './b';\n",
      "src/b.ts": "export {};\n",
    });
    client = new Client();
    await client.start(dir);
    const a = join(dir, "src/a.ts");
    expect((await client.diagnosticsFor(a, (l) => l.length > 0))[0].code).toBe("first-rule");

    writeFileSync(join(dir, "oxcruise.config.ts"), forbid("second-rule"));
    client.notify("workspace/didChangeWatchedFiles", {
      changes: [{ uri: pathToFileURL(join(dir, "oxcruise.config.ts")).href, type: 2 }],
    });
    const list = await client.diagnosticsFor(a, (l) => l[0]?.code === "second-rule");
    expect(list).toHaveLength(1);
    expect(client.stderr).toContain("noise from the config");
  });
});
