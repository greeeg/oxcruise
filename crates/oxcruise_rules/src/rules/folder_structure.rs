use oxcruise_config::Paths;
use oxcruise_core::{FileTree, FolderId, base_name, parent_path};
use oxcruise_macros::declare_rule;
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::folders::{FolderPath, join, label};
use crate::meta::{Analyses, Hooks, Rule, RuleCtx, RuleMeta, parse_options};
use crate::pattern::{Captures, Literal, PathPatterns, Role, fill_captures};

/// The case of a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum Case {
    /// `my-file`
    #[serde(rename = "kebab-case")]
    Kebab,
    /// `myFile`
    #[serde(rename = "camel-case")]
    Camel,
    /// `MyFile`
    #[serde(rename = "pascal-case")]
    Pascal,
    /// `my_file`
    #[serde(rename = "snake-case")]
    Snake,
}

impl Case {
    fn as_str(self) -> &'static str {
        match self {
            Case::Kebab => "kebab-case",
            Case::Camel => "camel-case",
            Case::Pascal => "pascal-case",
            Case::Snake => "snake-case",
        }
    }

    /// Does `word` have this case? A word has letters and digits, and for the kebab and snake
    /// case a separator between them. Letters can be of any script.
    fn accepts(self, word: &str) -> bool {
        let lower = |c: char| c.is_alphabetic() && !c.is_uppercase();
        let separated = |sep: char| {
            // a separator needs a letter or digit before it, and the word cannot end with one
            let mut after_sep = true;
            for c in word.chars() {
                if c == sep {
                    if after_sep {
                        return false;
                    }
                    after_sep = true;
                } else if lower(c) || c.is_ascii_digit() {
                    after_sep = false;
                } else {
                    return false;
                }
            }
            !after_sep
        };
        let alphanumeric = |first: fn(char) -> bool| {
            word.chars().next().is_some_and(first) && word.chars().all(char::is_alphanumeric)
        };
        match self {
            Case::Kebab => separated('-'),
            Case::Snake => separated('_'),
            Case::Camel => alphanumeric(lower),
            Case::Pascal => alphanumeric(char::is_uppercase),
        }
    }
}

/// One entry of `folders`: the folders it matches and the checks for each of them.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FolderStructureFolder {
    /// Globs of the folders to check. `{name}` captures a path segment, and `require` and `allow`
    /// can use the capture. A glob matches only the folder it names.
    /// A glob can also match a file. Then the checks apply to the folder of the file.
    pub path: Paths,
    /// Globs of the files and folders that must be in the folder. A glob is relative to the
    /// folder and can contain `/`. Each glob needs a match.
    pub require: Option<Paths>,
    /// Globs of the names that the files and folders directly in the folder can have.
    /// A glob that starts with `!` forbids a name. A list of only such globs allows every other name.
    pub allow: Option<Paths>,
    /// The case that the names of the files and folders directly in the folder must have.
    /// The rule checks the name before its first `.`, so `Button.test.tsx` is checked as `Button`.
    pub case: Option<Case>,
    /// Oxcruise shows this text instead of the generic message of the violations of this entry.
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FolderStructureConfig {
    /// The folders to check. When several entries match one folder, all of them apply.
    pub folders: Vec<FolderStructureFolder>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Checks the shape of the file tree: the files that a folder must have, the names that it can have, and the case of its names.
    /// The rule looks at all files in the entry folders, not only at modules.
    /// For the folders above the entry folders, such as the root folder, it knows their own files and folders, but not what is inside the other folders.
    /// It does not check those other folders.
    ///
    /// ### Why is this bad?
    ///
    /// A project that has a convention for its folders needs a check for it.
    /// Without the check, the convention drifts: a page has no owner file, or a helper folder grows in the wrong place.
    ///
    /// ### Options
    ///
    /// * `folders`: a list of entries. Each entry has a `path` and at least one of the other keys.
    ///   * `path`: globs of the folders to check. `{name}` captures a path segment.
    ///     A glob matches only the folder it names, so `src/pages/{page}` does not match `src/pages/a/components`.
    ///     A glob can also match a file, such as `src/components/**/{c}.tsx`. Then the checks apply to the folder of the file.
    ///     A glob that starts with `!` removes folders from the match.
    ///   * `require`: globs of the files and folders that the folder must have. A glob is relative to the folder and can contain `/`.
    ///     `{name}` stands for the capture of `path`, and `path` must capture it. `{a,b}` means one of `a` and `b`.
    ///   * `allow`: globs of the names that the files and folders directly in the folder can have.
    ///     Deeper levels need their own entry. A glob that starts with `!` forbids a name.
    ///     A list of only such globs allows every other name.
    ///   * `case`: `kebab-case`, `camel-case`, `pascal-case` or `snake-case`.
    ///     The rule checks the name of each file and folder directly in the folder, before its first `.`.
    ///   * `message`: Oxcruise shows this text instead of the generic message.
    ///
    /// When several entries match one folder, all of them apply.
    /// To make an exception for one folder, narrow `path` with a glob that starts with `!`.
    /// To switch the rule off for an area, use `overrides`.
    ///
    /// ### Ignoring a violation
    ///
    /// A violation about a missing file, or about a file that is not a module, has no place for a comment.
    /// Change the config file for these.
    /// A `// oxcruise-ignore-file folder-structure` comment in a module silences the violations about that module.
    ///
    /// ### Example
    ///
    /// Every page has an owner file, a module has no `utils` folder, and components have a test next to them:
    ///
    /// ```json
    /// { "folders": [
    ///   { "path": "src/pages/{page}", "require": ["codeowners.ts", "index.{ts,tsx}"] },
    ///   { "path": "src/modules/{m}", "allow": "!utils", "message": "Put shared helpers in src/lib." },
    ///   { "path": "src/components", "case": "pascal-case" },
    ///   { "path": ["src/components/**/{c}.tsx", "!**/*.{test,stories}.tsx"], "require": "{c}.test.tsx" }
    /// ] }
    /// ```
    FolderStructure, category = style, version = "0.1.0", config = FolderStructureConfig
}

