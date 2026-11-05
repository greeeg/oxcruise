//! The files of the project and the folders above them, for rules that check the shape of the tree.

use rustc_hash::{FxHashMap, FxHashSet};

/// The index of a folder in [`FileTree::folders`]. The root folder is `FolderId(0)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FolderId(pub u32);

impl FolderId {
    pub const ROOT: FolderId = FolderId(0);

    pub fn idx(self) -> usize {
        self.0 as usize
    }
}

/// Every file the walk saw (not only the source files), with the folders derived from them.
///
/// Paths are relative to the cruise root and use `/`. The root folder has the path `""`. Files and
/// folders are sorted by path, so the paths below a folder are one slice, and the children of a
/// folder are stored in CSR form. The tree is frozen once built.
///
/// Every folder is listed (its files and folders are all in the tree), except those given to
/// [`FileTree::with_unlisted`]. Those are known to exist and nothing more, so a rule must not
/// conclude from the tree that something is missing from them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileTree {
    files: Vec<String>,
    folders: Vec<String>,
    /// Per folder: is its content in the tree?
    listed: Vec<bool>,
    child_folder_offsets: Vec<u32>,
    child_folders: Vec<FolderId>,
    child_file_offsets: Vec<u32>,
    child_files: Vec<u32>,
}

