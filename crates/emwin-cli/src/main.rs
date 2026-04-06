//! EMWIN CLI - Command-line interface for EMWIN protocol.
//!
//! This application provides commands for:
//! - Running the live HTTP server with SSE and file endpoints

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
        /// Optional filesystem path or `s3://bucket[/prefix]` URI for async blob persistence.
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
        /// Custom server endpoints (comma-separated or multiple).
        #[arg(long = "server", env = "EMWIN_SERVER", value_delimiter = ',')]
        servers: Vec<String>,
        /// Path to persisted server list file.
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
        /// Optional Bearer token required for versioned HTTP and SSE API routes.
        #[arg(long, env = "EMWIN_OPENAPI_AUTH_TOKEN")]
        openapi_auth_token: Option<String>,
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
            openapi_auth_token,
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
            };
            emwin_api::serve(options, live).await.map_err(Into::into)
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
                    "./out",
                    "--persist-queue-capacity",
                    "55",
                    "--persist-database-url",
                    "postgres://localhost/emwin",
                    "--openapi-auth-token",
                    "secret-token",
                ]
                .as_slice(),
                Some("./out"),
                Some(55usize),
                Some("postgres://localhost/emwin"),
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
            ),
        ];

        for (args, expected_output_dir, expected_capacity, expected_database_url, expected_token) in
            server_cases
        {
            let cli = Cli::try_parse_from(args).expect("server args should parse");
            let Commands::Server {
                output_dir,
                persist_queue_capacity,
                persist_database_url,
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
            assert_eq!(openapi_auth_token.as_deref(), expected_token);
        }

        let relay = Cli::try_parse_from(["emwin", "relay", "--username", "test@example.com"])
            .expect("relay args should parse");
        assert!(matches!(relay.command, Commands::Relay { .. }));
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
}