/// One glob of `require`.
#[derive(Debug)]
enum Required {
    /// Text and references only: it names one path, found by a lookup.
    Literal(Literal),
    /// A real glob, matched against the paths below the folder.
    Glob {
        text: String,
        pattern: PathPatterns,
        /// The glob reaches into folders below the folder.
        deep: bool,
    },
}

#[derive(Debug)]
struct Entry {
    path: FolderPath,
    require: Vec<Required>,
    allow: Option<PathPatterns>,
    case: Option<Case>,
    message: Option<String>,
}

#[derive(Debug, Default)]
pub struct FolderStructure {
    entries: Vec<Entry>,
}

impl Entry {
    /// Every error of the entry, not the first.
    fn from_config(c: FolderStructureFolder, where_: &str) -> Result<Entry, Vec<String>> {
        let mut errors = vec![];
        if c.require.is_none() && c.allow.is_none() && c.case.is_none() {
            errors.push(format!(
                "{where_}: needs at least one of `require`, `allow` and `case`"
            ));
        }
        let path = match FolderPath::new(&c.path, where_) {
            Ok(p) => Some(p),
            Err(e) => {
                errors.push(e);
                None
            }
        };
        let mut check_refs = |key: &str, globs: &[String]| {
            if let Some(path) = &path {
                errors.extend(path.check_references(globs, key, where_));
            }
        };

        let require_globs: Vec<String> = c.require.iter().flat_map(Paths::to_vec).collect();
        check_refs("require", &require_globs);
        let allow_globs: Vec<String> = c.allow.iter().flat_map(Paths::to_vec).collect();
        check_refs("allow", &allow_globs);

        let mut require = vec![];
        for glob in &require_globs {
            if glob.starts_with('!') || glob.is_empty() {
                errors.push(format!(
                    "{where_}: `require` takes globs that name something: `{glob}`"
                ));
                continue;
            }
            let text = glob.strip_prefix("./").unwrap_or(glob).to_string();
            match Literal::parse(&text) {
                Some(literal) => require.push(Required::Literal(literal)),
                None => match PathPatterns::exact(std::slice::from_ref(&text), Role::Referencing) {
                    Ok(pattern) => require.push(Required::Glob {
                        pattern,
                        deep: text.contains('/') || text.contains("**"),
                        text,
                    }),
                    Err(e) => errors.push(format!("{where_}: `require`: {e}")),
                },
            }
        }
        let allow = match &c.allow {
            Some(_) if allow_globs.is_empty() => {
                errors.push(format!("{where_}: `allow` needs a glob"));
                None
            }
            Some(_) => match PathPatterns::exact(&allow_globs, Role::Referencing) {
                Ok(p) => Some(p),
                Err(e) => {
                    errors.push(format!("{where_}: `allow`: {e}"));
                    None
                }
            },
            None => None,
        };
        match (path, errors.is_empty()) {
            (Some(path), true) => Ok(Entry {
                path,
                require,
                allow,
                case: c.case,
                message: c.message,
            }),
            _ => Err(errors),
        }
    }

