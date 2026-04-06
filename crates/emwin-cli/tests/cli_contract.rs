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

#[test]
fn invalid_archive_boolean_filter_keeps_stdout_clean() {
    let output = command()
        .args([
            "query",
            "--database-url",
            "postgres://localhost/emwin",
            "products",
            "--has-issues",
            "maybe",
        ])
        .output()
        .expect("command should run");

    assert!(
        !output.status.success(),
        "invalid boolean filter should fail"
    );
    assert!(output.stdout.is_empty(), "stdout should stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("has_issues must be one of"),
        "stderr should contain invalid-argument output: {stderr}"
    );
}

#[test]
fn invalid_archive_product_size_range_keeps_stdout_clean_without_database_connection() {
    let output = command()
        .args([
            "query",
            "--database-url",
            "postgres://127.0.0.1:1/emwin",
            "products",
            "--min-size",
            "10",
            "--max-size",
            "1",
        ])
        .output()
        .expect("command should run");

    assert!(!output.status.success(), "invalid size range should fail");
    assert!(output.stdout.is_empty(), "stdout should stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("min_size must be less than or equal to max_size"),
        "stderr should contain invalid-argument output: {stderr}"
    );
    assert!(
        !stderr.contains("PoolTimedOut"),
        "validation should fail before any database connection attempt: {stderr}"
    );
}

#[test]
fn invalid_archive_feature_size_range_keeps_stdout_clean_without_database_connection() {
    let output = command()
        .args([
            "query",
            "--database-url",
            "postgres://127.0.0.1:1/emwin",
            "features",
            "--min-size",
            "10",
            "--max-size",
            "1",
        ])
        .output()
        .expect("command should run");

    assert!(!output.status.success(), "invalid size range should fail");
    assert!(output.stdout.is_empty(), "stdout should stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("min_size must be less than or equal to max_size"),
        "stderr should contain invalid-argument output: {stderr}"
    );
    assert!(
        !stderr.contains("PoolTimedOut"),
        "validation should fail before any database connection attempt: {stderr}"
    );
}

#[test]
fn invalid_archive_aggregate_size_range_keeps_stdout_clean_without_database_connection() {
    let output = command()
        .args([
            "query",
            "--database-url",
            "postgres://127.0.0.1:1/emwin",
            "aggregate-facets",
            "office",
            "--min-size",
            "10",
            "--max-size",
            "1",
        ])
        .output()
        .expect("command should run");

    assert!(!output.status.success(), "invalid size range should fail");
    assert!(output.stdout.is_empty(), "stdout should stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("min_size must be less than or equal to max_size"),
        "stderr should contain invalid-argument output: {stderr}"
    );
    assert!(
        !stderr.contains("PoolTimedOut"),
        "validation should fail before any database connection attempt: {stderr}"
    );
}

#[test]
fn invalid_feature_kind_keeps_stdout_clean_without_database_connection() {
    let output = command()
        .args([
            "query",
            "--database-url",
            "postgres://127.0.0.1:1/emwin",
            "features",
            "--kind",
            "bogus",
        ])
        .output()
        .expect("command should run");

    assert!(!output.status.success(), "invalid feature kind should fail");
    assert!(output.stdout.is_empty(), "stdout should stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid feature kind"),
        "stderr should contain invalid-argument output: {stderr}"
    );
    assert!(
        !stderr.contains("PoolTimedOut"),
        "validation should fail before any database connection attempt: {stderr}"
    );
}

#[test]
fn invalid_timeseries_measure_keeps_stdout_clean_without_database_connection() {
    let output = command()
        .args([
            "query",
            "--database-url",
            "postgres://127.0.0.1:1/emwin",
            "aggregate-timeseries",
            "bogus",
            "--start",
            "2025-03-05T12:00:00Z",
            "--end",
            "2025-03-05T13:00:00Z",
            "--bucket",
            "hour",
        ])
        .output()
        .expect("command should run");

    assert!(
        !output.status.success(),
        "invalid timeseries measure should fail"
    );
    assert!(output.stdout.is_empty(), "stdout should stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid timeseries measure"),
        "stderr should contain invalid-argument output: {stderr}"
    );
    assert!(
        !stderr.contains("PoolTimedOut"),
        "validation should fail before any database connection attempt: {stderr}"
    );
}
