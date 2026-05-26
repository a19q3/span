use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is before Unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{name}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn extracts_containing_rust_function() {
    let dir = temp_dir("span-rust");
    let file = dir.join("sample.rs");
    fs::write(
        &file,
        "fn alpha() {\n    let value = 42;\n    println!(\"{value}\");\n}\n\nfn beta() {}\n",
    )
    .expect("write sample");

    let target = format!("{}:2", file.display());
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .arg(target)
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("range: 1..4"), "{stdout}");
    assert!(stdout.contains("kind: function"), "{stdout}");
    assert!(stdout.contains("symbol: alpha"), "{stdout}");
    assert!(stdout.contains(">    2 |     let value = 42;"), "{stdout}");

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn json_output_contains_target_text() {
    let dir = temp_dir("span-json");
    let file = dir.join("sample.py");
    fs::write(&file, "def alpha():\n    return 42\n").expect("write sample");

    let target = format!("{}:2", file.display());
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["--json", &target])
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"tool\":\"span\""), "{stdout}");
    assert!(stdout.contains("\"symbol\":\"alpha\""), "{stdout}");
    assert!(stdout.contains("return 42"), "{stdout}");

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn contains_finds_first_matching_span() {
    let dir = temp_dir("span-contains");
    let file = dir.join("sample.rs");
    fs::write(
        &file,
        "fn alpha() {\n    println!(\"needle\");\n}\n\nfn beta() {}\n",
    )
    .expect("write sample");

    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["--contains", "needle", dir.to_str().expect("utf8 path")])
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("range: 1..3"), "{stdout}");
    assert!(stdout.contains("symbol: alpha"), "{stdout}");
    assert!(
        stdout.contains(">    2 |     println!(\"needle\");"),
        "{stdout}"
    );

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn symbol_finds_named_span() {
    let dir = temp_dir("span-symbol");
    let file = dir.join("sample.rs");
    fs::write(
        &file,
        "fn alpha() {}\n\npub fn beta() {\n    println!(\"selected\");\n}\n",
    )
    .expect("write sample");

    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["--symbol", "beta", dir.to_str().expect("utf8 path")])
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("range: 3..5"), "{stdout}");
    assert!(stdout.contains("symbol: beta"), "{stdout}");
    assert!(stdout.contains(">    3 | pub fn beta() {"), "{stdout}");

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn kind_filter_accepts_matching_span_kind() {
    let dir = temp_dir("span-kind");
    let file = dir.join("sample.rs");
    fs::write(&file, "fn alpha() {\n    println!(\"selected\");\n}\n").expect("write sample");

    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args([
            "--kind",
            "function",
            "--contains",
            "selected",
            dir.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kind: function"), "{stdout}");
    assert!(stdout.contains("symbol: alpha"), "{stdout}");

    fs::remove_dir_all(dir).expect("remove temp dir");
}
