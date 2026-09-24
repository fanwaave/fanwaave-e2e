use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::json;
use toml::Value;
use tree_sitter::{Language, Node, Parser};

use crate::model::{CommandReport, Finding};

const CLI_CONTRACT: &str = ".cli-flags.toml";
const GENERATED_ROOT: &str = "generated";
const MAX_INPUT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
enum LanguageKind {
    Rust,
    TypeScript,
    Dart,
    Gleam,
}

impl LanguageKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::Dart => "dart",
            Self::Gleam => "gleam",
        }
    }

    fn grammar(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Dart => tree_sitter_dart::LANGUAGE.into(),
            Self::Gleam => tree_sitter_gleam::LANGUAGE.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct GeneratedSurface {
    path: &'static str,
    language: LanguageKind,
}

const SURFACES: [GeneratedSurface; 4] = [
    GeneratedSurface {
        path: "generated/rust/env.rs",
        language: LanguageKind::Rust,
    },
    GeneratedSurface {
        path: "generated/typescript/env.ts",
        language: LanguageKind::TypeScript,
    },
    GeneratedSurface {
        path: "generated/dart/env.dart",
        language: LanguageKind::Dart,
    },
    GeneratedSurface {
        path: "generated/gleam/env.gleam",
        language: LanguageKind::Gleam,
    },
];

/// Compare environment keys declared by `.cli-flags.toml` with frozen
/// generated language surfaces using AST string nodes rather than grep alone.
pub(crate) fn audit_generated_env_parity(root: &Path, report: &mut CommandReport) {
    let Some(declared) = load_declared_env_keys(root, report) else {
        return;
    };
    let generated_root = root.join(GENERATED_ROOT);
    if !generated_root.is_dir() {
        report.insert_metadata("generatedEnvParitySurfaceCount", json!(0));
        return;
    }

    let mut checked = 0_u64;
    let mut supported_dirs = 0_u64;
    for surface in SURFACES {
        let path = root.join(surface.path);
        let Some(parent) = path.parent() else {
            continue;
        };
        if !parent.is_dir() {
            continue;
        }
        supported_dirs += 1;
        if !path.exists() {
            report.push(
                Finding::error(
                    "generated-env-surface-missing",
                    "generated language directory exists but its flags-2-env environment surface is missing",
                )
                .with_target(surface.path)
                .with_detail("language", json!(surface.language.label())),
            );
            continue;
        }
        let Some(observed) = parse_generated_env_surface(root, surface, report) else {
            continue;
        };
        checked += 1;

        let missing = declared.difference(&observed).cloned().collect::<Vec<_>>();
        if !missing.is_empty() {
            report.push(
                Finding::error(
                    "generated-env-surface-missing-keys",
                    "frozen generated environment surface is missing keys declared by .cli-flags.toml",
                )
                .with_target(surface.path)
                .with_detail("language", json!(surface.language.label()))
                .with_detail("missingKeys", json!(missing)),
            );
        }

        let stale = observed.difference(&declared).cloned().collect::<Vec<_>>();
        if !stale.is_empty() {
            report.push(
                Finding::error(
                    "generated-env-surface-stale-keys",
                    "frozen generated environment surface contains env keys no longer emitted by .cli-flags.toml",
                )
                .with_target(surface.path)
                .with_detail("language", json!(surface.language.label()))
                .with_detail("staleKeys", json!(stale)),
            );
        }
    }

    report.insert_metadata("generatedEnvDeclaredKeyCount", json!(declared.len()));
    report.insert_metadata("generatedEnvSupportedDirectoryCount", json!(supported_dirs));
    report.insert_metadata("generatedEnvParitySurfaceCount", json!(checked));
}

fn load_declared_env_keys(root: &Path, report: &mut CommandReport) -> Option<BTreeSet<String>> {
    let path = root.join(CLI_CONTRACT);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-env-cli-contract-unreadable",
                    format!(".cli-flags.toml metadata could not be inspected: {error}"),
                )
                .with_target(CLI_CONTRACT),
            );
            return None;
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_INPUT_BYTES {
        report.push(
            Finding::error(
                "generated-env-cli-contract-unsafe",
                ".cli-flags.toml must be a bounded regular file before generated parity can be trusted",
            )
            .with_target(CLI_CONTRACT),
        );
        return None;
    }
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-env-cli-contract-unreadable",
                    format!(".cli-flags.toml could not be read as UTF-8: {error}"),
                )
                .with_target(CLI_CONTRACT),
            );
            return None;
        }
    };
    let document = match toml::from_str::<Value>(&text) {
        Ok(document) => document,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-env-cli-contract-invalid",
                    format!(".cli-flags.toml could not be parsed for generated parity: {error}"),
                )
                .with_target(CLI_CONTRACT),
            );
            return None;
        }
    };

    let mut declared = BTreeSet::new();
    collect_declared_env_keys(&document, None, &mut declared);
    Some(declared)
}

fn collect_declared_env_keys(value: &Value, field: Option<&str>, out: &mut BTreeSet<String>) {
    match value {
        Value::Table(table) => {
            for (child_field, child) in table {
                collect_declared_env_keys(child, Some(child_field), out);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_declared_env_keys(child, field, out);
            }
        }
        Value::String(text) => {
            let Some(field) = field else {
                return;
            };
            if (field == "env" || parse_channel_env_field(field)) && valid_env_key(text) {
                out.insert(text.to_owned());
            }
        }
        _ => {}
    }
}

