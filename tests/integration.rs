//! Integration tests — require a working `ghci` installation.
//! Run with: cargo test -- --ignored

use std::process::Command;

fn ghcitty_bin() -> String {
    let path = env!("CARGO_BIN_EXE_ghcitty");
    path.to_string()
}

fn has_ghci() -> bool {
    Command::new("ghci")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
#[ignore]
fn test_eval_arithmetic() {
    if !has_ghci() {
        eprintln!("skipping: ghci not found");
        return;
    }
    let out = Command::new(ghcitty_bin())
        .args(["eval", "1 + 1"])
        .output()
        .expect("failed to run ghcitty");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("2"),
        "expected output to contain '2', got: {stdout}"
    );
}

#[test]
#[ignore]
fn test_eval_json_mode() {
    if !has_ghci() {
        eprintln!("skipping: ghci not found");
        return;
    }
    let out = Command::new(ghcitty_bin())
        .args(["--json", "eval", "map (+1) [1,2,3]"])
        .output()
        .expect("failed to run ghcitty");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let j: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("output should be valid JSON");
    assert_eq!(j["value"], "[2,3,4]");
    assert!(j["type"].as_str().unwrap().contains("Integer"));
}

#[test]
#[ignore]
fn test_eval_error_json() {
    if !has_ghci() {
        eprintln!("skipping: ghci not found");
        return;
    }
    let out = Command::new(ghcitty_bin())
        .args(["--json", "eval", "undefinedVar123"])
        .output()
        .expect("failed to run ghcitty");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let j: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("output should be valid JSON");
    assert!(
        !j["diagnostics"].as_array().unwrap().is_empty(),
        "expected diagnostics for undefined var"
    );
}
