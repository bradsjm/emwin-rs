//! Archive query command implementations.

use crate::cmd::query_output::{
    ArchiveIssueResponse, ArchiveIssuesResponse, ArchiveProductResponse, CellAggregateResponse,
    FacetAggregateResponse, FeatureCollectionResponse, FeaturesResponse, IncidentProductsResponse,
    IncidentResponse, IncidentsResponse, ProductsResponse, TimeseriesAggregateResponse, write_json,
    write_raw_bytes,
};
use crate::error::{CliError, CliResult};
use chrono::{DateTime, Utc};
use clap::{ArgGroup, Args, Subcommand};
use emwin_db::{PostgresConfig, PostgresMetadataSink};
use emwin_service::{
    ArchiveFilterInput, ArchivedIssueListQuery, CellAggregateQuery, FacetAggregateQuery,
    FeatureListQuery, IncidentKey, IncidentListQuery, IncidentProductsQuery, ProductListQuery,
    TimeseriesAggregateQuery, build_cell_aggregate_query, build_facet_aggregate_query,
    build_feature_list_query, build_timeseries_aggregate_query,
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
    /// List archived product summaries.
    Products(ProductsArgs),
    /// Fetch one archived product detail record.
    Product(ProductArgs),
    /// List archived issue rows.
    Issues(IssuesArgs),
    /// Fetch one archived issue row.
    Issue(IssueArgs),
    /// Read raw archived payload bytes.
    ProductRaw(ProductRawArgs),
    /// List archived spatial features.
    Features(FeaturesArgs),
    /// Emit a GeoJSON FeatureCollection view of archived spatial features.
    FeaturesGeojson(FeaturesGeoJsonArgs),
    /// Aggregate archived products into facet buckets.
    AggregateFacets(FacetAggregateArgs),
    /// Aggregate archived products into time buckets.
    AggregateTimeseries(TimeseriesAggregateArgs),
    /// Aggregate archived products into geohash cells.
    AggregateCells(CellAggregateArgs),
}

impl QueryCommand {
    fn prepare(self) -> CliResult<PreparedQueryCommand> {
        match self {
            Self::Incidents(args) => Ok(PreparedQueryCommand::Incidents(args)),
            Self::Incident(args) => Ok(PreparedQueryCommand::Incident(args.identity)),
            Self::IncidentProducts(args) => Ok(PreparedQueryCommand::IncidentProducts(args)),
            Self::Products(args) => Ok(PreparedQueryCommand::Products(
                args.filters
                    .into_product_list_query(100, Some(args.limit), args.cursor)?,
            )),
            Self::Product(args) => Ok(PreparedQueryCommand::Product(args.product_id)),
            Self::Issues(args) => Ok(PreparedQueryCommand::Issues(args)),
            Self::Issue(args) => Ok(PreparedQueryCommand::Issue(args.issue_id)),
            Self::ProductRaw(args) => Ok(PreparedQueryCommand::ProductRaw(args)),
            Self::Features(args) => Ok(PreparedQueryCommand::Features(build_feature_list_query(
                args.filters.into(),
                args.kind,
                100,
                Some(args.limit),
                args.cursor,
            )?)),
            Self::FeaturesGeojson(args) => Ok(PreparedQueryCommand::FeaturesGeojson(
                build_feature_list_query(
                    args.filters.into(),
                    args.kind,
                    100,
                    Some(args.limit),
                    None,
                )?,
            )),
            Self::AggregateFacets(args) => Ok(PreparedQueryCommand::AggregateFacets(
                build_facet_aggregate_query(
                    args.filters.into(),
                    &args.dimension,
                    Some(args.limit),
                )?,
            )),
            Self::AggregateTimeseries(args) => Ok(PreparedQueryCommand::AggregateTimeseries(
                build_timeseries_aggregate_query(
                    args.filters.into(),
                    &args.measure,
                    args.start,
                    args.end,
                    &args.bucket,
                )?,
            )),
            Self::AggregateCells(args) => Ok(PreparedQueryCommand::AggregateCells(
                build_cell_aggregate_query(
                    args.filters.into(),
                    &args.measure,
                    args.precision,
                    Some(args.limit),
                )?,
            )),
        }
    }
}