/// The last segment of a path.
pub fn base_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// The path without its last segment (`""` for a path in the root folder).
pub fn parent_path(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl FileTree {
    /// Build the tree of `files`: paths relative to the root. Duplicates are dropped.
    pub fn new(files: Vec<String>) -> FileTree {
        FileTree::with_unlisted(files, vec![])
    }

    /// Like [`FileTree::new`], with folders whose content is not in `files`: `unlisted` are
    /// folder paths, and the folders above them are derived as for a file.
    pub fn with_unlisted(mut files: Vec<String>, unlisted: Vec<String>) -> FileTree {
        files.sort_unstable();
        files.dedup();

        // every folder that holds a file, and every folder above them
        let mut seen: FxHashSet<&str> = FxHashSet::default();
        seen.insert("");
        for dir in files
            .iter()
            .map(|f| parent_path(f))
            .chain(unlisted.iter().map(String::as_str))
        {
            let mut dir = dir;
            while seen.insert(dir) {
                dir = parent_path(dir);
            }
        }
        let mut folders: Vec<String> = seen.iter().map(|p| p.to_string()).collect();
        drop(seen);
        folders.sort_unstable();
        let mut listed = vec![true; folders.len()];
        for dir in &unlisted {
            if let Ok(i) = folders.binary_search(dir) {
                listed[i] = false;
            }
        }

        let ids: FxHashMap<&str, u32> = folders
            .iter()
            .enumerate()
            .map(|(i, f)| (f.as_str(), i as u32))
            .collect();
        let n = folders.len();
        let mut folder_counts = vec![0u32; n + 1];
        let mut file_counts = vec![0u32; n + 1];
        // the root (id 0) has no parent
        for f in folders.iter().skip(1) {
            folder_counts[ids[parent_path(f)] as usize + 1] += 1;
        }
        let file_parent: Vec<u32> = files.iter().map(|f| ids[parent_path(f)]).collect();
        for &p in &file_parent {
            file_counts[p as usize + 1] += 1;
        }
        for i in 0..n {
            folder_counts[i + 1] += folder_counts[i];
            file_counts[i + 1] += file_counts[i];
        }
        let (child_folder_offsets, child_file_offsets) =
            (folder_counts.clone(), file_counts.clone());
        let mut child_folders = vec![FolderId(0); folders.len().saturating_sub(1)];
        let mut child_files = vec![0u32; files.len()];
        let (mut folder_cursor, mut file_cursor) = (folder_counts, file_counts);
        // sorted input, so each child list comes out sorted
        for (i, f) in folders.iter().enumerate().skip(1) {
            let p = ids[parent_path(f)] as usize;
            child_folders[folder_cursor[p] as usize] = FolderId(i as u32);
            folder_cursor[p] += 1;
        }
        for (i, &p) in file_parent.iter().enumerate() {
            child_files[file_cursor[p as usize] as usize] = i as u32;
            file_cursor[p as usize] += 1;
        }

        FileTree {
            files,
            folders,
            listed,
            child_folder_offsets,
            child_folders,
            child_file_offsets,
            child_files,
        }
    }

    /// All files, sorted by path.
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// All folders, sorted by path. The index is the [`FolderId`].
    pub fn folders(&self) -> &[String] {
        &self.folders
    }

    pub fn folder_count(&self) -> usize {
        self.folders.len()
    }

    pub fn folder(&self, f: FolderId) -> &str {
        &self.folders[f.idx()]
    }

    /// Is the content of `f` in the tree? `false` for a folder that was only seen from above.
    pub fn is_listed(&self, f: FolderId) -> bool {
        self.listed[f.idx()]
    }

    pub fn file(&self, i: u32) -> &str {
        &self.files[i as usize]
    }

    /// The folders directly in `f`, sorted by path.
    pub fn child_folders(&self, f: FolderId) -> &[FolderId] {
        let (a, b) = (
            self.child_folder_offsets[f.idx()],
            self.child_folder_offsets[f.idx() + 1],
        );
        &self.child_folders[a as usize..b as usize]
    }

    /// The files directly in `f`, as indices into [`FileTree::files`], sorted by path.
    pub fn child_files(&self, f: FolderId) -> &[u32] {
        let (a, b) = (
            self.child_file_offsets[f.idx()],
            self.child_file_offsets[f.idx() + 1],
        );
        &self.child_files[a as usize..b as usize]
    }

    pub fn folder_id(&self, path: &str) -> Option<FolderId> {
        self.folders
            .binary_search_by(|f| f.as_str().cmp(path))
            .ok()
            .map(|i| FolderId(i as u32))
    }

    pub fn has_file(&self, path: &str) -> bool {
        self.files
            .binary_search_by(|f| f.as_str().cmp(path))
            .is_ok()
    }

    /// Is there a file or a folder at `path`?
    pub fn contains(&self, path: &str) -> bool {
        self.has_file(path) || self.folder_id(path).is_some()
    }

    /// The files whose path starts with `prefix`.
    pub fn files_with_prefix(&self, prefix: &str) -> &[String] {
        prefix_range(&self.files, prefix)
    }

    /// The folders whose path starts with `prefix`, with the first one's [`FolderId`].
    pub fn folders_with_prefix(&self, prefix: &str) -> (FolderId, &[String]) {
        let start = self.folders.partition_point(|f| f.as_str() < prefix);
        (
            FolderId(start as u32),
            prefix_range(&self.folders[start..], prefix),
        )
    }
}

/// The slice of the sorted `paths` that starts with `prefix`: such paths are next to each other.
fn prefix_range<'a>(paths: &'a [String], prefix: &str) -> &'a [String] {
    let start = paths.partition_point(|p| p.as_str() < prefix);
    let len = paths[start..].partition_point(|p| p.starts_with(prefix));
    &paths[start..start + len]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[&str]) -> FileTree {
        FileTree::new(files.iter().map(|f| f.to_string()).collect())
    }

    fn names(t: &FileTree, ids: &[FolderId]) -> Vec<String> {
        ids.iter().map(|&f| t.folder(f).to_string()).collect()
    }

    #[test]
    fn folders_are_derived_from_files() {
        let t = tree(&[
            "src/pages/a/index.ts",
            "src/pages/a/codeowners.ts",
            "src/pages/b/index.ts",
            "README.md",
            "src/pages/a/index.ts",
        ]);
        assert_eq!(t.files().len(), 4, "duplicates are dropped");
        assert_eq!(
            t.folders(),
            ["", "src", "src/pages", "src/pages/a", "src/pages/b"]
        );
        assert_eq!(t.folder(FolderId::ROOT), "");
    }

    #[test]
    fn children_are_sorted_and_direct() {
        let t = tree(&[
            "src/z.ts",
            "src/a.ts",
            "src/lib/x.ts",
            "src/lib/deep/y.ts",
            "top.ts",
        ]);
        let src = t.folder_id("src").unwrap();
        assert_eq!(names(&t, t.child_folders(src)), ["src/lib"]);
        let files: Vec<&str> = t.child_files(src).iter().map(|&i| t.file(i)).collect();
        assert_eq!(files, ["src/a.ts", "src/z.ts"]);
        let root = FolderId::ROOT;
        assert_eq!(names(&t, t.child_folders(root)), ["src"]);
        let files: Vec<&str> = t.child_files(root).iter().map(|&i| t.file(i)).collect();
        assert_eq!(files, ["top.ts"]);
        // a folder with only folders in it has no files
        let lib = t.folder_id("src/lib").unwrap();
        assert_eq!(names(&t, t.child_folders(lib)), ["src/lib/deep"]);
        assert_eq!(t.child_files(lib).len(), 1);
    }

    #[test]
    fn lookups_and_prefix_slices() {
        let t = tree(&["a/b.ts", "a/c/d.ts", "a-b/e.ts", "a.ts"]);
        assert!(t.has_file("a/b.ts") && !t.has_file("a"));
        assert!(t.contains("a") && t.contains("a/c") && t.contains("a.ts"));
        assert!(!t.contains("a/x"));
        // `a-b` sorts between `a` and `a/`, but is not below `a`
        assert_eq!(t.files_with_prefix("a/"), ["a/b.ts", "a/c/d.ts"]);
        let (first, below) = t.folders_with_prefix("a/");
        assert_eq!(below, ["a/c"]);
        assert_eq!(t.folder(first), "a/c");
    }

    #[test]
    fn unlisted_folders_exist_but_are_not_listed() {
        let t = FileTree::with_unlisted(
            vec!["package.json".into(), "src/a.ts".into()],
            vec!["lib".into(), "docs/api".into()],
        );
        assert_eq!(t.folders(), ["", "docs", "docs/api", "lib", "src"]);
        let listed: Vec<bool> = (0..t.folder_count() as u32)
            .map(|i| t.is_listed(FolderId(i)))
            .collect();
        assert_eq!(listed, [true, true, false, false, true]);
        assert!(FileTree::new(vec!["a/b.ts".into()]).is_listed(FolderId(1)));
    }

    #[test]
    fn an_empty_tree_has_the_root() {
        let t = tree(&[]);
        assert_eq!(t.folders(), [""]);
        assert!(t.child_folders(FolderId::ROOT).is_empty());
        assert!(t.child_files(FolderId::ROOT).is_empty());
    }

    #[test]
    fn base_name_and_parent() {
        assert_eq!(base_name("a/b/c.ts"), "c.ts");
        assert_eq!(base_name("c.ts"), "c.ts");
        assert_eq!(parent_path("a/b/c.ts"), "a/b");
        assert_eq!(parent_path("c.ts"), "");
    }
}
