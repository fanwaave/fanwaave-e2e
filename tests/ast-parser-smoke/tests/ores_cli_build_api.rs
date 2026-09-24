use std::collections::BTreeSet;

use serde_json::Value as JsonValue;
use tree_sitter::{Language, Parser};

fn grammars() -> [Language; 4] {
    [
        tree_sitter_rust::LANGUAGE.into(),
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        tree_sitter_dart::LANGUAGE.into(),
        tree_sitter_gleam::LANGUAGE.into(),
    ]
}

#[test]
fn ores_cli_3386264_tree_sitter_api_compiles_and_loads() {
    for language in grammars() {
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .expect("ores-cli pinned grammar must be ABI-compatible");
    }
}

#[test]
fn ores_cli_3386264_json_defs_key_extraction_compiles_on_serde_1_0_151() {
    let models = serde_json::from_str::<JsonValue>(
        r#"{"$defs":{"Account":{"type":"object"},"Event":{"type":"object"}}}"#,
    )
    .ok()
    .and_then(|value| value.get("$defs")?.as_object().cloned())
    .map(|definitions| {
        definitions
            .into_iter()
            .map(|(key, _)| key)
            .collect::<BTreeSet<_>>()
    })
    .unwrap_or_default();

    assert_eq!(models, BTreeSet::from(["Account".to_owned(), "Event".to_owned()]));
}
