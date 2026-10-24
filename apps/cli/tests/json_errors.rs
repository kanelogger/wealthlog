//! Parse failures still produce the documented JSON object on stdout.

use std::process::Command;

fn wealthlog() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wealthlog"))
}

#[test]
fn missing_arguments_return_json_and_exit_2() {
    let output = wealthlog().args(["ordinary", "create"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON: {error}: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "invalid_input");
    assert_eq!(value["requestKey"], serde_json::Value::Null);
    assert!(value["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("--input"));
}

#[test]
fn an_unknown_app_id_returns_json_before_touching_files() {
    let output = wealthlog()
        .args(["--app-id", "../escape", "service", "status"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "invalid_input");
}

#[test]
fn help_and_version_keep_their_normal_output() {
    let help = wealthlog().arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));

    let version = wealthlog().arg("--version").output().unwrap();
    assert_eq!(version.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("wealthlog "));
}
