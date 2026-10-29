# oxcruise for VS Code

Shows [oxcruise](https://github.com/greeeg/oxcruise) violations where they are written: a squiggle on the import
(or the first line of the file for module-level rules), the cycle or path as related locations, quick fixes that add
an `oxcruise-ignore` comment, and a link to the rule's documentation.

Diagnostics refresh when the workspace opens and whenever you save a file or edit the config, not while you type.
See [docs/editor.md](https://github.com/greeeg/oxcruise/blob/master/docs/editor.md) for the details.
