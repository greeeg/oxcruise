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
    Disabled,
}

#[derive(Debug, Clone)]
pub struct ResolveConfig {
    /// Absolute cruise root; relative module paths are computed against it.
    pub root: PathBuf,
    /// `(prefix, targets)` aliases from config.
    pub alias: Vec<(String, Vec<String>)>,
    /// Extra export/import condition names appended to the defaults.
    pub conditions: Vec<String>,
    pub main_fields: Option<Vec<String>>,
    /// Explicit extension list. Never inferred from installed tooling.
    pub extensions: Option<Vec<String>>,
    pub tsconfig: TsconfigMode,
    /// Resolve workspace packages to their source rather than `dist` by
    /// adding the `source_conditions` first.
    pub prefer_source: bool,
    pub source_conditions: Vec<String>,
}

impl ResolveConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            alias: vec![],
            conditions: vec![],
            main_fields: None,
            extensions: None,
            tsconfig: TsconfigMode::Auto,
            prefer_source: false,
            source_conditions: vec!["source".into()],
        }
    }
}

const DEFAULT_EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".mts", ".cts", ".d.ts", ".js", ".jsx", ".mjs", ".cjs", ".json", ".node",
    ".vue", ".svelte", ".astro",
];

pub(crate) fn to_oxc(c: &ResolveConfig) -> ResolveOptions {
    let mut conditions: Vec<String> = vec![];
    if c.prefer_source {
        conditions.extend(c.source_conditions.iter().cloned());
    }
    for n in ["import", "require", "node", "default"]
        .map(String::from)
        .iter()
        .chain(&c.conditions)
    {
        if !conditions.contains(n) {
            conditions.push(n.clone());
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
                config_file: p.clone(),
                references: TsconfigReferences::Auto,
            })),
            TsconfigMode::Disabled => None,
        },
        alias: c
            .alias
            .iter()
            .map(|(k, vs)| {
                (
                    k.clone(),
                    vs.iter().map(|v| AliasValue::Path(v.clone())).collect(),
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
        extensions: c
            .extensions
            .clone()
            .unwrap_or_else(|| DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect()),
        main_fields: c
            .main_fields
            .clone()
            .unwrap_or_else(|| vec!["module".into(), "main".into()]),
        builtin_modules: true,
        ..ResolveOptions::default()
    }
}
