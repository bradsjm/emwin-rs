use std::process::Command;

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_emwin-cli"))
}

#[test]
fn invalid_query_timestamp_writes_only_to_stderr() {
    let output = command()
        .args([
            "query",
            "--database-url",
            "postgres://localhost/emwin",
            "incidents",
            "--updated-after",
            "not-a-timestamp",
        ])
        .output()
        .expect("command should run");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "stdout should stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid RFC3339 timestamp"),
        "stderr should contain clap validation error: {stderr}"
    );
}

#[test]
fn product_raw_requires_exactly_one_sink_on_stderr() {
    let output = command()
        .args([
            "query",
            "--database-url",
            "postgres://localhost/emwin",
            "product-raw",
            "42",
        ])
        .output()
        .expect("command should run");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "stdout should stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required arguments were not provided")
            || stderr.contains("the following required arguments were not provided"),
        "stderr should contain clap missing-argument output: {stderr}"
    );
}

#[test]
fn invalid_issue_id_keeps_stdout_clean() {
    let output = command()
        .args([
            "query",
            "--database-url",
            "postgres://localhost/emwin",
            "issue",
            "not-a-number",
        ])
        .output()
        .expect("command should run");

    assert!(!output.status.success(), "issue parse should fail");
    assert!(output.stdout.is_empty(), "stdout should stay empty");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid value"),
        "stderr should contain clap parse error"
    );
}
