use std::sync::OnceLock;

use oxcruise_config::Paths;
use oxcruise_core::{EdgeId, Graph, ModuleKind, parent_path};
use oxcruise_macros::declare_rule;
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::folders::FolderPath;
use crate::meta::{Hooks, Rule, RuleCtx, RuleMeta, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

/// One entry of `folders`: a folder and what it shows to the code outside it.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisibilityFolder {
    /// Globs of the folders. `{name}` captures a path segment, and `public` and `private` can use
    /// the capture. A glob matches only the folder it names. `**` matches every folder.
    pub path: Paths,
    /// Globs of the modules in the folder that code outside it can import. A glob is relative to
    /// the folder. Without `public`, every module is public.
    pub public: Option<Paths>,
    /// Globs of the modules in the folder that code outside it cannot import. A glob is relative
    /// to the folder. `private` wins over `public`.
    pub private: Option<Paths>,
    /// Oxcruise shows this text instead of the generic message.
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct VisibilityConfig {
    /// The folders to check. The rule does nothing without them.
    pub folders: Vec<VisibilityFolder>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Restricts what a folder shows to the code outside it.
    /// A module inside a folder can import every module in that folder.
    /// A module outside it can import only the modules that the folder makes visible.
    /// The rule checks dependencies on local modules.
    ///
    /// ### Options
    ///
    /// * `folders`: a list of entries. Each entry has a `path` and at least one of `public` and `private`.
    ///   The rule does nothing when the list is empty.
    ///   * `path`: globs of the folders. `{name}` captures a path segment. `**` matches every folder.
    ///   * `public`: globs of the modules that code outside the folder can import. The globs are relative to the folder.
    ///     Without `public`, every module is public.
    ///   * `private`: globs of the modules that code outside the folder cannot import. The globs are relative to the folder.
    ///     `private` wins over `public`.
    ///   * `message`: Oxcruise shows this text instead of the generic message.
    ///
    /// `public` and `private` can use the captures of `path`.
    /// A folder is relevant to a dependency only when it contains the target and not the importing module.
    /// If several folders hide the target, the rule reports the outermost one.
    ///
    /// ### Example
    ///
    /// A feature shows only its `index.ts`, and a folder named `internal` is private to its parent folder, as in the Go language:
    ///
    /// ```json
    /// { "folders": [
    ///   { "path": "src/features/{f}", "public": "index.ts", "message": "Import a feature through its index.ts." },
    ///   { "path": "**", "private": "internal" }
    /// ] }
    /// ```
    ///
    /// ### Ignoring a violation
    ///
    /// Put `// oxcruise-ignore-next-line visibility -- reason` above the dependency.
    Visibility, category = restriction, version = "0.1.0", config = VisibilityConfig
}

#[derive(Debug)]
struct Entry {
    path: FolderPath,
    public: Option<(PathPatterns, Vec<String>)>,
    private: Option<(PathPatterns, Vec<String>)>,
    message: Option<String>,
}

impl Entry {
    fn from_config(c: VisibilityFolder, where_: &str) -> Result<Entry, String> {
        if c.public.is_none() && c.private.is_none() {
            return Err(format!(
                "{where_}: needs at least one of `public` and `private`"
            ));
        }
        let path = FolderPath::new(&c.path, where_)?;
        type Compiled = Option<(PathPatterns, Vec<String>)>;
        let compile = |key: &str, paths: Option<Paths>| -> Result<Compiled, String> {
            paths
                .map(|p| {
                    let globs = p.to_vec();
                    if globs.is_empty() {
                        return Err(format!("{where_}: `{key}` needs a glob"));
                    }
                    let patterns = PathPatterns::new(&globs, Role::Referencing)
                        .map_err(|e| format!("{where_}: `{key}`: {e}"))?;
                    Ok((patterns, globs))
                })
                .transpose()
        };
        Ok(Entry {
            path,
            public: compile("public", c.public)?,
            private: compile("private", c.private)?,
            message: c.message,
        })
    }

    /// Why `to` (`rel` in `folder`) is hidden from outside, or `None` if it is visible.
    fn hides(&self, to: &str, rel: &str, caps: &Captures, folder: &str) -> Option<String> {
        let generic = if let Some((p, text)) = &self.private
            && p.is_match(rel, caps)
        {
            format!(
                "`{to}` is private to `{folder}` (`private: {}`)",
                text.join(", ")
            )
        } else if let Some((p, text)) = &self.public
            && !p.is_match(rel, caps)
        {
            format!(
                "`{to}` is not visible outside `{folder}`: only `{}` can be imported",
                text.join("`, `")
            )
        } else {
            return None;
        };
        Some(self.message.clone().unwrap_or(generic))
    }
}

/// Which entries match each folder that holds a local module, with the captures of the match.
type Matches = FxHashMap<String, Vec<(usize, Captures)>>;

#[derive(Debug, Default)]
pub struct Visibility {
    entries: Vec<Entry>,
    matches: OnceLock<Matches>,
}

impl Visibility {
    /// Test every folder of the graph against every entry once, so a dependency only looks up.
    fn matches(&self, g: &Graph) -> &Matches {
        self.matches.get_or_init(|| {
            let mut dirs: FxHashSet<&str> = FxHashSet::default();
            for m in g.modules().iter().filter(|m| m.kind == ModuleKind::Local) {
                let mut dir = parent_path(&m.path);
                while !dir.is_empty() && dirs.insert(dir) {
                    dir = parent_path(dir);
                }
            }
            // Sequential on purpose: this runs inside a `OnceLock` that the parallel dependency
            // checks wait on, and a nested parallel loop there can deadlock.
            let mut dirs: Vec<&str> = dirs.into_iter().collect();
            dirs.sort_unstable();
            dirs.into_iter()
                .filter_map(|dir| {
                    let hits: Vec<(usize, Captures)> = self
                        .entries
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| dir.starts_with(e.path.prefix()))
                        .filter_map(|(i, e)| Some((i, e.path.capture(dir)?)))
                        .collect();
                    (!hits.is_empty()).then(|| (dir.to_string(), hits))
                })
                .collect()
        })
    }
}

