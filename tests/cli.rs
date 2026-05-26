use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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
    assert!(stdout.contains("\"backend\":\"heuristic\""), "{stdout}");
    assert!(
        stdout.contains("\"backend_reason\":\"heuristic backend selected explicitly\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"fallback_used\":false"), "{stdout}");
    assert!(stdout.contains("\"truncated\":false"), "{stdout}");
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
fn symbol_name_does_not_remove_keyword_substrings() {
    let dir = temp_dir("span-symbol-substring");
    let file = dir.join("sample.rs");
    fs::write(
        &file,
        "fn classify_symbol() {\n    println!(\"selected\");\n}\n",
    )
    .expect("write sample");

    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args([
            "--symbol",
            "classify_symbol",
            dir.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("symbol: classify_symbol"), "{stdout}");
    assert!(
        stdout.contains(">    1 | fn classify_symbol() {"),
        "{stdout}"
    );

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn symbol_finds_rust_structs_enums_and_traits() {
    let dir = temp_dir("span-rust-items");
    let file = dir.join("sample.rs");
    fs::write(
        &file,
        "pub(crate) struct FileInfo {\n    len: u64,\n}\n\nenum Mode {\n    Fast,\n}\n\npub trait Runnable {\n    fn run(&self);\n}\n",
    )
    .expect("write sample");

    for (symbol, kind) in [
        ("FileInfo", "struct"),
        ("Mode", "enum"),
        ("Runnable", "trait"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_span"))
            .args(["--symbol", symbol, dir.to_str().expect("utf8 path")])
            .output()
            .expect("run span");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(&format!("kind: {kind}")), "{stdout}");
        assert!(stdout.contains(&format!("symbol: {symbol}")), "{stdout}");
    }

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn symbol_finds_rust_impl_blocks_by_implemented_type() {
    let dir = temp_dir("span-rust-impls");
    let file = dir.join("sample.rs");
    fs::write(
        &file,
        "struct Store<T> {\n    value: T,\n}\n\ntrait Runnable {}\n\nimpl<T: Clone> Store<T> {\n    fn get(&self) {}\n}\n\nimpl Runnable for crate::workers::Worker {\n    fn run(&self) {}\n}\n",
    )
    .expect("write sample");

    for symbol in ["Store", "Worker"] {
        let output = Command::new(env!("CARGO_BIN_EXE_span"))
            .args([
                "--kind",
                "impl",
                "--symbol",
                symbol,
                dir.to_str().expect("utf8 path"),
            ])
            .output()
            .expect("run span");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("kind: impl"), "{stdout}");
        assert!(stdout.contains(&format!("symbol: {symbol}")), "{stdout}");
    }

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

#[test]
fn kind_filter_continues_past_non_matching_candidates() {
    let dir = temp_dir("span-kind-search");
    fs::write(dir.join("a.txt"), "selected outside code\n").expect("write text sample");
    fs::write(
        dir.join("b.rs"),
        "fn beta() {\n    println!(\"selected\");\n}\n",
    )
    .expect("write rust sample");

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
    assert!(stdout.contains("symbol: beta"), "{stdout}");

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn markdown_fence_contains_inner_line() {
    let dir = temp_dir("span-markdown-fence");
    let file = dir.join("sample.md");
    fs::write(
        &file,
        "# Notes\n\n```sh\necho selected\n```\n\nplain text\n",
    )
    .expect("write markdown sample");

    let target = format!("{}:4", file.display());
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .arg(target)
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("range: 3..5"), "{stdout}");
    assert!(stdout.contains("kind: markdown-fence"), "{stdout}");
    assert!(stdout.contains(">    4 | echo selected"), "{stdout}");

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn markdown_text_between_fences_is_not_reported_as_fence() {
    let dir = temp_dir("span-markdown-between-fences");
    let file = dir.join("sample.md");
    fs::write(
        &file,
        "```sh\necho first\n```\n\nplain text between fences\n\n```sh\necho second\n```\n",
    )
    .expect("write markdown sample");

    let target = format!("{}:5", file.display());
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .arg(target)
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kind: context"), "{stdout}");
    assert!(
        !stdout.contains("kind: markdown-fence"),
        "plain text was incorrectly classified as a fence:\n{stdout}"
    );
    assert!(
        stdout.contains(">    5 | plain text between fences"),
        "{stdout}"
    );

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn auto_backend_falls_back_to_heuristic_when_external_tools_are_missing() {
    let dir = temp_dir("span-backend-auto");
    let file = dir.join("sample.rs");
    fs::write(&file, "fn alpha() {\n    println!(\"selected\");\n}\n").expect("write sample");

    let target = format!("{}:2", file.display());
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["--backend", "auto", &target])
        .env("PATH", "")
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kind: function"), "{stdout}");
    assert!(stdout.contains("symbol: alpha"), "{stdout}");
    assert!(
        stdout.contains(">    2 |     println!(\"selected\");"),
        "{stdout}"
    );

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn max_lines_rejects_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["--max-lines", "0", "src/main.rs:1"])
        .output()
        .expect("run span");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--max-lines requires a positive integer"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn backend_doctor_json_reports_available_and_missing_backends() {
    let dir = temp_dir("span-backend-doctor");
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin dir");
    let fake_backend = bin_dir.join("ast-outline");
    fs::write(
        &fake_backend,
        "#!/bin/sh\nif [ \"$1\" = \"--help\" ]; then printf 'fake help\\n'; exit 0; fi\nexit 0\n",
    )
    .expect("write fake backend");
    let mut permissions = fs::metadata(&fake_backend)
        .expect("fake backend metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_backend, permissions).expect("chmod fake backend");

    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["backend", "doctor", "--json"])
        .env("PATH", bin_dir.to_str().expect("utf8 bin dir"))
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"default_backend\":\"heuristic\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"auto_order\":[\"ast-outline\",\"ast-bro\",\"heuristic\"]"),
        "{stdout}"
    );
    assert!(stdout.contains("\"name\":\"ast-outline\""), "{stdout}");
    assert!(stdout.contains("\"available\":true"), "{stdout}");
    assert!(stdout.contains("\"help_status\":\"ok\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"ast-bro\""), "{stdout}");
    assert!(stdout.contains("\"available\":false"), "{stdout}");

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn explicit_missing_backend_returns_clear_error() {
    let dir = temp_dir("span-backend-missing");
    let file = dir.join("sample.rs");
    fs::write(&file, "fn alpha() {\n    println!(\"selected\");\n}\n").expect("write sample");

    let target = format!("{}:2", file.display());
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["--backend", "ast-outline", &target])
        .env("PATH", "")
        .output()
        .expect("run span");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("backend ast-outline is not available in PATH"),
        "{stderr}"
    );

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn explicit_backend_requires_concrete_symbol() {
    let dir = temp_dir("span-backend-context");
    let file = dir.join("notes.txt");
    fs::write(&file, "plain text without a symbol\n").expect("write sample");

    let target = format!("{}:1", file.display());
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["--backend", "ast-outline", &target])
        .output()
        .expect("run span");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("backend ast-outline requires a concrete symbol"),
        "{stderr}"
    );

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[cfg(unix)]
#[test]
fn auto_explain_reports_selected_external_backend() {
    let dir = temp_dir("span-backend-explain");
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin dir");
    let fake_backend = bin_dir.join("ast-outline");
    fs::write(
        &fake_backend,
        "#!/bin/sh\nif [ \"$1\" = \"--help\" ]; then printf 'fake help\\n'; exit 0; fi\nprintf 'external body for %s\\n' \"$3\"\n",
    )
    .expect("write fake backend");
    let mut permissions = fs::metadata(&fake_backend)
        .expect("fake backend metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_backend, permissions).expect("chmod fake backend");

    let file = dir.join("sample.rs");
    fs::write(&file, "fn alpha() {\n    println!(\"selected\");\n}\n").expect("write sample");

    let target = format!("{}:2", file.display());
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["--backend", "auto", "--explain", &target])
        .env("PATH", bin_dir.to_str().expect("utf8 bin dir"))
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("backend: ast-outline"), "{stdout}");
    assert!(stdout.contains("external body for alpha"), "{stdout}");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("backend: ast-outline"), "{stderr}");
    assert!(
        stderr.contains("backend reason: auto selected ast-outline"),
        "{stderr}"
    );
    assert!(stderr.contains("fallback used: no"), "{stderr}");

    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[cfg(unix)]
