//! What modules export, which names their importers use, and what `package.json` declares.
//!
//! Filled only when a rule, plugin or output asks for symbols; empty otherwise.

use serde::{Deserialize, Serialize};

use crate::{Loc, ModuleId};

/// The names an importer uses from the module an edge points to.
///
/// A dotted name is a member of an export: `ns.a` for a namespace import only used as `ns.a`,
/// `E.X` for an enum only used as `E.X`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsedNames {
    /// Anything (the import cannot be narrowed, or it is not a static ES import).
    #[default]
    All,
    Names(Box<[Box<str>]>),
}

impl UsedNames {
    pub fn none() -> Self {
        UsedNames::Names(Box::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportKind {
    /// `export const a`, `export { a }`, `export default x`
    Local,
    /// `export { a as b } from "x"`
    Reexport,
    /// `export * from "x"`
    Star,
    /// `export * as ns from "x"`
    Namespace,
}

/// A member of an exported enum or TypeScript namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportMember {
    pub name: String,
    pub loc: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Export {
    /// The name importers use (`default` for the default export, `*` for `export *`).
    pub name: String,
    pub kind: ExportKind,
    pub is_type: bool,
    /// Module a re-export reads from.
    pub from: Option<ModuleId>,
    /// Name read from `from` (`a` in `export { a as b } from "x"`).
    pub imported: Option<String>,
    pub loc: Loc,
    /// Rules silenced for this export by a comment; `"*"` means all.
    pub suppressed: Vec<String>,
    pub members: Vec<ExportMember>,
    /// The exported binding is also referenced elsewhere in its own file.
    pub used_in_file: bool,
    /// Earlier export of the same binding under another name.
    pub duplicate_of: Option<String>,
    /// Byte range to delete to remove the export from its file, when that is safe.
    #[serde(default)]
    pub fix: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DepSection {
    Dependencies,
    DevDependencies,
    PeerDependencies,
    OptionalDependencies,
}

impl DepSection {
    pub const ALL: [DepSection; 4] = [
        DepSection::Dependencies,
        DepSection::DevDependencies,
        DepSection::PeerDependencies,
        DepSection::OptionalDependencies,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DepSection::Dependencies => "dependencies",
            DepSection::DevDependencies => "devDependencies",
            DepSection::PeerDependencies => "peerDependencies",
            DepSection::OptionalDependencies => "optionalDependencies",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredDep {
    pub name: String,
    pub section: DepSection,
    /// Position of the `"name": "range"` member in package.json.
    pub loc: Loc,
    /// Byte range of that member (without a trailing comma), for `--fix`.
    pub range: (u32, u32),
    /// Peer dependencies of the installed package, which count as used when it is.
    pub peers: Vec<String>,
}

/// One `package.json` that owns modules of the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredManifest {
    /// Path of the package.json relative to the cruise root.
    pub rel_path: String,
    /// Directory of the package.json relative to the cruise root (`""` for the root).
    pub rel_dir: String,
    pub name: Option<String>,
    pub deps: Vec<DeclaredDep>,
    /// Every package name this manifest declares in any section.
    pub declared: Vec<String>,
    /// Files the manifest publishes as its interface (`main`, `module`, `types`, `bin`,
    /// `exports`), relative to its directory without a leading `./`. May contain `*`.
    pub entry_files: Vec<String>,
    /// `outDir` and `rootDir` of the `tsconfig.json` next to it, to map those files back to source.
    pub out_dir: Option<String>,
    pub root_dir: Option<String>,
}
