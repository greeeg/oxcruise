use crate::OxcruiseConfig;

const RECOMMENDED: &str = include_str!("../presets/recommended.json");
const STRICT: &str = include_str!("../presets/strict.json");

/// A built-in preset by name (`oxcruise:recommended`, `oxcruise:strict`).
pub fn preset(name: &str) -> Option<OxcruiseConfig> {
    let text = match name {
        "oxcruise:recommended" => RECOMMENDED,
        "oxcruise:strict" => STRICT,
        _ => return None,
    };
    Some(serde_json::from_str(text).expect("built-in presets are valid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_parse() {
        let r = preset("oxcruise:recommended").unwrap();
        assert_eq!(r.rules.len(), 12);
        assert!(
            preset("oxcruise:strict")
                .unwrap()
                .extends
                .contains(&"oxcruise:recommended".to_string())
        );
        assert!(preset("oxcruise:nope").is_none());
    }
}