#[test]
fn explicit_ast_outline_backend_delegates_known_symbol_body() {
    let dir = temp_dir("span-backend-ast-outline");
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin dir");
    let fake_backend = bin_dir.join("ast-outline");
    fs::write(
        &fake_backend,
        "#!/bin/sh\nprintf 'external backend called: %s %s %s\\nline two\\nline three\\nline four\\nline five\\n' \"$1\" \"$2\" \"$3\"\n",
    )
    .expect("write fake backend");
    let mut permissions = fs::metadata(&fake_backend)
        .expect("fake backend metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_backend, permissions).expect("chmod fake backend");

    let file = dir.join("sample.rs");
    fs::write(&file, "fn alpha() {\n    println!(\"selected\");\n}\n").expect("write sample");

    let target = format!("{}:2", file.display());
    let output = Command::new(env!("CARGO_BIN_EXE_span"))
        .args(["--backend", "ast-outline", "--max-lines", "3", &target])
        .env("PATH", bin_dir.to_str().expect("utf8 bin dir"))
        .output()
        .expect("run span");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("backend: ast-outline"), "{stdout}");
    assert!(stdout.contains("external backend called: show"), "{stdout}");
    assert!(stdout.contains("alpha"), "{stdout}");
    assert!(
        stdout.contains("--- truncated by span --max-lines ---"),
        "{stdout}"
    );
    assert!(!stdout.contains("line five"), "{stdout}");

    fs::remove_dir_all(dir).expect("remove temp dir");
}
