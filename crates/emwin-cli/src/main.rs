//! EMWIN CLI - Command-line interface for EMWIN protocol.
//!
//! This application provides commands for:
//! - Running the live HTTP server with SSE and file endpoints

#![recursion_limit = "4096"]

mod cmd;
mod error;
mod relay;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use std::io::IsTerminal;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Supported upstream receiver backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ReceiverKind {
    /// QBT/EMWIN TCP receiver.
    Qbt,
    /// Weather Wire XMPP receiver.
    Wxwire,
}

/// Available CLI commands.
#[derive(Debug, Subcommand)]
enum Commands {
    /// Query archived incident and product data directly from persistence.
    Query {
        #[command(flatten)]
        options: Box<cmd::query::QueryOptions>,
    },
    /// Live command with HTTP, SSE, and retained file endpoints.
    Server {
        /// Optional object-store root URI for async blob persistence, for example `file:///tmp/emwin` or `s3://bucket/prefix`.
        #[arg(long, env = "EMWIN_OUTPUT_DIR")]
        output_dir: Option<String>,
        /// Whether completed ZIP/ZIS archives should be extracted before downstream handling.
        #[arg(
            long,
            env = "EMWIN_POST_PROCESS_ARCHIVES",
            default_value = "true",
            action = ArgAction::Set
        )]
        post_process_archives: bool,
        /// Account username for authentication.
        #[arg(long, env = "EMWIN_USERNAME")]
        username: String,
        /// Password for receivers that require one (for example wxwire).
        #[arg(long, env = "EMWIN_PASSWORD")]
        password: Option<String>,
        /// Receiver backend to use.
        #[arg(long, value_enum, env = "EMWIN_RECEIVER", default_value_t = ReceiverKind::Qbt)]
        receiver: ReceiverKind,
        /// Custom QBT server endpoints (comma-separated or multiple). Pins the runtime to this list.
        #[arg(long = "server", env = "EMWIN_SERVER", value_delimiter = ',')]
        servers: Vec<String>,
        /// Path to persisted automatic QBT server list file. Rejected when --server is set.
        #[arg(long, env = "EMWIN_SERVER_LIST_PATH")]
        server_list_path: Option<String>,
        /// Bind address for the HTTP server.
        #[arg(long, env = "EMWIN_BIND", default_value = "127.0.0.1:8080")]
        bind: String,
        /// CORS origin header (use "*" for any).
        #[arg(long, env = "EMWIN_CORS_ORIGIN")]
        cors_origin: Option<String>,
        /// Maximum concurrent SSE clients.
        #[arg(long, env = "EMWIN_MAX_CLIENTS", default_value_t = 100)]
        max_clients: usize,
        /// Stats logging interval in seconds (0 to disable).
        #[arg(long, env = "EMWIN_STATS_INTERVAL_SECS", default_value_t = 30)]
        stats_interval_secs: u64,
        /// File retention time in seconds.
        #[arg(long, env = "EMWIN_FILE_RETENTION_SECS", default_value_t = 300)]
        file_retention_secs: u64,
        /// Maximum number of retained files.
        #[arg(long, env = "EMWIN_MAX_RETAINED_FILES", default_value_t = 1000)]
        max_retained_files: usize,
        /// Suppress non-error output.
        #[arg(long, env = "EMWIN_QUIET", default_value_t = false)]
        quiet: bool,
        /// Maximum number of queued persistence requests before evicting the oldest request.
        #[arg(long, env = "EMWIN_PERSIST_QUEUE_CAPACITY", default_value_t = 1024)]
        persist_queue_capacity: usize,
        /// Optional Postgres metadata sink URL used alongside --output-dir blob storage.
        #[arg(long, env = "EMWIN_PERSIST_DATABASE_URL")]
        persist_database_url: Option<String>,
        /// Maximum Postgres connections used for archive metadata access and persistence.
        #[arg(
            long,
            env = "EMWIN_MAX_DB_CONNECTIONS",
            default_value_t = emwin_db::DEFAULT_MAX_DB_CONNECTIONS,
            value_parser = clap::value_parser!(u32).range(1..)
        )]
        max_db_connections: u32,
        /// Optional Bearer token required for versioned HTTP and SSE API routes.
        #[arg(long, env = "EMWIN_OPENAPI_AUTH_TOKEN")]
        openapi_auth_token: Option<String>,
        /// Optional Apprise API base URL used by alerting contact-point test sends.
        #[arg(long, env = "EMWIN_APPRISE_API_URL")]
        alerting_apprise_api_url: Option<String>,
    },
    /// Run the alert worker against Postgres-backed alerting state.
    AlertWorker {
        /// Postgres metadata sink URL used for alerting state.
        #[arg(long, env = "EMWIN_PERSIST_DATABASE_URL")]
        database_url: String,
        /// Optional Apprise API base URL, for example `http://127.0.0.1:8000`.
        #[arg(long, env = "EMWIN_APPRISE_API_URL")]
        apprise_api_url: Option<String>,
        /// Maximum number of source events claimed per poll.
        #[arg(long, env = "EMWIN_ALERT_SOURCE_BATCH_SIZE", default_value_t = 32)]
        source_batch_size: i64,
        /// Maximum number of delivery attempts claimed per poll.
        #[arg(long, env = "EMWIN_ALERT_DELIVERY_BATCH_SIZE", default_value_t = 32)]
        delivery_batch_size: i64,
        /// Idle poll interval in seconds.
        #[arg(long, env = "EMWIN_ALERT_IDLE_POLL_SECS", default_value_t = 2)]
        idle_poll_secs: u64,
        /// Source-event claim lease in seconds.
        #[arg(
            long,
            env = "EMWIN_ALERT_SOURCE_CLAIM_LEASE_SECS",
            default_value_t = 300,
            value_parser = clap::value_parser!(u64).range(1..)
        )]
        source_claim_lease_secs: u64,
        /// Delivery-attempt claim lease in seconds.
        #[arg(
            long,
            env = "EMWIN_ALERT_DELIVERY_CLAIM_LEASE_SECS",
            default_value_t = 300,
            value_parser = clap::value_parser!(u64).range(1..)
        )]
        delivery_claim_lease_secs: u64,
        /// Default outbound HTTP request timeout in seconds.
        #[arg(
            long,
            env = "EMWIN_ALERT_HTTP_TIMEOUT_SECS",
            default_value_t = 30,
            value_parser = clap::value_parser!(u64).range(1..)
        )]
        http_timeout_secs: u64,
        /// Maximum Postgres connections used by the worker.
        #[arg(
            long,
            env = "EMWIN_MAX_DB_CONNECTIONS",
            default_value_t = emwin_db::DEFAULT_MAX_DB_CONNECTIONS,
            value_parser = clap::value_parser!(u32).range(1..)
        )]
        max_db_connections: u32,
    },
    /// Run low-latency EMWIN passthrough relay.
    Relay {
        #[command(flatten)]
        options: relay::RelayOptions,
    },
}

