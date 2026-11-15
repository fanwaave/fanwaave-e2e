#![forbid(unsafe_code)]
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

fn root(env: &str, fallback: &str) -> PathBuf {
    let path = std::env::var(env).unwrap_or_else(|_| fallback.to_owned());
    PathBuf::from(path)
}
fn read_json(root: &PathBuf, relative: &str) -> Value {
    let content = fs::read_to_string(root.join(relative)).expect("read exact candidate file");
    serde_json::from_str(&content).expect("parse authoritative JSON")
}
fn candidates() -> (Value, Value, Value, Value) {
    let clients = root("FANWAAVE_CLIENTS_CHECKOUT", "../external/fanwaave-clients");
    let interfaces = root(
        "FANWAAVE_INTERFACES_CHECKOUT",
        "../external/fanwaave-interfaces",
    );
    (
        read_json(&clients, "clients/atto/contract-binding.v1.json"),
        read_json(&clients, "rpc/public-projection.v1.json"),
        read_json(&interfaces, "atto/notification-dispatch.v1.json"),
        read_json(&interfaces, "schema/v1/notificationdispatch.json"),
    )
}
fn validate(client: &Value, rpc: &Value, atto: &Value, schema: &Value) -> Result<(), &'static str> {
    if client["schema"] != "fanwaave.atto-client-binding/v1"
        || client["language"] != "atto"
        || client["admission"] != "contract-only"
        || client["contractAuthority"] != "fanwaave/fanwaave-interfaces"
        || client["operation"] != "fanwaave.health.get"
        || client["scope"] != "regular"
        || client["audience"] != "browser"
        || client["transportImplemented"] != false
    {
        return Err("client authority mismatch");
    }
    if client["security"]["httpsOnly"] != true
        || client["security"]["bearerHeader"] != true
        || client["security"]["boundedResponse"] != true
    {
        return Err("client security mismatch");
    }
    if rpc["schema"] != "fanwaave.rpc-public-projection/v1"
        || rpc["contractAuthority"] != client["contractAuthority"]
        || rpc["audience"] != "public"
        || rpc["scope"] != "regular"
        || rpc["rules"]["allowAdminScope"] != false
        || rpc["rules"]["allowAudienceWidening"] != false
        || rpc["rules"]["allowHandAuthoredGeneratedSources"] != false
    {
        return Err("RPC authority mismatch");
    }
    let operations = rpc["operations"].as_array().ok_or("missing operations")?;
    if !operations.iter().any(|op| {
        op["key"] == client["operation"]
            && op["requiredAudience"] == client["audience"]
            && op["requiredScope"] == client["scope"]
    }) {
        return Err("unadmitted public operation");
    }
    if atto["schema"] != "fanwaave.atto-contract-projection/v1"
        || atto["language"] != client["language"]
        || atto["admission"] != client["admission"]
        || atto["generated"] != false
        || atto["authoritativeSchema"] != "schema/v1/notificationdispatch.json"
        || atto["model"] != schema["title"]
        || schema["additionalProperties"] != false
        || atto["additionalProperties"] != false
    {
        return Err("interface authority mismatch");
    }
    let fields = atto["fields"].as_object().ok_or("invalid fields")?;
    let props = schema["properties"].as_object().ok_or("invalid schema")?;
    let needed = ["id", "revision", "payload"];
    if fields.len() != needed.len() {
        return Err("unexpected interface field");
    }
    let required = atto["required"].as_array().ok_or("missing required")?;
    let authored = schema["required"]
        .as_array()
        .ok_or("missing schema required")?;
    if required.len() != needed.len() || authored.len() != needed.len() {
        return Err("incomplete required fields");
    }
    for name in needed {
        if !required.iter().any(|v| v == name) || !authored.iter().any(|v| v == name) {
            return Err("required field mismatch");
        }
        if fields.get(name) != props.get(name) {
            return Err("contract drift");
        }
    }
    Ok(())
}
#[test]
fn atto_cross_repo_contracts_are_consistent() {
    let (client, rpc, atto, schema) = candidates();
    assert_eq!(validate(&client, &rpc, &atto, &schema), Ok(()));
}
#[test]
fn rejects_admin_scope_and_operation_widening() {
    let (mut client, rpc, atto, schema) = candidates();
    client["scope"] = json!("admin");
    assert!(validate(&client, &rpc, &atto, &schema).is_err());
}
#[test]
fn rejects_missing_security_flag() {
    let (mut client, rpc, atto, schema) = candidates();
    client["security"]["httpsOnly"] = json!(false);
    assert!(validate(&client, &rpc, &atto, &schema).is_err());
}
#[test]
fn rejects_forged_notification_revision() {
    let (client, rpc, mut atto, schema) = candidates();
    atto["fields"]["revision"]["const"] = json!("forged");
    assert!(validate(&client, &rpc, &atto, &schema).is_err());
}
#[test]
fn rejects_generated_claim_and_unadmitted_operation() {
    let (client, mut rpc, mut atto, schema) = candidates();
    atto["generated"] = json!(true);
    assert!(validate(&client, &rpc, &atto, &schema).is_err());
    atto["generated"] = json!(false);
    rpc["operations"] = json!([]);
    assert!(validate(&client, &rpc, &atto, &schema).is_err());
}
