use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_path(label: &str, extension: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("mbus-{label}-{nanos}.{extension}"))
}

fn run_bootstrap(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cdp_bootstrap"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run cdp_bootstrap")
}

#[test]
fn cdp_bootstrap_reports_launch_context_for_bad_executable_cli() {
    let output = run_bootstrap(&[
        "--browser-executable",
        "/definitely/missing/chrome",
        "--browser-launch-timeout-ms",
        "1234",
        "--browser-no-sandbox",
        "true",
        "--browser-arg=--alpha",
        "--browser-arg=--beta=1",
    ]);

    assert!(
        !output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("code=cdp_launch_failed"), "{stderr}");
    assert!(
        stderr.contains("executable=/definitely/missing/chrome"),
        "{stderr}"
    );
    assert!(stderr.contains("no_sandbox=true"), "{stderr}");
    assert!(stderr.contains("launch_timeout_ms=1234"), "{stderr}");
    assert!(stderr.contains("--alpha"), "{stderr}");
    assert!(stderr.contains("--beta=1"), "{stderr}");
}

#[test]
fn cdp_bootstrap_reads_browser_launch_config_from_file() {
    let config_path = unique_path("cdp-bootstrap-config", "toml");
    std::fs::write(
        &config_path,
        r#"
[browser]
executable_path = "/missing/from/config/chrome"
launch_timeout_ms = 2345
no_sandbox = true
extra_args = ["--gamma", "--delta=1"]
keep_user_data_dir = true
"#,
    )
    .expect("write config");

    let output = run_bootstrap(&["--config", &config_path.to_string_lossy()]);

    let _ = std::fs::remove_file(&config_path);

    assert!(
        !output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("code=cdp_launch_failed"), "{stderr}");
    assert!(
        stderr.contains("executable=/missing/from/config/chrome"),
        "{stderr}"
    );
    assert!(stderr.contains("launch_timeout_ms=2345"), "{stderr}");
    assert!(stderr.contains("keep_user_data_dir=true"), "{stderr}");
    assert!(stderr.contains("--gamma"), "{stderr}");
    assert!(stderr.contains("--delta=1"), "{stderr}");
}
