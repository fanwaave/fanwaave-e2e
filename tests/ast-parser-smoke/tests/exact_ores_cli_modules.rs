#![forbid(unsafe_code)]

#[path = "../src/model.rs"]
mod model;
#[path = "../src/generated_env_parity.rs"]
mod generated_env_parity;
#[path = "../src/generated_runtime_parity.rs"]
mod generated_runtime_parity;

#[test]
fn exact_ores_cli_modules_are_linked_into_this_test_binary() {
    let report = model::CommandReport::new("exact ores-cli module compile proof").finalize();
    assert_eq!(report.exit_code(), 0);
}