fn parse_channel_env_field(field: &str) -> bool {
    matches!(
        field,
        "command_env" | "positionals_env" | "unknown_options_env" | "errors_env"
    )
}

fn parse_generated_env_surface(
    root: &Path,
    surface: GeneratedSurface,
    report: &mut CommandReport,
) -> Option<BTreeSet<String>> {
    let path = root.join(surface.path);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-env-surface-unreadable",
                    format!("generated env surface metadata could not be inspected: {error}"),
                )
                .with_target(surface.path),
            );
            return None;
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_INPUT_BYTES {
        report.push(
            Finding::error(
                "generated-env-surface-unsafe",
                "generated env surface must be a bounded regular file",
            )
            .with_target(surface.path)
            .with_detail("language", json!(surface.language.label())),
        );
        return None;
    }
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-env-surface-unreadable",
                    format!("generated env surface could not be read as UTF-8: {error}"),
                )
                .with_target(surface.path),
            );
            return None;
        }
    };

    let mut parser = Parser::new();
    let grammar = surface.language.grammar();
    if let Err(error) = parser.set_language(&grammar) {
        report.push(
            Finding::error(
                "generated-env-parser-init-failed",
                format!("generated env surface parser could not initialize: {error}"),
            )
            .with_target(surface.path)
            .with_detail("language", json!(surface.language.label())),
        );
        return None;
    }
    let Some(tree) = parser.parse(source.as_bytes(), None) else {
        report.push(
            Finding::error(
                "generated-env-parser-returned-none",
                "generated env surface parser returned no syntax tree",
            )
            .with_target(surface.path)
            .with_detail("language", json!(surface.language.label())),
        );
        return None;
    };
    if tree.root_node().has_error() {
        report.push(
            Finding::error(
                "generated-env-surface-syntax-error",
                "generated env surface contains AST syntax errors and cannot satisfy cross-language parity",
            )
            .with_target(surface.path)
            .with_detail("language", json!(surface.language.label())),
        );
        return None;
    }

    let mut observed = BTreeSet::new();
    collect_env_string_nodes(tree.root_node(), source.as_bytes(), &mut observed);
    Some(observed)
}

fn collect_env_string_nodes(node: Node<'_>, source: &[u8], out: &mut BTreeSet<String>) {
    if node.kind().contains("string") {
        if let Ok(raw) = node.utf8_text(source) {
            if let Some(value) = unquote(raw).filter(|value| valid_env_key(value)) {
                out.insert(value.to_owned());
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_env_string_nodes(child, source, out);
    }
}

fn unquote(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.len() < 2 {
        return None;
    }
    let bytes = value.as_bytes();
    if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
        || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        || (bytes[0] == b'`' && bytes[value.len() - 1] == b'`')
    {
        return Some(&value[1..value.len() - 1]);
    }
    None
}

fn valid_env_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_uppercase() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::audit_generated_env_parity;
    use crate::model::CommandReport;

    fn has(report: &CommandReport, code: &str) -> bool {
        report.findings.iter().any(|finding| finding.code == code)
    }

    #[test]
    fn rust_generated_env_surface_matches_cli_contract() {
        let root = tempdir().expect("temporary repository");
        fs::write(
            root.path().join(".cli-flags.toml"),
            "[flags.api]\nenv = \"API_URL\"\ntype = \"string\"\n",
        )
        .expect("flags contract");
        fs::create_dir_all(root.path().join("generated/rust")).expect("generated dir");
        fs::write(
            root.path().join("generated/rust/env.rs"),
            "pub const API: &str = \"API_URL\";\n",
        )
        .expect("generated env");

        let mut report = CommandReport::new("generated env parity test");
        audit_generated_env_parity(root.path(), &mut report);
        assert!(!has(&report, "generated-env-surface-missing-keys"));
        assert!(!has(&report, "generated-env-surface-stale-keys"));
    }

    #[test]
    fn stale_generated_env_key_is_rejected() {
        let root = tempdir().expect("temporary repository");
        fs::write(
            root.path().join(".cli-flags.toml"),
            "[flags.api]\nenv = \"API_URL\"\ntype = \"string\"\n",
        )
        .expect("flags contract");
        fs::create_dir_all(root.path().join("generated/rust")).expect("generated dir");
        fs::write(
            root.path().join("generated/rust/env.rs"),
            "pub const API: &str = \"API_BASE\";\n",
        )
        .expect("generated env");

        let mut report = CommandReport::new("generated env parity test");
        audit_generated_env_parity(root.path(), &mut report);
        assert!(has(&report, "generated-env-surface-missing-keys"));
        assert!(has(&report, "generated-env-surface-stale-keys"));
    }

    #[test]
    fn malformed_generated_surface_fails_closed() {
        let root = tempdir().expect("temporary repository");
        fs::write(
            root.path().join(".cli-flags.toml"),
            "[flags.api]\nenv = \"API_URL\"\ntype = \"string\"\n",
        )
        .expect("flags contract");
        fs::create_dir_all(root.path().join("generated/rust")).expect("generated dir");
        fs::write(
            root.path().join("generated/rust/env.rs"),
            "pub const API: &str = \"API_URL\"\nfn broken( {\n",
        )
        .expect("generated env");

        let mut report = CommandReport::new("generated env parity test");
        audit_generated_env_parity(root.path(), &mut report);
        assert!(has(&report, "generated-env-surface-syntax-error"));
    }
}