    /// The violations for the folder `folder` (the checks of this entry only), as
    /// `(path, message)`.
    fn check(
        &self,
        tree: &FileTree,
        folder: FolderId,
        caps: &Captures,
        out: &mut Vec<(String, String)>,
    ) {
        let dir = tree.folder(folder);
        let message = |generic: String| self.message.clone().unwrap_or(generic);

        for required in &self.require {
            match required {
                Required::Literal(literal) => {
                    let Some(name) = literal.resolve(caps) else {
                        continue;
                    };
                    let full = join(dir, &name);
                    if !tree.contains(&full) {
                        out.push((
                            full,
                            message(format!("`{}` must contain `{name}`", label(dir))),
                        ));
                    }
                }
                Required::Glob {
                    text,
                    pattern,
                    deep,
                } => {
                    let found = if *deep {
                        let prefix = if dir.is_empty() {
                            String::new()
                        } else {
                            format!("{dir}/")
                        };
                        tree.files_with_prefix(&prefix)
                            .iter()
                            .chain(tree.folders_with_prefix(&prefix).1)
                            .any(|p| {
                                p.len() > prefix.len() && pattern.is_match(&p[prefix.len()..], caps)
                            })
                    } else {
                        children(tree, folder).any(|(_, name)| pattern.is_match(name, caps))
                    };
                    if !found {
                        let wanted = fill_captures(text, caps);
                        out.push((
                            join(dir, &wanted),
                            message(format!("`{}` must contain `{wanted}`", label(dir))),
                        ));
                    }
                }
            }
        }

        if self.allow.is_none() && self.case.is_none() {
            return;
        }
        for (path, name) in children(tree, folder) {
            if let Some(allow) = &self.allow
                && !allow.is_match(name, caps)
            {
                out.push((
                    path.to_string(),
                    message(format!("`{name}` is not allowed in `{}`", label(dir))),
                ));
            }
            if let Some(case) = self.case {
                // `.eslintrc` has no name before its first `.`
                let stem = name.split('.').next().unwrap_or(name);
                if !stem.is_empty() && !case.accepts(stem) {
                    out.push((
                        path.to_string(),
                        message(format!(
                            "`{name}` in `{}` must be in {}",
                            label(dir),
                            case.as_str()
                        )),
                    ));
                }
            }
        }
    }
}

/// The folders and files directly in `folder`, as `(path, name)`.
fn children(tree: &FileTree, folder: FolderId) -> impl Iterator<Item = (&str, &str)> {
    let folders = tree
        .child_folders(folder)
        .iter()
        .map(|&f| tree.folder(f))
        .map(|p| (p, base_name(p)));
    let files = tree
        .child_files(folder)
        .iter()
        .map(|&i| tree.file(i))
        .map(|p| (p, base_name(p)));
    folders.chain(files)
}

impl Rule for FolderStructure {
    const HOOKS: Hooks = Hooks::GRAPH;

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: FolderStructureConfig = parse_options(Self::NAME, value)?;
        let mut entries = vec![];
        let mut errors = vec![];
        for (i, folder) in c.folders.into_iter().enumerate() {
            match Entry::from_config(folder, &format!("folders[{i}]")) {
                Ok(e) => entries.push(e),
                Err(e) => errors.extend(e),
            }
        }
        if errors.is_empty() {
            Ok(Self { entries })
        } else {
            Err(format!(
                "invalid options for `{}`: {}",
                Self::NAME,
                errors.join("; ")
            ))
        }
    }

    fn requirements(&self) -> Analyses {
        Analyses::FILE_TREE
    }

    fn check_graph(&self, cx: &RuleCtx<'_>) {
        let tree = cx.file_tree();
        // every folder to check, with the entry and the captures of its `path`
        let mut targets: Vec<(usize, FolderId, Captures)> = vec![];
        for (i, entry) in self.entries.iter().enumerate() {
            let prefix = entry.path.prefix();
            // the root folder is `.`
            if let Some(caps) = entry.path.capture(".") {
                targets.push((i, FolderId::ROOT, caps));
            }
            let (first, folders) = tree.folders_with_prefix(prefix);
            targets.extend(
                folders
                    .par_iter()
                    .enumerate()
                    .filter(|(k, _)| first.idx() + k > 0)
                    .filter_map(|(k, f)| {
                        let caps = entry.path.capture(f)?;
                        Some((i, FolderId((first.idx() + k) as u32), caps))
                    })
                    .collect::<Vec<_>>(),
            );
            // a file that `path` matches stands for its folder
            targets.extend(
                tree.files_with_prefix(prefix)
                    .par_iter()
                    .filter_map(|f| {
                        let caps = entry.path.capture(f)?;
                        let folder = tree.folder_id(parent_path(f))?;
                        Some((i, folder, caps))
                    })
                    .collect::<Vec<_>>(),
            );
        }

        // A folder is its own target once per file that `path` matches in it, and the same check
        // would run for each; only a different capture makes a different check.
        targets.retain(|(_, folder, _)| tree.is_listed(*folder));
        targets.sort_by_key(|(i, folder, _)| (*i, *folder));
        targets.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);

        let mut found: Vec<(String, String)> = targets
            .par_iter()
            .fold(Vec::new, |mut out, (i, folder, caps)| {
                self.entries[*i].check(tree, *folder, caps, &mut out);
                out
            })
            .reduce(Vec::new, |mut a, mut b| {
                a.append(&mut b);
                a
            });
        // one finding for a path and a message, however many entries and files led to it
        found.sort_unstable();
        found.dedup();
        for (path, message) in found {
            cx.report_path(&path, message);
        }
    }
}
