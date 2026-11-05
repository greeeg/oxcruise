//! Prints the JSON schema of the config file: `just schema-json`.
fn main() {
    let schema = oxcruise_rules::schema::config_json_schema();
    println!(
        "{}",
        serde_json::to_string_pretty(&schema).expect("schema serialises")
    );
}
