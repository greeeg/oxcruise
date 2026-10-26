/// Remove `//` and `/* */` comments and trailing commas so JSONC parses as JSON.
pub fn strip_jsonc(input: &str) -> String {
    let b = input.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                out.push(b[i]);
                i += 1;
                while i < b.len() {
                    out.push(b[i]);
                    if b[i] == b'\\' && i + 1 < b.len() {
                        out.push(b[i + 1]);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    if b[i - 1] == b'"' {
                        break;
                    }
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    // keep newlines so error positions stay meaningful
                    if b[i] == b'\n' {
                        out.push(b'\n');
                    }
                    i += 1;
                }
                i += 2;
            }
            b',' => {
                // drop a comma that is followed (ignoring whitespace/comments) by `}` or `]`
                let mut j = i + 1;
                loop {
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if b.get(j) == Some(&b'/') && b.get(j + 1) == Some(&b'/') {
                        while j < b.len() && b[j] != b'\n' {
                            j += 1;
                        }
                    } else if b.get(j) == Some(&b'/') && b.get(j + 1) == Some(&b'*') {
                        j += 2;
                        while j + 1 < b.len() && !(b[j] == b'*' && b[j + 1] == b'/') {
                            j += 1;
                        }
                        j += 2;
                    } else {
                        break;
                    }
                }
                if !matches!(b.get(j), Some(b'}' | b']')) {
                    out.push(b',');
                }
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).expect("stripping preserves utf-8")
}

#[cfg(test)]
mod tests {
    use super::strip_jsonc;

    fn parse(s: &str) -> serde_json::Value {
        serde_json::from_str(&strip_jsonc(s)).unwrap()
    }

    #[test]
    fn comments_and_trailing_commas() {
        let v = parse(
            r#"{
              // line
              "a": [1, 2, /* inline */ 3,],
              /* block
                 spanning */ "b": "x // not a comment, \" nor /* this */",
              "c": {"d": 1,},
            }"#,
        );
        assert_eq!(v["a"], serde_json::json!([1, 2, 3]));
        assert_eq!(v["b"], "x // not a comment, \" nor /* this */");
        assert_eq!(v["c"]["d"], 1);
    }

    #[test]
    fn plain_json_is_unchanged() {
        let s = r#"{"a":[1,2],"b":"c,d"}"#;
        assert_eq!(strip_jsonc(s), s);
    }
}
