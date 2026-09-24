use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

use serde_json::{json, Value as JsonValue};
use toml::Value;
use tree_sitter::{Language, Node, Parser};

use crate::model::{CommandReport, Finding};

const CLI_CONTRACT: &str = ".cli-flags.toml";
const LEGACY_RPC_MANIFEST: &str = "generated/rpc/manifest.json";
const RPC_SCOPED_MANIFESTS: [(&str, &str); 2] = [
    ("generated/rpc/regular/manifest.json", "regular"),
    ("generated/rpc/admin/manifest.json", "admin"),
];
const RPC_OPERATION_INDEX: &str = "generated/rpc/server-operation-index.json";
const CANONICAL_RPC_LANGUAGES: [&str; 5] = ["rust", "go", "dart", "typescript", "gleam"];
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
struct RuntimeSurface {
    env_path: &'static str,
    runtime_path: &'static str,
    language: LanguageKind,
}

const SURFACES: [RuntimeSurface; 4] = [
    RuntimeSurface {
        env_path: "generated/rust/env.rs",
        runtime_path: "generated/rust/runtime.rs",
        language: LanguageKind::Rust,
    },
    RuntimeSurface {
        env_path: "generated/typescript/env.ts",
        runtime_path: "generated/typescript/runtime.ts",
        language: LanguageKind::TypeScript,
    },
    RuntimeSurface {
        env_path: "generated/dart/env.dart",
        runtime_path: "generated/dart/runtime.dart",
        language: LanguageKind::Dart,
    },
    RuntimeSurface {
        env_path: "generated/gleam/env.gleam",
        runtime_path: "generated/gleam/runtime.gleam",
        language: LanguageKind::Gleam,
    },
];

/// Verify that generated runtime interpreters still consume the env keys
/// emitted by `.cli-flags.toml` after the generated constant/type catalog has
/// been refreshed. Also validate tracked RPC client evidence when a repository
/// contains scoped `generated/rpc/{regular,admin}/manifest.json` output.
///
/// Generated files are evidence, never contract authorities. The source
/// contract remains `.cli-flags.toml`; RPC implementation authority is the
/// exact pinned `*-api-server.rs` handlers.rs operation index, while the route
/// map is only the optional HTTP compatibility projection.
pub(crate) fn audit_generated_runtime_parity(root: &Path, report: &mut CommandReport) {
    audit_generated_rpc_manifests(root, report);

    let Some(declared) = load_declared_env_keys(root, report) else {
        return;
    };

    let mut checked = 0_u64;
    let mut required = 0_u64;
    for surface in SURFACES {
        let env_path = root.join(surface.env_path);
        if !env_path.is_file() {
            continue;
        }
        required += 1;

        let runtime_path = root.join(surface.runtime_path);
        if !runtime_path.exists() {
            report.push(
                Finding::error(
                    "generated-runtime-surface-missing",
                    "generated env catalog exists but its runtime interpreter is missing",
                )
                .with_target(surface.runtime_path)
                .with_detail("language", json!(surface.language.label()))
                .with_detail("envSurface", json!(surface.env_path)),
            );
            continue;
        }

        let Some(observed) = parse_runtime_surface(root, surface, report) else {
            continue;
        };
        checked += 1;

        let missing = declared.difference(&observed).cloned().collect::<Vec<_>>();
        if !missing.is_empty() {
            report.push(
                Finding::error(
                    "generated-runtime-missing-keys",
                    "generated runtime interpreter does not reference every env key emitted by .cli-flags.toml",
                )
                .with_target(surface.runtime_path)
                .with_detail("language", json!(surface.language.label()))
                .with_detail("missingKeys", json!(&missing)),
            );
        }

        let stale = observed
            .difference(&declared)
            .filter(|candidate| missing.iter().any(|expected| likely_stale_alias(candidate, expected)))
            .cloned()
            .collect::<Vec<_>>();
        if !stale.is_empty() {
            report.push(
                Finding::error(
                    "generated-runtime-stale-keys",
                    "generated runtime interpreter still references env keys that look like stale aliases of current .cli-flags.toml keys",
                )
                .with_target(surface.runtime_path)
                .with_detail("language", json!(surface.language.label()))
                .with_detail("staleKeys", json!(stale)),
            );
        }
    }

    report.insert_metadata("generatedRuntimeDeclaredKeyCount", json!(declared.len()));
    report.insert_metadata("generatedRuntimeRequiredSurfaceCount", json!(required));
    report.insert_metadata("generatedRuntimeParitySurfaceCount", json!(checked));
}