impl Commands {
    fn name(&self) -> &'static str {
        match self {
            Self::Query { .. } => "query",
            Self::Server { .. } => "server",
            Self::AlertWorker { .. } => "alert-worker",
            Self::Relay { .. } => "relay",
        }
    }
}

impl From<ReceiverKind> for emwin_live::ReceiverKind {
    fn from(value: ReceiverKind) -> Self {
        match value {
            ReceiverKind::Qbt => Self::Qbt,
            ReceiverKind::Wxwire => Self::Wxwire,
        }
    }
}

/// CLI argument parser for emwin.
#[derive(Debug, Parser)]
#[command(name = "emwin")]
#[command(about = "EMWIN console client")]
struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() -> crate::error::CliResult<()> {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    init_logging();
    log_startup(&cli.command);

    match cli.command {
        Commands::Query { options } => cmd::query::run(*options).await,
        Commands::Server {
            output_dir,
            post_process_archives,
            username,
            password,
            receiver,
            servers,
            server_list_path,
            bind,
            cors_origin,
            max_clients,
            stats_interval_secs,
            file_retention_secs,
            max_retained_files,
            quiet,
            persist_queue_capacity,
            persist_database_url,
            max_db_connections,
            openapi_auth_token,
            alerting_apprise_api_url,
        } => {
            let live = emwin_live::LiveRuntime::start(emwin_live::LiveOptions {
                username,
                password,
                receiver: receiver.into(),
                raw_servers: servers,
                server_list_path,
                output_dir,
                post_process_archives,
                quiet,
                persistence_queue_capacity: persist_queue_capacity,
                postgres_database_url: persist_database_url,
                max_db_connections,
                file_retention_secs,
                max_retained_files,
            })
            .await?;
            let options = emwin_api::HttpServerOptions {
                bind,
                cors_origin,
                max_clients,
                stats_interval_secs,
                quiet,
                openapi_auth_token,
                alerting_apprise_api_url,
            };
            let services = emwin_api::ApiServices::from_live_runtime(live);
            emwin_api::serve(options, services)
                .await
                .map_err(Into::into)
        }
        Commands::AlertWorker {
            database_url,
            apprise_api_url,
            source_batch_size,
            delivery_batch_size,
            idle_poll_secs,
            source_claim_lease_secs,
            delivery_claim_lease_secs,
            http_timeout_secs,
            max_db_connections,
        } => {
            let mut config = emwin_db::PostgresConfig::new(database_url);
            config.application_name = "emwin-alert-worker".to_string();
            config.max_connections = max_db_connections;
            let sink = emwin_db::PostgresMetadataSink::connect(config).await?;
            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
            let mut worker = tokio::spawn(emwin_alert::run_worker(
                sink,
                emwin_alert::AlertWorkerConfig {
                    source_batch_size,
                    delivery_batch_size,
                    idle_poll_interval: std::time::Duration::from_secs(idle_poll_secs.max(1)),
                    source_claim_lease: std::time::Duration::from_secs(source_claim_lease_secs),
                    delivery_claim_lease: std::time::Duration::from_secs(delivery_claim_lease_secs),
                    http_timeout: std::time::Duration::from_secs(http_timeout_secs),
                    apprise_api_url,
                },
                shutdown_rx,
            ));
            tokio::select! {
                result = &mut worker => match result {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(err)) => Err(crate::error::CliError::Runtime(err.to_string())),
                    Err(err) => Err(crate::error::CliError::Runtime(err.to_string())),
                },
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    let _ = shutdown_tx.send(true);
                    match worker.await {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(err)) => Err(crate::error::CliError::Runtime(err.to_string())),
                        Err(err) => Err(crate::error::CliError::Runtime(err.to_string())),
                    }
                }
            }
        }
        Commands::Relay { options } => relay::runtime::run(options).await,
    }
}

