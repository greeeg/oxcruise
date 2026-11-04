use std::io;
use std::path::Path;

use oxcruise_report::CruiseResult;

use crate::VizData;

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_JS: &str = include_str!("../assets/app.js");
const APP_CSS: &str = include_str!("../assets/app.css");

/// Write the site into `dir`. Only our four files are replaced; other files stay.
pub fn write_site(dir: &Path, result: &CruiseResult) -> io::Result<()> {
    write_data_site(dir, &VizData::from_result(result))
}

pub(crate) fn write_data_site(dir: &Path, data: &VizData) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("index.html"), INDEX_HTML)?;
    std::fs::write(dir.join("app.js"), APP_JS)?;
    std::fs::write(dir.join("app.css"), APP_CSS)?;
    std::fs::write(dir.join("data.js"), data_script(data))
}

/// `data.js`: the data as a string for `JSON.parse`, which is faster than an object literal and
/// opens from `file://` (a script needs no CORS).
pub fn data_script(data: &VizData) -> String {
    let json = serde_json::to_string(data).expect("viz data serialises");
    format!(
        "globalThis.OXCRUISE_DATA = JSON.parse('{}');\n",
        escape_js_string(&json)
    )
}

fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 16);
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if let Some(tail) = rest.strip_prefix("</script") {
            out.push_str("<\\/script");
            rest = tail;
            continue;
        }
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
        rest = &rest[c.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::escape_js_string;

    #[test]
    fn escapes_what_breaks_a_js_string() {
        assert_eq!(escape_js_string(r"a\b'c"), r"a\\b\'c");
        assert_eq!(escape_js_string("</script><b>"), "<\\/script><b>");
        assert_eq!(escape_js_string("x\u{2028}y\u{2029}"), "x\\u2028y\\u2029");
        assert_eq!(escape_js_string("é日本"), "é日本");
    }
}