fn audit_generated_rpc_manifests(root: &Path, report: &mut CommandReport) {
    if root.join(LEGACY_RPC_MANIFEST).exists() {
        report.push(
            Finding::error(
                "generated-rpc-manifest-layout-stale",
                "unscoped generated/rpc/manifest.json is stale; regenerate into regular/admin scope directories",
            )
            .with_target(LEGACY_RPC_MANIFEST),
        );
    }

    let mut checked = 0_u64;
    let mut commits = BTreeSet::new();
    for (display, expected_scope) in RPC_SCOPED_MANIFESTS {
        let manifest_path = root.join(display);
        if !manifest_path.exists() {
            continue;
        }
        checked += 1;
        let Some(manifest) = read_bounded_json(&manifest_path, display, report) else {
            continue;
        };
        let Some(object) = manifest.as_object() else {
            report.push(
                Finding::error(
                    "generated-rpc-manifest-invalid",
                    "generated RPC manifest must be a JSON object",
                )
                .with_target(display),
            );
            continue;
        };

        if object.get("schema_version").and_then(JsonValue::as_u64) != Some(2) {
            report.push(
                Finding::error(
                    "generated-rpc-manifest-version-invalid",
                    "generated RPC manifest must use the handlers-authoritative v2 evidence schema",
                )
                .with_target(display)
                .with_detail("schemaVersion", json!(object.get("schema_version"))),
            );
        }
        if object.get("source_role").and_then(JsonValue::as_str)
            != Some("api-server-handlers.rs")
        {
            report.push(
                Finding::error(
                    "generated-rpc-source-role-invalid",
                    "generated RPC clients must identify *-api-server.rs handlers.rs as their implementation source role",
                )
                .with_target(display),
            );
        }
        if object.get("http_endpoint").and_then(JsonValue::as_str) != Some("/v1/rpc") {
            report.push(
                Finding::error(
                    "generated-rpc-endpoint-invalid",
                    "generated RPC clients must use canonical POST /v1/rpc transport",
                )
                .with_target(display),
            );
        }

        let commit = object
            .get("source_commit_sha")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            report.push(
                Finding::error(
                    "generated-rpc-source-commit-invalid",
                    "generated RPC manifest must pin the exact 40-character API-server Git commit",
                )
                .with_target(display)
                .with_detail("sourceCommit", json!(commit)),
            );
        } else {
            commits.insert(commit.to_ascii_lowercase());
        }

        for digest_field in ["source_route_map_sha256", "source_operation_index_sha256"] {
            let digest = object
                .get(digest_field)
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                report.push(
                    Finding::error(
                        "generated-rpc-digest-invalid",
                        format!("generated RPC {digest_field} must be a 64-character SHA-256 digest"),
                    )
                    .with_target(display)
                    .with_detail("field", json!(digest_field)),
                );
            }
        }

        let scope = object.get("scope").and_then(JsonValue::as_str).unwrap_or_default();
        if scope != expected_scope {
            report.push(
                Finding::error(
                    "generated-rpc-scope-path-mismatch",
                    "generated RPC manifest scope must match its regular/admin directory",
                )
                .with_target(display)
                .with_detail("expectedScope", json!(expected_scope))
                .with_detail("scope", json!(scope)),
            );
        }
        let audience = object
            .get("audience")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        if !matches!(audience, "server" | "public") || (scope == "admin" && audience != "server") {
            report.push(
                Finding::error(
                    "generated-rpc-audience-invalid",
                    "generated RPC audience must be server/public, and admin scope must remain server-only",
                )
                .with_target(display)
                .with_detail("audience", json!(audience))
                .with_detail("scope", json!(scope)),
            );
        }

        let languages = object
            .get("languages")
            .and_then(JsonValue::as_array)
            .map(|values| values.iter().filter_map(JsonValue::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        if languages != CANONICAL_RPC_LANGUAGES {
            report.push(
                Finding::error(
                    "generated-rpc-languages-invalid",
                    "generated RPC manifest must contain exactly rust, go, dart, typescript, gleam in canonical order",
                )
                .with_target(display)
                .with_detail("languages", json!(languages)),
            );
        }

        let operation_index = object
            .get("source_operation_index")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        if operation_index != RPC_OPERATION_INDEX {
            report.push(
                Finding::error(
                    "generated-rpc-operation-index-invalid",
                    "generated RPC clients must pin the handlers-authoritative server operation index",
                )
                .with_target(display)
                .with_detail("sourceOperationIndex", json!(operation_index)),
            );
        }

        let route_map = object
            .get("source_route_map")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        if !valid_relative_evidence_path(route_map) {
            report.push(
                Finding::error(
                    "generated-rpc-route-map-path-invalid",
                    "generated RPC source_route_map must be a non-empty relative path without parent traversal",
                )
                .with_target(display)
                .with_detail("sourceRouteMap", json!(route_map)),
            );
        }

        for field in ["source_repository", "service"] {
            if object
                .get(field)
                .and_then(JsonValue::as_str)
                .is_none_or(|value| value.trim().is_empty())
            {
                report.push(
                    Finding::error(
                        "generated-rpc-identity-invalid",
                        format!("generated RPC {field} must be a non-empty string"),
                    )
                    .with_target(display)
                    .with_detail("field", json!(field)),
                );
            }
        }

        let operations = object
            .get("operations")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        if operations.iter().any(|value| {
            value
                .as_str()
                .is_none_or(|key| !valid_dotted_operation_key(key) || !seen.insert(key.to_owned()))
        }) {
            report.push(
                Finding::error(
                    "generated-rpc-operations-invalid",
                    "generated RPC operations must be unique stable dotted operation keys",
                )
                .with_target(display),
            );
        }
    }

    if commits.len() > 1 {
        report.push(
            Finding::error(
                "generated-rpc-source-commit-split",
                "regular/admin generated RPC clients must pin the same API-server commit when both scopes are present in one target repository",
            )
            .with_detail("sourceCommits", json!(commits)),
        );
    }
    report.insert_metadata("generatedRpcScopedManifestCount", json!(checked));
    report.insert_metadata("generatedRpcSourceCommits", json!(commits));
}

