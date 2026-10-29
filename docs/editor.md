# Editor integration

`oxcruise --lsp` is a language server on stdio. It shows the violations of a normal run in the editor: on the import
they apply to, or on the first line of the file for rules about the module as a whole.

## VS Code

The extension lives in [`npm/vscode-extension`](../npm/vscode-extension). It is not published yet; to try it, run
`just vscode` and open that folder in VS Code with F5 (Extension Development Host).

It finds the server in this order:

1. `oxcruise.path.server`, when set. A `.js` file or an npm bin script runs with `node`; anything else runs as an
   executable.
2. `node_modules/oxcruise` of the workspace folder (or the nearest parent that has one), run with `node` from the
   `PATH`. This is the npm build, so `jsPlugins` and `.ts` configs work.
3. The native binary bundled in the `.vsix`. It has no JS runtime: a config with `jsPlugins` fails with a message
   that says so, and `oxcruise.config.ts` needs `node` on the `PATH` (JSON configs always work).

| Setting | |
|---|---|
| `oxcruise.enable` | turn the extension off |
| `oxcruise.path.server` | explicit server (see above) |
| `oxcruise.configPath` | config to use instead of the discovered one; `${workspaceFolder}` works |
| `oxcruise.trace.server` | log the protocol in the *oxcruise* output channel |

Commands: *oxcruise: Restart Server*, *oxcruise: Show Output*. Changing a setting restarts the server.

## What you get

* **Diagnostics** with the rule as `code` (a link to its page in [rules.md](rules.md) for built-in rules), `error` or
  `warning` as configured. Cycles and reachability paths list every hop as *related information*, at the import
  that leaves each module.
* **Quick fixes** on a diagnostic in a `.js`/`.ts` file: *Ignore `<rule>` for this line* (adds
  `// oxcruise-ignore-next-line <rule>` above the import, with its indentation) and *Ignore `<rule>` for this file*
  (`// oxcruise-ignore-file <rule>` at the top, after a shebang). Module-level violations get only the file fix.
  `.vue`, `.svelte` and `.astro` files get none yet.
* **Hover** on an import specifier: what it resolves to and every rule that reports it.

## When it refreshes

Not while you type. A full run starts when the workspace opens, when you **save** a file, and when
`oxcruise.config.*`, `.oxcruiserc.json`, `package.json` or a `tsconfig*.json` changes on disk. Saves in quick
succession (save all, a branch switch) are collapsed into one run, and a run that is overtaken by a newer save is
dropped. A save can add or clear violations in *other* files (a cycle is broken, an orphan stops being one), so every
run is the whole graph from disk; on large projects that still takes well under a second. This also means the
diagnostics always match `oxcruise` on the command line.

Consequences: unsaved edits are not analysed, so after applying a quick fix, save to see the diagnostic go away. A file
changed outside the editor is picked up at the next save of anything in the workspace.

A config that fails to load (a syntax error, an unknown rule, a missing plugin) shows one error message and clears
the diagnostics until the next successful run. Warnings of a successful run (such as a `forbid` rule that matches
nothing) go to the output channel.

## Plugins and the npm build

With the npm build the server keeps one Node process for its whole life. A config module is evaluated again on every
run, so edits to `oxcruise.config.ts` apply on save, but **plugin files are imported once**: restart the server
(*oxcruise: Restart Server*) after changing a plugin. In this mode anything a config or plugin prints with
`console.log` goes to stderr, because stdout carries the protocol.

## Other editors

Anything that can start `oxcruise --lsp` (or `npx oxcruise --lsp`) and speak LSP over stdio works. The server uses
`file:` workspace folders, negotiates UTF-8 columns when the client offers them (UTF-16 otherwise), does not need
text synchronisation beyond open/close/save, and expects the client to watch the config files above and send
`workspace/didChangeWatchedFiles` (VS Code's `synchronize.fileEvents` does this).
