//! Archive query command implementations.

use crate::cmd::query_output::{
    ArchiveIssueResponse, ArchiveIssuesResponse, ArchiveProductResponse, IncidentProductsResponse,
    IncidentResponse, IncidentsResponse, write_json, write_raw_bytes,
};
use crate::error::{CliError, CliResult};
use chrono::{DateTime, Utc};
use clap::{ArgGroup, Args, Subcommand};
use emwin_db::{
    ArchivedIssueListQuery, IncidentKey, IncidentListQuery, IncidentProductsQuery, PostgresConfig,
    PostgresMetadataSink,
};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

/// Query archived incident and product data directly from Postgres-backed persistence.
#[derive(Debug, Args)]
pub(crate) struct QueryOptions {
    /// Postgres database URL for archived metadata queries.
    #[arg(long, env = "EMWIN_DATABASE_URL")]
    pub(crate) database_url: String,
    #[command(subcommand)]
    pub(crate) command: QueryCommand,
}

/// Query commands for archived data.
#[derive(Debug, Subcommand)]
pub(crate) enum QueryCommand {
    /// List live incident projection rows from persisted archive metadata.
    Incidents(IncidentsArgs),
    /// Fetch one incident by office, phenomena, significance, and ETN.
    Incident(IncidentArgs),
    /// List archived products linked to one incident.
    IncidentProducts(IncidentIdentityArgs),
    /// Fetch one archived product detail record.
    Product(ProductArgs),
    /// List archived issue rows.
    Issues(IssuesArgs),
    /// Fetch one archived issue row.
    Issue(IssueArgs),
    /// Read raw archived payload bytes.
    ProductRaw(ProductRawArgs),
}

#[derive(Debug, Args)]
pub(crate) struct IncidentsArgs {
    #[arg(long)]
    office: Option<String>,
    #[arg(long)]
    phenomena: Option<String>,
    #[arg(long)]
    significance: Option<String>,
    #[arg(long)]
    etn: Option<i64>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long, value_parser = parse_rfc3339_utc)]
    updated_after: Option<DateTime<Utc>>,
    #[arg(long, value_parser = parse_rfc3339_utc)]
    updated_before: Option<DateTime<Utc>>,
    #[arg(long, value_parser = parse_rfc3339_utc)]
    active_at: Option<DateTime<Utc>>,
    #[arg(long, default_value_t = 100)]
    limit: usize,
    #[arg(long)]
    cursor: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct IncidentArgs {
    #[command(flatten)]
    identity: IncidentLocatorArgs,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct IncidentLocatorArgs {
    office: String,
    phenomena: String,
    significance: String,
    etn: i64,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct IncidentIdentityArgs {
    #[command(flatten)]
    locator: IncidentLocatorArgs,
    #[arg(long, default_value_t = 100)]
    limit: usize,
    #[arg(long)]
    cursor: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct ProductArgs {
    product_id: i64,
}

#[derive(Debug, Args)]
pub(crate) struct IssuesArgs {
    #[arg(long)]
    product_id: Option<i64>,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    code: Option<String>,
    #[arg(long, default_value_t = 100)]
    limit: usize,
    #[arg(long)]
    cursor: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct IssueArgs {
    issue_id: i64,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("sink")
        .required(true)
        .multiple(false)
        .args(["output", "stdout"])
))]
pub(crate) struct ProductRawArgs {
    product_id: i64,
    /// Write raw payload bytes to a file path.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Write raw payload bytes directly to stdout.
    #[arg(long, default_value_t = false)]
    stdout: bool,
}

pub(crate) async fn run(options: QueryOptions) -> CliResult<()> {
    let database_url = options.database_url.trim();
    if database_url.is_empty() {
        return Err(CliError::invalid_argument(
            "--database-url must not be empty",
        ));
    }

    let sink = PostgresMetadataSink::connect(PostgresConfig::new(database_url.to_string())).await?;

    match options.command {
        QueryCommand::Incidents(args) => run_incidents(&sink, args).await,
        QueryCommand::Incident(args) => run_incident(&sink, args.identity).await,
        QueryCommand::IncidentProducts(args) => run_incident_products(&sink, args).await,
        QueryCommand::Product(args) => run_product(&sink, args.product_id).await,
        QueryCommand::Issues(args) => run_issues(&sink, args).await,
        QueryCommand::Issue(args) => run_issue(&sink, args.issue_id).await,
        QueryCommand::ProductRaw(args) => run_product_raw(&sink, args).await,
    }
}

async fn run_incidents(sink: &PostgresMetadataSink, args: IncidentsArgs) -> CliResult<()> {
    let page = sink
        .list_incidents(IncidentListQuery {
            office: args.office.map(|value| normalize_upper(&value)),
            phenomena: args.phenomena.map(|value| normalize_upper(&value)),
            significance: args.significance.map(|value| normalize_upper(&value)),
            etn: args.etn,
            status: args.status.map(|value| normalize_lower(&value)),
            updated_after: args.updated_after,
            updated_before: args.updated_before,
            active_at: args.active_at,
            limit: args.limit,
            cursor: args.cursor,
        })
        .await?;

    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, &IncidentsResponse::from_page(page))
}

async fn run_incident(sink: &PostgresMetadataSink, args: IncidentLocatorArgs) -> CliResult<()> {
    let key = incident_key(&args);
    let incident = sink
        .get_incident(&key)
        .await?
        .ok_or_else(|| CliError::runtime("incident not found"))?;

    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, &IncidentResponse::from_incident(incident))
}

