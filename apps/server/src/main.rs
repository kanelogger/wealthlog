use wealthfolio_server::{
    api, config::Config, init_tracing, run_profile_database_maintenance, static_files,
};

/// Offline database maintenance, run with the server stopped.
///
/// Converting the database replaces the file, which requires that nothing is
/// connected to it — so it is a command, not an API call.
fn run_maintenance_cli(args: &[String]) -> Option<anyhow::Result<()>> {
    if args.first().map(String::as_str) != Some("db") {
        return None;
    }

    let mut selected = None;
    let mut filtered = args.to_vec();
    if let Some(index) = filtered.iter().position(|s| s == "--profile") {
        let value = match filtered
            .get(index + 1)
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
        {
            Some(id) => id,
            None => {
                return Some(Err(anyhow::anyhow!(
                    "--profile requires a registered profile UUID"
                )))
            }
        };
        selected = Some(value);
        filtered.drain(index..=index + 1);
    }
    let args = filtered.as_slice();

    // Once `db` is given, a missing or unknown subcommand is an error. Falling
    // through would silently start the server instead of converting anything.
    let encrypt = match args.get(1).map(String::as_str) {
        Some("encrypt") => true,
        Some("decrypt") => false,
        Some(other) => {
            return Some(Err(anyhow::anyhow!(
                "Unknown database command 'db {other}'. Expected 'db encrypt' or 'db decrypt'."
            )))
        }
        None => {
            return Some(Err(anyhow::anyhow!(
                "Missing database command. Expected 'db encrypt' or 'db decrypt'."
            )))
        }
    };

    if args.len() != 2 {
        return Some(Err(anyhow::anyhow!(
            "Unexpected database maintenance arguments"
        )));
    }
    init_tracing();
    Some(run_profile_database_maintenance(encrypt, selected))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(result) = run_maintenance_cli(&args) {
        return result;
    }

    let config = Config::from_env()?;
    init_tracing();
    // Bind before starting database workers so a port conflict cannot strand them.
    let listener = tokio::net::TcpListener::bind(config.listen_addr).await?;

    if let Some(ref auth) = config.auth {
        tracing::info!(
            "Authentication enabled, cookie secure policy: {}",
            auth.cookie_secure
        );
    } else {
        tracing::warn!(
            "Authentication disabled: every client that can reach {} can write through REST \
             without a user session, including proxy-forwarded requests. Anonymous calls are \
             audited as unauthenticated. Enable ledger locking in Settings; this server never \
             changes settings automatically.",
            config.listen_addr
        );
    }
    if config.listen_addr.ip().is_loopback() {
        tracing::info!("Listen scope: loopback");
    } else {
        tracing::warn!("Listen scope: non_loopback");
    }
    let static_dir = std::path::PathBuf::from(&config.static_dir);
    let router = api::app_router_from_config(&config)
        .await
        .inspect_err(|error| {
            tracing::error!(error = %format!("{error:#}"), "Server startup failed; no requests were served");
        })?
        .fallback_service(static_files::router(&static_dir))
        .layer(axum::middleware::from_fn(api::security_headers));
    tracing::info!("Listening on {}", listener.local_addr()?);
    let result = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await;
    result?;
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::run_maintenance_cli;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn non_db_arguments_fall_through_to_normal_startup() {
        assert!(run_maintenance_cli(&args(&[])).is_none());
        assert!(run_maintenance_cli(&args(&["--version"])).is_none());
    }

    #[test]
    fn db_without_a_subcommand_is_an_error_not_a_server_start() {
        let result = run_maintenance_cli(&args(&["db"])).expect("must not fall through");
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Missing database command"));
    }

    #[test]
    fn db_with_an_unknown_subcommand_is_an_error() {
        let result = run_maintenance_cli(&args(&["db", "rotate"])).expect("must not fall through");
        assert!(result.unwrap_err().to_string().contains("db rotate"));
    }
}