fn log_startup(command: &Commands) {
    info!(
        package = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
        subcommand = command.name(),
        "starting emwin CLI"
    );
}

fn init_logging() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let ansi = match std::env::var("RUST_LOG_STYLE") {
        Ok(value) => match value.to_ascii_lowercase().as_str() {
            "always" => true,
            "never" => false,
            _ => std::io::stderr().is_terminal(),
        },
        Err(_) => std::io::stderr().is_terminal(),
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .with_ansi(ansi)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn cli_parses_representative_commands() {
        let query_cases = [
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "incidents",
                "--office",
                "KOAX",
                "--limit",
                "25",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "incident",
                "KOAX",
                "FF",
                "W",
                "2001",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "incident-products",
                "KOAX",
                "FF",
                "W",
                "2001",
                "--cursor",
                "opaque",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "products",
                "--office",
                "KOAX",
                "--artifact-kind",
                "nws_text_product",
                "--min-lat",
                "41.0",
                "--max-lat",
                "42.0",
                "--min-lon=-97.0",
                "--max-lon=-95.0",
                "--limit",
                "25",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "product",
                "42",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "features",
                "--kind",
                "polygon",
                "--limit",
                "25",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "features-geojson",
                "--kind",
                "search_point",
                "--limit",
                "50",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "aggregate-facets",
                "office",
                "--limit",
                "10",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "aggregate-timeseries",
                "product_count",
                "--start",
                "2025-03-05T12:00:00Z",
                "--end",
                "2025-03-05T15:00:00Z",
                "--bucket",
                "hour",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "aggregate-cells",
                "product_count",
                "--precision",
                "5",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "issues",
                "--product-id",
                "42",
                "--kind",
                "text_product_parse",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "issue",
                "7",
            ]
            .as_slice(),
            [
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "product-raw",
                "42",
                "--stdout",
            ]
            .as_slice(),
        ];

        for args in query_cases {
            let cli = Cli::try_parse_from(args).expect("query args should parse");
            assert!(matches!(cli.command, Commands::Query { .. }));
        }

        let server_cases = [
            (
                [
                    "emwin",
                    "server",
                    "--username",
                    "test@example.com",
                    "--output-dir",
                    "file:///tmp/out",
                    "--persist-queue-capacity",
                    "55",
                    "--persist-database-url",
                    "postgres://localhost/emwin",
                    "--max-db-connections",
                    "16",
                    "--openapi-auth-token",
                    "secret-token",
                ]
                .as_slice(),
                Some("file:///tmp/out"),
                Some(55usize),
                Some("postgres://localhost/emwin"),
                Some(16u32),
                Some("secret-token"),
            ),
            (
                [
                    "emwin",
                    "server",
                    "--username",
                    "test@example.com",
                    "--output-dir",
                    "s3://bucket/prefix",
                ]
                .as_slice(),
                Some("s3://bucket/prefix"),
                None,
                None,
                None,
                None,
            ),
        ];

        for (
            args,
            expected_output_dir,
            expected_capacity,
            expected_database_url,
            expected_max_db_connections,
            expected_token,
        ) in server_cases
        {
            let cli = Cli::try_parse_from(args).expect("server args should parse");
            let Commands::Server {
                output_dir,
                persist_queue_capacity,
                persist_database_url,
                max_db_connections,
                openapi_auth_token,
                ..
            } = cli.command
            else {
                panic!("expected server command");
            };

            assert_eq!(output_dir.as_deref(), expected_output_dir);
            if let Some(capacity) = expected_capacity {
                assert_eq!(persist_queue_capacity, capacity);
            }
            assert_eq!(persist_database_url.as_deref(), expected_database_url);
            if let Some(expected_max_db_connections) = expected_max_db_connections {
                assert_eq!(max_db_connections, expected_max_db_connections);
            }
            assert_eq!(openapi_auth_token.as_deref(), expected_token);
        }

        let relay = Cli::try_parse_from(["emwin", "relay", "--username", "test@example.com"])
            .expect("relay args should parse");
        assert!(matches!(relay.command, Commands::Relay { .. }));

        let alert_worker = Cli::try_parse_from([
            "emwin",
            "alert-worker",
            "--database-url",
            "postgres://localhost/emwin",
            "--source-claim-lease-secs",
            "60",
            "--delivery-claim-lease-secs",
            "120",
            "--http-timeout-secs",
            "15",
        ])
        .expect("alert worker args should parse");
        let Commands::AlertWorker {
            source_claim_lease_secs,
            delivery_claim_lease_secs,
            http_timeout_secs,
            ..
        } = alert_worker.command
        else {
            panic!("expected alert-worker command");
        };
        assert_eq!(source_claim_lease_secs, 60);
        assert_eq!(delivery_claim_lease_secs, 120);
        assert_eq!(http_timeout_secs, 15);
    }

    #[test]
    fn invalid_subcommand_is_rejected() {
        assert!(Cli::try_parse_from(["emwin", "download", "./out"]).is_err());
    }

    #[test]
    fn product_raw_requires_exactly_one_output_sink() {
        assert!(
            Cli::try_parse_from([
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "product-raw",
                "42",
            ])
            .is_err()
        );

        assert!(
            Cli::try_parse_from([
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "product-raw",
                "42",
                "--stdout",
                "--output",
                "./payload.bin",
            ])
            .is_err()
        );
    }

    #[test]
    fn incidents_reject_invalid_rfc3339_timestamp() {
        assert!(
            Cli::try_parse_from([
                "emwin",
                "query",
                "--database-url",
                "postgres://localhost/emwin",
                "incidents",
                "--updated-after",
                "not-a-timestamp",
            ])
            .is_err()
        );
    }

    #[test]
    fn server_defaults_and_validates_max_db_connections() {
        let cli = Cli::try_parse_from(["emwin", "server", "--username", "test@example.com"])
            .expect("server args should parse");
        let Commands::Server {
            max_db_connections, ..
        } = cli.command
        else {
            panic!("expected server command");
        };
        assert_eq!(max_db_connections, emwin_db::DEFAULT_MAX_DB_CONNECTIONS);

        assert!(
            Cli::try_parse_from([
                "emwin",
                "server",
                "--username",
                "test@example.com",
                "--max-db-connections",
                "0",
            ])
            .is_err()
        );
    }

    #[test]
    fn alert_worker_rejects_zero_lease_and_timeout_values() {
        for arg in [
            "--source-claim-lease-secs",
            "--delivery-claim-lease-secs",
            "--http-timeout-secs",
        ] {
            assert!(
                Cli::try_parse_from([
                    "emwin",
                    "alert-worker",
                    "--database-url",
                    "postgres://localhost/emwin",
                    arg,
                    "0",
                ])
                .is_err()
            );
        }
    }
}