async fn run_incident_products(
    sink: &PostgresMetadataSink,
    args: IncidentIdentityArgs,
) -> CliResult<()> {
    let key = incident_key(&args.locator);
    let page = sink
        .list_incident_products(
            &key,
            IncidentProductsQuery {
                limit: args.limit,
                cursor: args.cursor,
            },
        )
        .await?;

    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, &IncidentProductsResponse::from_page(page))
}

async fn run_product(sink: &PostgresMetadataSink, product_id: i64) -> CliResult<()> {
    let product = sink
        .get_archived_product(product_id)
        .await?
        .ok_or_else(|| CliError::runtime("archived product not found"))?;

    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, &ArchiveProductResponse::from_product(product))
}

async fn run_issues(sink: &PostgresMetadataSink, args: IssuesArgs) -> CliResult<()> {
    let page = sink
        .list_archived_issues(ArchivedIssueListQuery {
            product_id: args.product_id,
            kind: args.kind.map(|value| normalize_lower(&value)),
            code: args.code.map(|value| normalize_lower(&value)),
            limit: args.limit,
            cursor: args.cursor,
        })
        .await?;

    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, &ArchiveIssuesResponse::from_page(page))
}

async fn run_issue(sink: &PostgresMetadataSink, issue_id: i64) -> CliResult<()> {
    let issue = sink
        .get_archived_issue(issue_id)
        .await?
        .ok_or_else(|| CliError::runtime("archived issue not found"))?;

    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, &ArchiveIssueResponse::from_issue(issue))
}

async fn run_product_raw(sink: &PostgresMetadataSink, args: ProductRawArgs) -> CliResult<()> {
    let payload = sink
        .read_archived_payload(args.product_id)
        .await?
        .ok_or_else(|| CliError::runtime("archived payload not found"))?;

    if let Some(path) = args.output {
        write_payload_to_path(&path, &payload.bytes)?;
        return Ok(());
    }

    let mut stdout = io::stdout().lock();
    write_raw_bytes(&mut stdout, &payload.bytes)
}

fn write_payload_to_path(path: &PathBuf, bytes: &[u8]) -> CliResult<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

fn incident_key(args: &IncidentLocatorArgs) -> IncidentKey {
    IncidentKey {
        office: normalize_upper(&args.office),
        phenomena: normalize_upper(&args.phenomena),
        significance: normalize_upper(&args.significance),
        etn: args.etn,
    }
}

fn parse_rfc3339_utc(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|err| format!("invalid RFC3339 timestamp `{value}`: {err}"))
}

fn normalize_upper(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn normalize_lower(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        IncidentLocatorArgs, incident_key, normalize_lower, normalize_upper, parse_rfc3339_utc,
    };
    use chrono::{TimeZone, Utc};

    #[test]
    fn parse_rfc3339_normalizes_to_utc() {
        let parsed =
            parse_rfc3339_utc("2025-03-05T08:30:00-05:00").expect("timestamp should parse");
        assert_eq!(parsed, Utc.with_ymd_and_hms(2025, 3, 5, 13, 30, 0).unwrap());
    }

    #[test]
    fn normalize_helpers_trim_and_adjust_case() {
        assert_eq!(normalize_upper(" koax "), "KOAX");
        assert_eq!(normalize_lower(" Active "), "active");
    }

    #[test]
    fn incident_key_normalizes_identity_parts() {
        let key = incident_key(&IncidentLocatorArgs {
            office: "koax".to_string(),
            phenomena: "ff".to_string(),
            significance: "w".to_string(),
            etn: 2001,
        });

        assert_eq!(key.office, "KOAX");
        assert_eq!(key.phenomena, "FF");
        assert_eq!(key.significance, "W");
        assert_eq!(key.etn, 2001);
    }
}
