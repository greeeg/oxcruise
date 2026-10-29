//! `file:` URIs <-> paths. Only what the server needs, so no `url` dependency.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;

pub fn path_to_uri(path: &Path) -> Option<Uri> {
    let mut p = path.to_str()?.replace('\\', "/");
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    let mut out = String::from("file://");
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    Uri::from_str(&out).ok()
}

pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let rest = uri.as_str().strip_prefix("file://")?;
    // skip the authority (empty, or `localhost`)
    let path = &rest[rest.find('/')?..];
    let path = percent_decode(path)?;
    let path = if cfg!(windows) {
        // `/c:/Users/x` -> `c:/Users/x`
        match path.as_bytes() {
            [b'/', d, b':', ..] if d.is_ascii_alphabetic() => path[1..].to_string(),
            _ => path,
        }
    } else {
        path
    };
    Some(PathBuf::from(path))
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn round_trips_special_characters() {
        for p in ["/a/b.ts", "/a b/ü/c#d.ts", "/tmp/日本語/x%y.ts"] {
            let uri = path_to_uri(Path::new(p)).unwrap();
            assert_eq!(uri_to_path(&uri).unwrap(), Path::new(p), "{uri:?}");
        }
    }

    #[test]
    fn reads_vscode_style_uris() {
        let uri = Uri::from_str("file:///home/me/my%20project/src/a.ts").unwrap();
        assert_eq!(
            uri_to_path(&uri).unwrap(),
            Path::new("/home/me/my project/src/a.ts")
        );
        assert!(uri_to_path(&Uri::from_str("untitled:Untitled-1").unwrap()).is_none());
    }
}