fn valid_relative_evidence_path(value: &str) -> bool {
    if value.trim().is_empty() {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && path.components().all(|component| {
            !matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
}

fn valid_dotted_operation_key(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() >= 2
        && parts.into_iter().all(|part| {
            let mut chars = part.chars();
            matches!(chars.next(), Some(ch) if ch.is_ascii_lowercase())
                && chars.all(|ch| {
                    ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-'
                })
        })
}

fn read_bounded_json(path: &Path, display: &str, report: &mut CommandReport) -> Option<JsonValue> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-rpc-evidence-unreadable",
                    format!("generated RPC evidence metadata could not be inspected: {error}"),
                )
                .with_target(display),
            );
            return None;
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_INPUT_BYTES {
        report.push(
            Finding::error(
                "generated-rpc-evidence-unsafe",
                "generated RPC evidence must be a bounded regular file",
            )
            .with_target(display),
        );
        return None;
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-rpc-evidence-unreadable",
                    format!("generated RPC evidence could not be read as UTF-8: {error}"),
                )
                .with_target(display),
            );
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-rpc-evidence-invalid-json",
                    format!("generated RPC evidence is not valid JSON: {error}"),
                )
                .with_target(display),
            );
            None
        }
    }
}

fn load_declared_env_keys(root: &Path, report: &mut CommandReport) -> Option<BTreeSet<String>> {
    let path = root.join(CLI_CONTRACT);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-runtime-cli-contract-unreadable",
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
                "generated-runtime-cli-contract-unsafe",
                ".cli-flags.toml must be a bounded regular file before generated runtime parity can be trusted",
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
                    "generated-runtime-cli-contract-unreadable",
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
                    "generated-runtime-cli-contract-invalid",
                    format!(".cli-flags.toml could not be parsed for generated runtime parity: {error}"),
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

fn parse_runtime_surface(
    root: &Path,
    surface: RuntimeSurface,
    report: &mut CommandReport,
) -> Option<BTreeSet<String>> {
    let path = root.join(surface.runtime_path);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-runtime-surface-unreadable",
                    format!("generated runtime surface metadata could not be inspected: {error}"),
                )
                .with_target(surface.runtime_path),
            );
            return None;
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_INPUT_BYTES {
        report.push(
            Finding::error(
                "generated-runtime-surface-unsafe",
                "generated runtime surface must be a bounded regular file",
            )
            .with_target(surface.runtime_path)
            .with_detail("language", json!(surface.language.label())),
        );
        return None;
    }

    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            report.push(
                Finding::error(
                    "generated-runtime-surface-unreadable",
                    format!("generated runtime surface could not be read as UTF-8: {error}"),
                )
                .with_target(surface.runtime_path),
            );
            return None;
        }
    };

    let mut parser = Parser::new();
    let grammar = surface.language.grammar();
    if let Err(error) = parser.set_language(&grammar) {
        report.push(
            Finding::error(
                "generated-runtime-parser-init-failed",
                format!("generated runtime parser could not initialize: {error}"),
            )
            .with_target(surface.runtime_path)
            .with_detail("language", json!(surface.language.label())),
        );
        return None;
    }
    let Some(tree) = parser.parse(source.as_bytes(), None) else {
        report.push(
            Finding::error(
                "generated-runtime-parser-returned-none",
                "generated runtime parser returned no syntax tree",
            )
            .with_target(surface.runtime_path)
            .with_detail("language", json!(surface.language.label())),
        );
        return None;
    };
    if tree.root_node().has_error() {
        report.push(
            Finding::error(
                "generated-runtime-surface-syntax-error",
                "generated runtime surface contains AST syntax errors and cannot satisfy runtime parity",
            )
            .with_target(surface.runtime_path)
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

fn likely_stale_alias(candidate: &str, expected: &str) -> bool {
    if candidate == expected {
        return false;
    }
    if candidate.starts_with(expected) || expected.starts_with(candidate) {
        return true;
    }

    let candidate_parts = candidate.split('_').collect::<Vec<_>>();
    let expected_parts = expected.split('_').collect::<Vec<_>>();
    let common = candidate_parts
        .iter()
        .zip(expected_parts.iter())
        .take_while(|(left, right)| left == right)
        .count();
    common >= 3
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
    use std::path::Path;

    use serde_json::json;
    use tempfile::tempdir;

    use super::audit_generated_runtime_parity;
    use crate::model::CommandReport;

    fn has(report: &CommandReport, code: &str) -> bool {
        report.findings.iter().any(|finding| finding.code == code)
    }

    fn write_contract(root: &Path, env: &str) {
        fs::write(
            root.join(".cli-flags.toml"),
            format!("[flags.api]\nenv = \"{env}\"\ntype = \"string\"\n"),
        )
        .expect("flags contract");
        fs::create_dir_all(root.join("generated/rust")).expect("generated dir");
        fs::write(
            root.join("generated/rust/env.rs"),
            format!("pub const API: &str = \"{env}\";\n"),
        )
        .expect("generated env");
    }

    fn write_rpc_manifest(root: &Path, scope: &str, audience: &str) {
        let dir = root.join("generated/rpc").join(scope);
        fs::create_dir_all(&dir).expect("rpc dir");
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "schema_version":2,
                "generated_by":"ores-stack sync",
                "source_role":"api-server-handlers.rs",
                "source_repository":"https://github.com/example/demo-api-server.rs.git",
                "source_commit_sha":"0123456789012345678901234567890123456789",
                "source_route_map":"generated/rpc/server-route-map.json",
                "source_route_map_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "source_operation_index":"generated/rpc/server-operation-index.json",
                "source_operation_index_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "service":"demo-api-server",
                "audience":audience,
                "scope":scope,
                "operations":["demo.users.find"],
                "languages":["rust","go","dart","typescript","gleam"],
                "http_endpoint":"/v1/rpc"
            }))
            .expect("manifest json"),
        )
        .expect("manifest");
    }

    #[test]
    fn runtime_surface_matches_contract_and_ignores_flags2env_control_keys() {
        let root = tempdir().expect("temporary repository");
        write_contract(root.path(), "API_URL");
        fs::write(
            root.path().join("generated/rust/runtime.rs"),
            "pub fn load() { let _ = \"API_URL\"; let _ = \"FLAGS2ENV_DOTENV\"; }\n",
        )
        .expect("runtime");

        let mut report = CommandReport::new("generated runtime parity test");
        audit_generated_runtime_parity(root.path(), &mut report);
        assert!(!has(&report, "generated-runtime-missing-keys"));
        assert!(!has(&report, "generated-runtime-stale-keys"));
    }

    #[test]
    fn partial_regeneration_reports_missing_and_stale_runtime_key() {
        let root = tempdir().expect("temporary repository");
        write_contract(root.path(), "FANWAAVE_API_BASE_URL");
        fs::write(
            root.path().join("generated/rust/runtime.rs"),
            "pub fn load() { let _ = \"FANWAAVE_API_BASE\"; }\n",
        )
        .expect("runtime");

        let mut report = CommandReport::new("generated runtime parity test");
        audit_generated_runtime_parity(root.path(), &mut report);
        assert!(has(&report, "generated-runtime-missing-keys"));
        assert!(has(&report, "generated-runtime-stale-keys"));
    }

    #[test]
    fn env_surface_without_runtime_surface_fails_closed() {
        let root = tempdir().expect("temporary repository");
        write_contract(root.path(), "API_URL");

        let mut report = CommandReport::new("generated runtime parity test");
        audit_generated_runtime_parity(root.path(), &mut report);
        assert!(has(&report, "generated-runtime-surface-missing"));
    }

    #[test]
    fn malformed_runtime_surface_fails_closed() {
        let root = tempdir().expect("temporary repository");
        write_contract(root.path(), "API_URL");
        fs::write(
            root.path().join("generated/rust/runtime.rs"),
            "pub fn load( { let _ = \"API_URL\"; }\n",
        )
        .expect("runtime");

        let mut report = CommandReport::new("generated runtime parity test");
        audit_generated_runtime_parity(root.path(), &mut report);
        assert!(has(&report, "generated-runtime-surface-syntax-error"));
    }

    #[test]
    fn scoped_handlers_authoritative_rpc_manifest_is_accepted() {
        let root = tempdir().expect("temporary repository");
        write_rpc_manifest(root.path(), "regular", "server");

        let mut report = CommandReport::new("generated RPC parity test");
        audit_generated_runtime_parity(root.path(), &mut report);
        assert!(!has(&report, "generated-rpc-manifest-version-invalid"));
        assert!(!has(&report, "generated-rpc-source-role-invalid"));
        assert!(!has(&report, "generated-rpc-languages-invalid"));
        assert!(!has(&report, "generated-rpc-operation-index-invalid"));
    }

    #[test]
    fn admin_rpc_manifest_cannot_be_public() {
        let root = tempdir().expect("temporary repository");
        write_rpc_manifest(root.path(), "admin", "public");

        let mut report = CommandReport::new("generated RPC parity test");
        audit_generated_runtime_parity(root.path(), &mut report);
        assert!(has(&report, "generated-rpc-audience-invalid"));
    }

    #[test]
    fn noncanonical_language_set_is_rejected() {
        let root = tempdir().expect("temporary repository");
        write_rpc_manifest(root.path(), "regular", "server");
        let path = root.path().join("generated/rpc/regular/manifest.json");
        let mut value: JsonValue = serde_json::from_slice(&fs::read(&path).expect("read manifest"))
            .expect("parse manifest");
        value["languages"] = json!(["rust", "typescript", "dart"]);
        fs::write(&path, serde_json::to_vec_pretty(&value).expect("json")).expect("write manifest");

        let mut report = CommandReport::new("generated RPC parity test");
        audit_generated_runtime_parity(root.path(), &mut report);
        assert!(has(&report, "generated-rpc-languages-invalid"));
    }

    #[test]
    fn stale_unscoped_rpc_manifest_is_rejected() {
        let root = tempdir().expect("temporary repository");
        fs::create_dir_all(root.path().join("generated/rpc")).expect("rpc dir");
        fs::write(root.path().join(LEGACY_RPC_MANIFEST), "{}\n").expect("legacy manifest");

        let mut report = CommandReport::new("generated RPC parity test");
        audit_generated_runtime_parity(root.path(), &mut report);
        assert!(has(&report, "generated-rpc-manifest-layout-stale"));
    }

    #[test]
    fn traversal_in_route_map_provenance_is_rejected() {
        let root = tempdir().expect("temporary repository");
        write_rpc_manifest(root.path(), "regular", "server");
        let path = root.path().join("generated/rpc/regular/manifest.json");
        let mut value: JsonValue = serde_json::from_slice(&fs::read(&path).expect("read manifest"))
            .expect("parse manifest");
        value["source_route_map"] = json!("../../other.json");
        fs::write(&path, serde_json::to_vec_pretty(&value).expect("json")).expect("write manifest");

        let mut report = CommandReport::new("generated RPC parity test");
        audit_generated_runtime_parity(root.path(), &mut report);
        assert!(has(&report, "generated-rpc-route-map-path-invalid"));
    }
}
