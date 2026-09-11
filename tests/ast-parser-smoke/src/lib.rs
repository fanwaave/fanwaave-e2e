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

    fn likely_stale_alias(candidate: &str, expected: &str) -> bool {
        if candidate == expected {
            return false;
        }
        if candidate.starts_with(expected) || expected.starts_with(candidate) {
            return true;
        }
        let candidate_parts = candidate.split('_').collect::<Vec<_>>();
        let expected_parts = expected.split('_').collect::<Vec<_>>();
        candidate_parts
            .iter()
            .zip(expected_parts.iter())
            .take_while(|(left, right)| left == right)
            .count()
            >= 3
    }

    fn secret_like(value: &str) -> bool {
        let words = value
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();

        if words.iter().any(|word| {
            matches!(
                word.as_str(),
                "secret" | "password" | "passwd" | "credential" | "credentials"
            )
        }) {
            return true;
        }
        if words.len() == 1 && matches!(words[0].as_str(), "token" | "apikey" | "hmac") {
            return true;
        }
        words.windows(2).any(|pair| {
            matches!(
                (pair[0].as_str(), pair[1].as_str()),
                ("api", "key")
                    | ("auth", "token")
                    | ("access", "token")
                    | ("refresh", "token")
                    | ("bearer", "token")
                    | ("session", "token")
                    | ("private", "key")
                    | ("hmac", "key")
                    | ("signing", "key")
                    | ("client", "secret")
            )
        })
    }

    fn is_sha40(value: &str) -> bool {
        value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    fn line_has_pin(line: &str) -> bool {
        if let Some((_, fragment)) = line.rsplit_once('#') {
            let candidate = fragment
                .trim()
                .trim_matches(|ch: char| matches!(ch, '"' | '\'' | ',' | '}' | ']' | ' '));
            if is_sha40(candidate) {
                return true;
            }
        }
        for marker in ["rev", "ref", "commit"] {
            let Some(position) = line.find(marker) else {
                continue;
            };
            let tail = &line[position + marker.len()..];
            for token in tail.split(|ch: char| {
                ch.is_ascii_whitespace()
                    || matches!(ch, '=' | ':' | '"' | '\'' | ',' | '{' | '}' | '[' | ']')
            }) {
                if is_sha40(token) {
                    return true;
                }
            }
        }
        false
    }

    fn canonical_git_source_is_pinned(text: &str) -> bool {
        const SOURCE: &str = "github.com/flags-2-env/flags-2-env";
        let lines = text.lines().collect::<Vec<_>>();
        lines.iter().enumerate().all(|(index, line)| {
            if !line.contains(SOURCE) {
                return true;
            }
            let start = index.saturating_sub(2);
            let end = usize::min(lines.len(), index + 5);
            lines[start..end].iter().any(|line| line_has_pin(line))
        })
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
    fn generated_runtime_partial_regeneration_is_detectable_without_control_key_noise() {
        let declared = BTreeSet::from(["FANWAAVE_API_BASE_URL".to_owned()]);
        let observed = env_literals(
            tree_sitter_rust::LANGUAGE.into(),
            r#"pub fn load() {
                let _ = "FANWAAVE_API_BASE";
                let _ = "FLAGS2ENV_DOTENV";
            }"#,
        );
        let missing = declared.difference(&observed).cloned().collect::<Vec<_>>();
        let stale = observed
            .difference(&declared)
            .filter(|candidate| missing.iter().any(|expected| likely_stale_alias(candidate, expected)))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(missing, vec!["FANWAAVE_API_BASE_URL".to_owned()]);
        assert_eq!(stale, vec!["FANWAAVE_API_BASE".to_owned()]);
        assert!(!stale.contains(&"FLAGS2ENV_DOTENV".to_owned()));
    }

    #[test]
    fn secret_flag_classifier_catches_credential_classes() {
        for name in [
            "FANWAAVE_AUTH_TOKEN",
            "PROVIDER_API_KEY",
            "ORES_RL_HMAC_KEY",
            "DATABASE_PASSWORD",
            "OAUTH_CLIENT_SECRET",
            "SIGNING_PRIVATE_KEY",
        ] {
            assert!(secret_like(name), "expected secret classification for {name}");
        }
    }

    #[test]
    fn secret_flag_classifier_avoids_policy_false_positives() {
        for name in [
            "TOKEN_BUCKET_POLICY",
            "TOKEN_BUCKET_CAPACITY",
            "AUTH_MODE",
            "PUBLIC_API_BASE_URL",
            "KEY_VERSION",
        ] {
            assert!(!secret_like(name), "unexpected secret classification for {name}");
        }
    }

    #[test]
    fn flags2env_canonical_git_sources_require_full_commit_pins() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert!(canonical_git_source_is_pinned(&format!(
            "flags2env = {{ git = \"https://github.com/flags-2-env/flags-2-env.git\", rev = \"{sha}\" }}"
        )));
        assert!(canonical_git_source_is_pinned(&format!(
            "flags2env:\n  git:\n    url: https://github.com/flags-2-env/flags-2-env.git\n    ref: {sha}"
        )));
        assert!(canonical_git_source_is_pinned(&format!(
            "\"flags2env\": \"git+https://github.com/flags-2-env/flags-2-env.git#{sha}\""
        )));
        assert!(!canonical_git_source_is_pinned(
            "flags2env = { git = \"https://github.com/flags-2-env/flags-2-env.git\", branch = \"main\" }"
        ));
    }

    #[test]
    fn retired_flags2env_owner_is_unambiguously_distinct_from_canonical_source() {
        let retired = "https://github.com/ORESoftware/flags-2-env.git";
        let canonical = "https://github.com/flags-2-env/flags-2-env.git";
        assert!(retired.contains("github.com/ORESoftware/flags-2-env"));
        assert!(!canonical.contains("github.com/ORESoftware/flags-2-env"));
        assert!(canonical.contains("github.com/flags-2-env/flags-2-env"));
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
