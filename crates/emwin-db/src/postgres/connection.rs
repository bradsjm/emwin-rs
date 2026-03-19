use super::{PersistError, PersistResult, PostgresConfig};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use tracing::info;

pub(super) async fn connect_pool(config: &PostgresConfig) -> PersistResult<PgPool> {
    if config.database_url.trim().is_empty() {
        return Err(PersistError::InvalidConfig(
            "postgres database url must not be empty".to_string(),
        ));
    }
    if config.application_name.trim().is_empty() {
        return Err(PersistError::InvalidConfig(
            "postgres application name must not be empty".to_string(),
        ));
    }

    let options = connect_options(config)?;
    let connect_target = describe_connect_target(&options);
    info!(
        target = %connect_target,
        connect_timeout_secs = config.connect_timeout.as_secs_f64(),
        application_name = %config.application_name,
        "connecting to postgres"
    );
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections.max(1))
        .acquire_timeout(config.connect_timeout)
        .connect_with(options)
        .await?;

    super::MIGRATOR.run(&pool).await?;
    sqlx::query_scalar::<_, String>("SELECT postgis_version()")
        .fetch_one(&pool)
        .await?;

    Ok(pool)
}

pub(super) fn connect_options(config: &PostgresConfig) -> PersistResult<PgConnectOptions> {
    Ok(
        PgConnectOptions::from_str(&config.database_url)?
            .application_name(&config.application_name),
    )
}

pub(super) fn connection_target(config: &PostgresConfig) -> PersistResult<String> {
    Ok(describe_connect_target(&connect_options(config)?))
}

pub(super) fn describe_connect_target(options: &PgConnectOptions) -> String {
    match options.get_socket() {
        Some(socket) => match options.get_database() {
            Some(database) if !database.is_empty() => {
                format!("unix:{} / {}", socket.display(), database)
            }
            _ => format!("unix:{}", socket.display()),
        },
        None => match options.get_database() {
            Some(database) if !database.is_empty() => {
                format!(
                    "{}:{} / {}",
                    options.get_host(),
                    options.get_port(),
                    database
                )
            }
            _ => format!("{}:{}", options.get_host(), options.get_port()),
        },
    }
}