impl Rule for Visibility {
    const HOOKS: Hooks = Hooks::EDGE;

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        if value.get("marker").is_some() {
            return Err(format!(
                "invalid options for `{}`: the option `marker` was removed; write \
                 {{ \"folders\": [{{ \"path\": \"**\", \"private\": \"<marker>\" }}] }}",
                Self::NAME
            ));
        }
        let c: VisibilityConfig = parse_options(Self::NAME, value)?;
        let mut entries = vec![];
        let mut errors = vec![];
        for (i, folder) in c.folders.into_iter().enumerate() {
            match Entry::from_config(folder, &format!("folders[{i}]")) {
                Ok(e) => entries.push(e),
                Err(e) => errors.push(e),
            }
        }
        if errors.is_empty() {
            Ok(Self {
                entries,
                matches: OnceLock::new(),
            })
        } else {
            Err(format!(
                "invalid options for `{}`: {}",
                Self::NAME,
                errors.join("; ")
            ))
        }
    }

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        if self.entries.is_empty() {
            return;
        }
        let edge = cx.edge(e);
        let (from, to) = (cx.module(edge.from), cx.module(edge.to));
        if to.kind != ModuleKind::Local {
            return;
        }
        let matches = self.matches(cx.graph());
        // The folders that hold `to` but not `from`, outermost first: those below the folder
        // that the two modules share.
        let mut dirs: Vec<&str> = vec![];
        let mut dir = parent_path(&to.path);
        while !dir.is_empty() {
            let holds_from = from
                .path
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/'));
            if holds_from {
                break;
            }
            dirs.push(dir);
            dir = parent_path(dir);
        }
        for dir in dirs.into_iter().rev() {
            let Some(hits) = matches.get(dir) else {
                continue;
            };
            let rel = &to.path[dir.len() + 1..];
            for (i, caps) in hits {
                if let Some(message) = self.entries[*i].hides(&to.path, rel, caps, dir) {
                    cx.report_edge(e, message);
                    return;
                }
            }
        }
    }
}