enum PreparedQueryCommand {
    Incidents(IncidentsArgs),
    Incident(IncidentLocatorArgs),
    IncidentProducts(IncidentIdentityArgs),
    Products(ProductListQuery),
    Product(i64),
    Issues(IssuesArgs),
    Issue(i64),
    ProductRaw(ProductRawArgs),
    Features(FeatureListQuery),
    FeaturesGeojson(FeatureListQuery),
    AggregateFacets(FacetAggregateQuery),
    AggregateTimeseries(TimeseriesAggregateQuery),
    AggregateCells(CellAggregateQuery),
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

#[derive(Debug, Args, Clone)]
pub(crate) struct ProductsArgs {
    #[command(flatten)]
    filters: ArchiveFilterArgs,
    #[arg(long, default_value_t = 100)]
    limit: usize,
    #[arg(long)]
    cursor: Option<String>,
}

macro_rules! define_archive_filter_args {
    (@fields [$($fields:tt)*]) => {
        #[derive(Debug, Args, Clone, Default)]
        pub(crate) struct ArchiveFilterArgs {
            $($fields)*
        }
    };
    (@fields [$($fields:tt)*] $field:ident, string; $( $rest:tt )*) => {
        define_archive_filter_args!(
            @fields
            [$($fields)* #[arg(long)] $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, bool_string; $( $rest:tt )*) => {
        define_archive_filter_args!(
            @fields
            [$($fields)* #[arg(long)] $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, f64; $( $rest:tt )*) => {
        define_archive_filter_args!(
            @fields
            [$($fields)* #[arg(long)] $field: Option<f64>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, usize; $( $rest:tt )*) => {
        define_archive_filter_args!(
            @fields
            [$($fields)* #[arg(long)] $field: Option<usize>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, i64; $( $rest:tt )*) => {
        define_archive_filter_args!(
            @fields
            [$($fields)* #[arg(long)] $field: Option<i64>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, datetime_utc; $( $rest:tt )*) => {
        define_archive_filter_args!(
            @fields
            [$($fields)* #[arg(long, value_parser = parse_rfc3339_utc)] $field: Option<DateTime<Utc>>,]
            $( $rest )*
        );
    };
    ($( $rows:tt )*) => {
        define_archive_filter_args!(@fields [] $( $rows )*);
    };
}

macro_rules! build_archive_filter_input_from_args {
    ($value:ident; $( $field:ident, $kind:ident; )*) => {
        ArchiveFilterInput {
            $($field: $value.$field,)*
        }
    };
}

emwin_service::emwin_archive_filter_fields!(define_archive_filter_args);

impl ArchiveFilterArgs {
    fn into_product_list_query(
        self,
        default_limit: usize,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> CliResult<ProductListQuery> {
        Ok(ArchiveFilterInput::from(self).into_product_list_query(default_limit, limit, cursor)?)
    }
}

impl From<ArchiveFilterArgs> for ArchiveFilterInput {
    fn from(value: ArchiveFilterArgs) -> Self {
        emwin_service::emwin_archive_filter_fields!(build_archive_filter_input_from_args, value)
    }
}

#[derive(Debug, Args, Clone)]
pub(crate) struct FeaturesArgs {
    #[command(flatten)]
    filters: ArchiveFilterArgs,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long, default_value_t = 100)]
    limit: usize,
    #[arg(long)]
    cursor: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct FeaturesGeoJsonArgs {
    #[command(flatten)]
    filters: ArchiveFilterArgs,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long, default_value_t = 100)]
    limit: usize,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct FacetAggregateArgs {
    #[command(flatten)]
    filters: ArchiveFilterArgs,
    dimension: String,
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct TimeseriesAggregateArgs {
    #[command(flatten)]
    filters: ArchiveFilterArgs,
    measure: String,
    #[arg(long, value_parser = parse_rfc3339_utc)]
    start: DateTime<Utc>,
    #[arg(long, value_parser = parse_rfc3339_utc)]
    end: DateTime<Utc>,
    #[arg(long)]
    bucket: String,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct CellAggregateArgs {
    #[command(flatten)]
    filters: ArchiveFilterArgs,
    measure: String,
    #[arg(long)]
    precision: u8,
    #[arg(long, default_value_t = 100)]
    limit: usize,
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
    let command = options.command.prepare()?;

    let sink = PostgresMetadataSink::connect(PostgresConfig::new(database_url.to_string())).await?;

    match command {
        PreparedQueryCommand::Incidents(args) => run_incidents(&sink, args).await,
        PreparedQueryCommand::Incident(args) => run_incident(&sink, args).await,
        PreparedQueryCommand::IncidentProducts(args) => run_incident_products(&sink, args).await,
        PreparedQueryCommand::Products(query) => run_products(&sink, query).await,
        PreparedQueryCommand::Product(product_id) => run_product(&sink, product_id).await,
        PreparedQueryCommand::Issues(args) => run_issues(&sink, args).await,
        PreparedQueryCommand::Issue(issue_id) => run_issue(&sink, issue_id).await,
        PreparedQueryCommand::ProductRaw(args) => run_product_raw(&sink, args).await,
        PreparedQueryCommand::Features(query) => run_features(&sink, query).await,
        PreparedQueryCommand::FeaturesGeojson(query) => run_features_geojson(&sink, query).await,
        PreparedQueryCommand::AggregateFacets(query) => run_aggregate_facets(&sink, query).await,
        PreparedQueryCommand::AggregateTimeseries(query) => {
            run_aggregate_timeseries(&sink, query).await
        }
        PreparedQueryCommand::AggregateCells(query) => run_aggregate_cells(&sink, query).await,
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

async fn run_products(sink: &PostgresMetadataSink, query: ProductListQuery) -> CliResult<()> {
    let page = sink.list_archived_products(query).await?;

    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, &ProductsResponse::from_page(page))
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

async fn run_features(sink: &PostgresMetadataSink, query: FeatureListQuery) -> CliResult<()> {
    let page = sink.list_archived_features(query).await?;

    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, &FeaturesResponse::from_page(page))
}

async fn run_features_geojson(
    sink: &PostgresMetadataSink,
    query: FeatureListQuery,
) -> CliResult<()> {
    let page = sink.list_archived_features(query).await?;

    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, &FeatureCollectionResponse::from_page(page))
}

async fn run_aggregate_facets(
    sink: &PostgresMetadataSink,
    query: FacetAggregateQuery,
) -> CliResult<()> {
    let items = sink.list_facet_aggregate(query.clone()).await?;

    let mut stdout = io::stdout().lock();
    write_json(
        &mut stdout,
        &FacetAggregateResponse {
            dimension: query.dimension.as_str().to_string(),
            completeness: items.completeness,
            items: items.items,
        },
    )
}

async fn run_aggregate_timeseries(
    sink: &PostgresMetadataSink,
    query: TimeseriesAggregateQuery,
) -> CliResult<()> {
    let items = sink.list_timeseries_aggregate(query.clone()).await?;

    let mut stdout = io::stdout().lock();
    write_json(
        &mut stdout,
        &TimeseriesAggregateResponse {
            measure: query.measure.as_str().to_string(),
            bucket: query.bucket.as_str().to_string(),
            start: query.start,
            end: query.end,
            completeness: items.completeness,
            items: items.items,
        },
    )
}

async fn run_aggregate_cells(
    sink: &PostgresMetadataSink,
    query: CellAggregateQuery,
) -> CliResult<()> {
    let items = sink.list_cell_aggregate(query.clone()).await?;

    let mut stdout = io::stdout().lock();
    write_json(
        &mut stdout,
        &CellAggregateResponse {
            measure: query.measure.as_str().to_string(),
            precision: query.precision,
            completeness: items.completeness,
            items: items.items,
        },
    )
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
    use emwin_service::parse_archive_bool;

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

    #[test]
    fn parse_bool_flag_rejects_invalid_literal() {
        let error = parse_archive_bool("has_issues", Some("maybe"))
            .expect_err("invalid bool literal should fail");
        assert!(error.to_string().contains("has_issues must be one of"));
    }
}
