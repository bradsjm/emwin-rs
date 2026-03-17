//! EMWIN CLI - Command-line interface for EMWIN protocol.
//!
//! This application provides commands for:
//! - Running the live HTTP server with SSE and file endpoints

mod cmd;
mod default_servers;
mod error;
mod live;
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
            Self::Server { .. } => "server",
            Self::Relay { .. } => "relay",
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
        } => {
            let options = live::server::ServerOptions {
                username,
                password,
                receiver,
                raw_servers: servers,
                server_list_path,
                bind,
                cors_origin,
                max_clients,
                stats_interval_secs,
                file_retention_secs,
                max_retained_files,
                output_dir,
                post_process_archives,
                quiet,
                persistence_queue_capacity: persist_queue_capacity,
                postgres_database_url: persist_database_url,
            };
            live::server::run(options).await
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
    use clap::{CommandFactory, Parser};

    #[test]
    fn root_help_does_not_list_download() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("server"));
        assert!(!help.contains("stream"));
        assert!(!help.contains("download"));
    }

    #[test]
    fn server_help_mentions_post_process_archives() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("server")
            .expect("server subcommand should exist")
            .render_long_help()
            .to_string();

        assert!(help.contains("--post-process-archives"));
        assert!(help.contains("--output-dir"));
        assert!(help.contains("--persist-database-url"));
    }

    #[test]
    fn download_subcommand_is_rejected() {
        let error = Cli::try_parse_from(["emwin", "download", "./out"])
            .expect_err("download subcommand should be rejected");

        assert!(
            error
                .to_string()
                .contains("unrecognized subcommand 'download'")
        );
    }

    #[test]
    fn server_accepts_output_dir_queue_capacity_and_database_url() {
        let cli = Cli::try_parse_from([
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
        ])
        .expect("server args should parse");

        let Commands::Server {
            output_dir,
            persist_queue_capacity,
            persist_database_url,
            ..
        } = cli.command
        else {
            panic!("expected server command");
        };

        assert_eq!(output_dir.as_deref(), Some("./out"));
        assert_eq!(persist_queue_capacity, 55);
        assert_eq!(
            persist_database_url.as_deref(),
            Some("postgres://localhost/emwin")
        );
    }

    #[test]
    fn server_accepts_s3_output_dir() {
        let cli = Cli::try_parse_from([
            "emwin",
            "server",
            "--username",
            "test@example.com",
            "--output-dir",
            "s3://bucket/prefix",
        ])
        .expect("server args should parse");

        let Commands::Server { output_dir, .. } = cli.command else {
            panic!("expected server command");
        };

        assert_eq!(output_dir.as_deref(), Some("s3://bucket/prefix"));
    }

    #[test]
    fn command_name_matches_subcommand() {
        let server = Cli::try_parse_from(["emwin", "server", "--username", "test@example.com"])
            .expect("server args should parse");
        let relay = Cli::try_parse_from(["emwin", "relay", "--username", "test@example.com"])
            .expect("relay args should parse");

        assert_eq!(server.command.name(), "server");
        assert_eq!(relay.command.name(), "relay");
        assert!(!env!("CARGO_PKG_VERSION").is_empty());
    }
}
