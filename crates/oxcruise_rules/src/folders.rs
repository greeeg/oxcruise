//! The `path` of a `folders` entry, which `folder-structure`, `visibility` and
//! `circular-dependencies` share.
//!
//! A `path` is a list of globs for one folder. `{name}` captures one segment, and the other
//! options of the entry can use the capture. A glob matches only the folder it names, not the
//! folders below it, so `src/pages/{page}` does not match `src/pages/a/components`.

use oxcruise_config::Paths;

use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug)]
pub struct FolderPath {
    patterns: PathPatterns,
}

impl FolderPath {
    /// `where_` names the entry in the error, for example `folders[2]`.
    pub fn new(path: &Paths, where_: &str) -> Result<FolderPath, String> {
        let globs = path.to_vec();
        if globs.is_empty() || globs.iter().any(String::is_empty) {
            return Err(format!("{where_}: `path` needs a glob"));
        }
        let patterns = PathPatterns::exact(&globs, Role::Capturing)
            .map_err(|e| format!("{where_}: `path`: {e}"))?;
        Ok(FolderPath { patterns })
    }

    /// The text that every matching path starts with. A filter for the paths to try.
    pub fn prefix(&self) -> &str {
        self.patterns.literal_prefix()
    }

    /// The captures if `path` matches. The root folder is `.`.
    pub fn capture(&self, path: &str) -> Option<Captures> {
        self.patterns.capture(path)
    }
}

/// `folder/name`, where the root folder is `""`.
pub fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_string()
    } else {
        format!("{folder}/{name}")
    }
}

/// The folder as a user reads it: the root folder is `.`.
pub fn label(folder: &str) -> &str {
    if folder.is_empty() { "." } else { folder }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxcruise_config::OneOrMany;

    fn path(globs: &[&str]) -> FolderPath {
        let many = OneOrMany::Many(globs.iter().map(|g| g.to_string()).collect());
        FolderPath::new(&many, "folders[0]").unwrap()
    }

    #[test]
    fn a_path_matches_the_folder_it_names_and_captures_segments() {
        let p = path(&["src/pages/{page}"]);
        assert_eq!(p.prefix(), "src/pages/");
        assert_eq!(p.capture("src/pages/home").unwrap()["page"], "home");
        assert!(p.capture("src/pages/home/components").is_none());
        assert!(p.capture("src/pages").is_none());
    }

    #[test]
    fn negations_narrow_the_path() {
        let p = path(&["src/pages/{page}", "!src/pages/legacy"]);
        assert!(p.capture("src/pages/home").is_some());
        assert!(p.capture("src/pages/legacy").is_none());
        assert_eq!(path(&["!legacy"]).prefix(), "");
    }

    #[test]
    fn the_prefix_is_common_to_all_globs() {
        assert_eq!(path(&["src/a/**", "src/b/{x}"]).prefix(), "src/");
        assert_eq!(path(&["**"]).prefix(), "");
        assert_eq!(path(&["packages/{p}"]).prefix(), "packages/");
    }

    #[test]
    fn an_empty_path_is_an_error() {
        let err = FolderPath::new(&OneOrMany::Many(vec![]), "folders[1]").unwrap_err();
        assert_eq!(err, "folders[1]: `path` needs a glob");
        let err = FolderPath::new(&OneOrMany::One("src/[".into()), "folders[1]");
        assert!(err.is_ok() || err.unwrap_err().starts_with("folders[1]: `path`"));
    }

    #[test]
    fn join_and_label() {
        assert_eq!(join("", "a.ts"), "a.ts");
        assert_eq!(join("src", "a.ts"), "src/a.ts");
        assert_eq!(label(""), ".");
        assert_eq!(label("src"), "src");
    }
}
