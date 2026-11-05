//! `--fix`: apply the repairs that rules attach to their violations.

use std::path::Path;

use oxcruise_core::Violation;
use std::collections::HashMap;

/// A violation list split into what was repaired and what is left.
pub struct Applied {
    pub remaining: Vec<Violation>,
    pub fixed: Vec<Violation>,
}

type Edits = Vec<(u32, u32, String)>;

/// Apply the fixes of `violations`, in order. A fix is applied whole or not at all: when one of
/// its edits overlaps an edit already accepted, its violation stays and a second run repairs it.
/// Each file is written once.
pub fn apply(root: &Path, violations: Vec<Violation>) -> Result<Applied, String> {
    let mut accepted: HashMap<String, Edits> = HashMap::new();
    let (mut remaining, mut fixed) = (vec![], vec![]);
    for v in violations {
        let Some(fix) = &v.fix else {
            remaining.push(v);
            continue;
        };
        let taken = accepted.get(&fix.file);
        let clashes = fix.edits.iter().enumerate().any(|(i, (s, e, _))| {
            // against the edits of other fixes, and against the other edits of this one
            let overlaps = |(s2, e2, _): &(u32, u32, String)| s < e2 && s2 < e;
            taken.is_some_and(|t| t.iter().any(overlaps))
                || fix
                    .edits
                    .iter()
                    .enumerate()
                    .any(|(j, other)| i != j && overlaps(other))
        });
        if clashes {
            remaining.push(v);
        } else {
            accepted
                .entry(fix.file.clone())
                .or_default()
                .extend(fix.edits.iter().cloned());
            fixed.push(v);
        }
    }

    let mut files: Vec<_> = accepted.into_iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    for (file, mut edits) in files {
        let path = root.join(&file);
        let mut bytes = std::fs::read(&path).map_err(|e| format!("cannot read {file}: {e}"))?;
        // back to front, so earlier offsets stay valid
        edits.sort_by_key(|(s, _, _)| std::cmp::Reverse(*s));
        for (s, e, replacement) in edits {
            let (s, e) = (s as usize, e as usize);
            if e > bytes.len() {
                return Err(format!("{file} changed since it was read; run again"));
            }
            bytes.splice(s..e, replacement.into_bytes());
        }
        std::fs::write(&path, bytes).map_err(|e| format!("cannot write {file}: {e}"))?;
    }
    Ok(Applied { remaining, fixed })
}

#[cfg(test)]
mod tests {
    use oxcruise_core::{Fix, ModuleId, ViolationLevel};

    use super::*;

    fn violation(file: &str, edits: &[(u32, u32)]) -> Violation {
        let mut v = Violation::module("r", ViolationLevel::Error, ModuleId(0), "m".into());
        v.fix = Some(Fix {
            file: file.into(),
            edits: edits.iter().map(|&(s, e)| (s, e, String::new())).collect(),
        });
        v
    }

    #[test]
    fn edits_apply_back_to_front_and_overlaps_wait_for_the_next_run() {
        let dir = std::env::temp_dir().join(format!("oxcruise-fix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ts"), "0123456789").unwrap();
        std::fs::write(dir.join("b.ts"), "abcdef").unwrap();
        let mut plain = Violation::module("r", ViolationLevel::Warn, ModuleId(0), "no fix".into());
        plain.fix = None;
        let applied = apply(
            &dir,
            vec![
                violation("a.ts", &[(6, 8)]),
                violation("a.ts", &[(1, 3)]),
                violation("a.ts", &[(2, 4)]), // overlaps the previous one
                violation("b.ts", &[(0, 1), (4, 6)]),
                plain,
            ],
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.ts")).unwrap(), "034589");
        assert_eq!(std::fs::read_to_string(dir.join("b.ts")).unwrap(), "bcd");
        assert_eq!(applied.fixed.len(), 3);
        assert_eq!(applied.remaining.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
