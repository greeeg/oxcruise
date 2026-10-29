use std::path::PathBuf;

use oxc_resolver::{
    AliasValue, ResolveOptions, TsconfigDiscovery, TsconfigOptions, TsconfigReferences,
};

#[derive(Debug, Clone, Default)]
pub enum TsconfigMode {
    /// Nearest tsconfig per file, following project references.
    #[default]
    Auto,
    /// One explicit tsconfig for every file.
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct ResolveConfig {
    /// Absolute cruise root; relative module paths are computed against it.
    pub root: PathBuf,
    /// `(prefix, targets)` aliases from config.
    pub alias: Vec<(String, Vec<String>)>,
    /// Export/import condition names tried before `import`/`require`, `node` and `default`.
    pub conditions: Vec<String>,
    pub tsconfig: TsconfigMode,
}

impl ResolveConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: crate::simplify_path(&root.into()),
            alias: vec![],
            conditions: vec![],
            tsconfig: TsconfigMode::Auto,
        }
    }
}

/// Source extensions first, declaration files last: a runtime dependency graph wants
/// `index.js` over `index.d.ts` when a package has no `main`.
const DEFAULT_EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json", ".node", ".vue",
    ".svelte", ".astro", ".d.ts", ".d.mts", ".d.cts",
];

/// Which export condition decides between dual ESM/CJS packages, as in Node:
/// `import`/`export`/`import()` use `import`, `require()` uses `require`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveMode {
    Esm,
    Cjs,
}

pub(crate) fn to_oxc(c: &ResolveConfig, mode: ResolveMode) -> ResolveOptions {
    let runtime = match mode {
        ResolveMode::Esm => "import",
        ResolveMode::Cjs => "require",
    };
    // user conditions first, so `["source"]` beats `import` for workspace packages
    let mut conditions: Vec<String> = vec![];
    for n in c
        .conditions
        .iter()
        .map(String::as_str)
        .chain([runtime, "node", "default"])
    {
        if !conditions.iter().any(|c| c == n) {
            conditions.push(n.to_string());
        }
    }

    let ext_alias = |from: &str, to: &[&str]| {
        (
            from.to_string(),
            to.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    };

    ResolveOptions {
        cwd: Some(c.root.clone()),
        tsconfig: match &c.tsconfig {
            TsconfigMode::Auto => Some(TsconfigDiscovery::Auto),
            TsconfigMode::File(p) => Some(TsconfigDiscovery::Manual(TsconfigOptions {
                config_file: crate::simplify_path(p),
                references: TsconfigReferences::Auto,
            })),
        },
        alias: c
            .alias
            .iter()
            .map(|(k, vs)| {
                (
                    k.clone(),
                    vs.iter()
                        .map(|v| AliasValue::Path(simplify_str(v)))
                        .collect(),
                )
            })
            .collect(),
        condition_names: conditions,
        extension_alias: vec![
            ext_alias(".js", &[".js", ".ts", ".tsx"]),
            ext_alias(".jsx", &[".jsx", ".tsx"]),
            ext_alias(".mjs", &[".mjs", ".mts"]),
            ext_alias(".cjs", &[".cjs", ".cts"]),
        ],
        extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
        main_fields: match mode {
            ResolveMode::Esm => vec!["module".into(), "main".into()],
            ResolveMode::Cjs => vec!["main".into()],
        },
        builtin_modules: true,
        ..ResolveOptions::default()
    }
}

/// `simplify_path` for alias targets, which are strings (and may be bare package names).
fn simplify_str(v: &str) -> String {
    crate::simplify_path(std::path::Path::new(v))
        .to_string_lossy()
        .into_owned()
}
