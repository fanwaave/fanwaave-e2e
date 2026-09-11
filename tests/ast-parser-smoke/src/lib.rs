#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tree_sitter::{Language, Node, Parser};

    fn assert_clean_parse(language: Language, source: &str, expected_root: &str) {
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .expect("grammar must be ABI-compatible with tree-sitter");
        let tree = parser.parse(source, None).expect("parser must produce a tree");
        let root = tree.root_node();
        assert_eq!(root.kind(), expected_root);
        assert!(
            !root.has_error(),
            "parser produced ERROR/MISSING nodes for {expected_root}: {}",
            root.to_sexp()
        );
    }

    fn env_literals(language: Language, source: &str) -> BTreeSet<String> {
        let mut parser = Parser::new();
        parser.set_language(&language).expect("grammar loads");
        let tree = parser.parse(source, None).expect("tree exists");
        assert!(
            !tree.root_node().has_error(),
            "generated fixture must parse cleanly: {}",
            tree.root_node().to_sexp()
        );
        let mut out = BTreeSet::new();
        collect_env_literals(tree.root_node(), source.as_bytes(), &mut out);
        out
    }

    fn collect_env_literals(node: Node<'_>, source: &[u8], out: &mut BTreeSet<String>) {
        if node.kind().contains("string") {
            if let Ok(raw) = node.utf8_text(source) {
                if let Some(value) = unquote(raw) {
                    if valid_env_key(value) {
                        out.insert(value.to_owned());
                    }
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_env_literals(child, source, out);
        }
    }

    fn unquote(raw: &str) -> Option<&str> {
        let raw = raw.trim();
        if raw.len() < 2 {
            return None;
        }
        let bytes = raw.as_bytes();
        if (bytes[0] == b'"' && bytes[raw.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[raw.len() - 1] == b'\'')
            || (bytes[0] == b'`' && bytes[raw.len() - 1] == b'`')
        {
            Some(&raw[1..raw.len() - 1])
        } else {
            None
        }
    }

    fn valid_env_key(value: &str) -> bool {
        let mut bytes = value.bytes();
        let Some(first) = bytes.next() else {
            return false;
        };
        (first.is_ascii_uppercase() || first == b'_')
            && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    }

    #[test]
    fn rust_ast_is_clean() {
        assert_clean_parse(
            tree_sitter_rust::LANGUAGE.into(),
            r#"fn main() { let config = ".cli-flags.toml"; println!("{config}"); }"#,
            "source_file",
        );
    }

    #[test]
    fn typescript_ast_is_clean() {
        assert_clean_parse(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            r#"const config: string = '.cli-flags.toml'; console.log(config);"#,
            "program",
        );
    }

    #[test]
    fn go_ast_is_clean() {
        assert_clean_parse(
            tree_sitter_go::LANGUAGE.into(),
            r#"package main
import "fmt"
func main() { config := ".cli-flags.toml"; fmt.Println(config) }
"#,
            "source_file",
        );
    }

    #[test]
    fn gleam_ast_is_clean() {
        assert_clean_parse(
            tree_sitter_gleam::LANGUAGE.into(),
            r#"import gleam/io
pub fn main() {
  let config = ".cli-flags.toml"
  io.println(config)
}
"#,
            "source_file",
        );
    }

    #[test]
    fn dart_ast_is_clean() {
        assert_clean_parse(
            tree_sitter_dart::LANGUAGE.into(),
            r#"void main() {
  final config = '.cli-flags.toml';
  print(config);
}
"#,
            "source_file",
        );
    }

    #[test]
    fn generated_env_literals_are_ast_visible_across_fleet_languages() {
        let expected = BTreeSet::from(["FANWAAVE_API_BASE_URL".to_owned()]);
        for (language, source) in [
            (
                tree_sitter_rust::LANGUAGE.into(),
                r#"pub const API_BASE: &str = "FANWAAVE_API_BASE_URL";"#,
            ),
            (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                r#"export const API_BASE = "FANWAAVE_API_BASE_URL" as const;"#,
            ),
            (
                tree_sitter_dart::LANGUAGE.into(),
                r#"const String apiBase = 'FANWAAVE_API_BASE_URL';"#,
            ),
            (
                tree_sitter_gleam::LANGUAGE.into(),
                r#"pub const api_base = "FANWAAVE_API_BASE_URL""#,
            ),
        ] {
            assert_eq!(env_literals(language, source), expected);
        }
    }

    #[test]
    fn generated_env_parity_detects_stale_key_spelling() {
        let declared = BTreeSet::from(["FANWAAVE_API_BASE_URL".to_owned()]);
        let observed = env_literals(
            tree_sitter_rust::LANGUAGE.into(),
            r#"pub const API_BASE: &str = "FANWAAVE_API_BASE";"#,
        );
        assert_eq!(
            declared.difference(&observed).cloned().collect::<Vec<_>>(),
            vec!["FANWAAVE_API_BASE_URL".to_owned()]
        );
        assert_eq!(
            observed.difference(&declared).cloned().collect::<Vec<_>>(),
            vec!["FANWAAVE_API_BASE".to_owned()]
        );
    }

    #[test]
    fn malformed_source_is_never_treated_as_clean() {
        for (language, source) in [
            (tree_sitter_rust::LANGUAGE.into(), "fn main( {"),
            (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                "const x: = ;",
            ),
            (tree_sitter_go::LANGUAGE.into(), "package main\nfunc main( {"),
            (tree_sitter_gleam::LANGUAGE.into(), "pub fn main( {"),
            (tree_sitter_dart::LANGUAGE.into(), "void main( {"),
        ] {
            let mut parser = Parser::new();
            parser.set_language(&language).expect("grammar loads");
            let tree = parser.parse(source, None).expect("tree exists");
            assert!(tree.root_node().has_error(), "malformed source parsed cleanly");
        }
    }
}
