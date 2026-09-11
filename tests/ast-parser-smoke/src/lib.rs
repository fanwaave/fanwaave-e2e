#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use tree_sitter::{Language, Parser};

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
