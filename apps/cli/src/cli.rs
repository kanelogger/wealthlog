//! Command-line surface. Every command maps to one shared MCP tool.

use std::io::Read;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde_json::Value;
use uuid::Uuid;

use crate::client::Session;
use crate::discovery;
use crate::error::{CliError, EXIT_OK};
use crate::token;

const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "wealthlog",
    version,
    about = "Local Wealthlog CLI (bookkeeping, transfers, manual holdings and cash)",
    long_about = "Calls the running Wealthlog desktop service over its private Unix socket. \
                  Writes require --ledger and a frozen requestKey in the input JSON. The CLI \
                  prints one JSON object on stdout and logs nothing else there; tokens are never \
                  printed. Exit codes: 0 success, 1 internal, 2 usage/validation, 3 auth/scope, \
                  4 conflict (including duplicate_activity), 5 unavailable/unknown result, \
                  6 profile locked."
)]
pub struct Cli {
    /// Application identifier whose discovery file to use.
    #[arg(long, global = true, default_value = discovery::PRODUCTION_APP_ID)]
    pub app_id: String,
    /// File holding the PAT (mode 0400/0600, current user, no symlinks).
    #[arg(long, global = true, value_name = "FILE")]
    pub token_file: Option<PathBuf>,
    /// Ledger id. Required for writes; resolved from the discovery file for reads.
    #[arg(long, global = true, value_name = "UUID")]
    pub ledger: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Account reads.
    Accounts {
        #[command(subcommand)]
        command: AccountsCommand,
    },
    /// Ordinary income/expense reads and writes.
    Ordinary {
        #[command(subcommand)]
        command: OrdinaryCommand,
    },
    /// Pending (unconfirmed) entries.
    Pending {
        #[command(subcommand)]
        command: PendingCommand,
    },
    /// Per-account holdings and cash state, manual snapshots and trades.
    Holdings {
        #[command(subcommand)]
        command: HoldingsCommand,
    },
    /// Internal cash-transfer bookkeeping.
    Transfers {
        #[command(subcommand)]
        command: TransfersCommand,
    },
    /// Local service discovery.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AccountsCommand {
    /// List accounts.
    List,
}

#[derive(Debug, Subcommand)]
pub enum OrdinaryCommand {
    /// Read eligible accounts, live categories, rules and recent classified entries.
    Context,
    /// List ordinary activities.
    List {
        #[arg(long, default_value_t = 0)]
        page: i64,
        #[arg(long = "page-size", default_value_t = 50)]
        page_size: i64,
        #[arg(long = "account-id")]
        account_ids: Vec<String>,
        #[arg(long = "date-from", value_name = "YYYY-MM-DD")]
        date_from: Option<String>,
        #[arg(long = "date-to", value_name = "YYYY-MM-DD")]
        date_to: Option<String>,
    },
    /// Read one ordinary activity with all editable fields.
    Get { activity_id: String },
    /// Create one posted ordinary activity.
    Create {
        #[arg(long, value_name = "FILE|-")]
        input: String,
    },
    /// Update one posted ordinary activity.
    Update {
        #[arg(long, value_name = "FILE|-")]
        input: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum PendingCommand {
    /// List pending entries.
    List {
        #[arg(long = "include-resolved")]
        include_resolved: bool,
    },
    /// Store a pending entry for review in the app.
    Create {
        #[arg(long, value_name = "FILE|-")]
        input: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum HoldingsCommand {
    /// Today's exact holdings, cash by currency and stateVersion.
    Get {
        #[arg(long)]
        account_id: String,
    },
    /// Patch specified positions/currencies in today's manual snapshot.
    Patch {
        #[arg(long, value_name = "FILE|-")]
        input: String,
    },
    /// Record an executed manual-account BUY/SELL, without placing an order.
    Trade {
        #[arg(long, value_name = "FILE|-")]
        input: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum TransfersCommand {
    /// Atomically record both legs of an already-completed cash transfer.
    Create {
        #[arg(long, value_name = "FILE|-")]
        input: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    /// Show discovery state without authenticating.
    Status,
}

pub struct Outcome {
    pub ledger_id: Option<String>,
    pub request_key: Option<String>,
    pub request_id: Option<String>,
    pub data: Value,
}

pub fn validate_ledger(value: &str) -> Result<String, CliError> {
    Uuid::parse_str(value)
        .map(|id| id.to_string())
        .map_err(|_| CliError::usage("--ledger must be a UUID"))
}

fn read_input(path: &str) -> Result<Value, CliError> {
    let mut buffer = Vec::new();
    if path == "-" {
        std::io::stdin()
            .take(MAX_INPUT_BYTES as u64 + 1)
            .read_to_end(&mut buffer)
            .map_err(|error| CliError::usage(format!("cannot read stdin: {error}")))?;
    } else {
        let file = std::fs::File::open(path)
            .map_err(|error| CliError::usage(format!("cannot open --input: {error}")))?;
        file.take(MAX_INPUT_BYTES as u64 + 1)
            .read_to_end(&mut buffer)
            .map_err(|error| CliError::usage(format!("cannot read --input: {error}")))?;
    }
    if buffer.len() > MAX_INPUT_BYTES {
        return Err(CliError::usage(format!(
            "--input exceeds {MAX_INPUT_BYTES} bytes"
        )));
    }
    let value: Value = serde_json::from_slice(&buffer)
        .map_err(|error| CliError::usage(format!("--input is not valid JSON: {error}")))?;
    if !value.is_object() {
        return Err(CliError::usage("--input must be a JSON object"));
    }
    Ok(value)
}

fn require_request_key(value: &Value) -> Result<String, CliError> {
    match value.get("requestKey").and_then(Value::as_str) {
        Some(key) if !key.trim().is_empty() => Ok(key.to_string()),
        _ => Err(CliError::usage(
            "the input must contain a non-blank requestKey",
        )),
    }
}

fn required_ledger(cli: &Cli) -> Result<String, CliError> {
    let ledger = cli
        .ledger
        .as_deref()
        .ok_or_else(|| CliError::usage("this command requires --ledger <UUID>"))?;
    validate_ledger(ledger)
}

async fn resolved_ledger(cli: &Cli, lock_ledger: Option<&str>) -> Result<String, CliError> {
    match cli.ledger.as_deref() {
        Some(ledger) => validate_ledger(ledger),
        None => lock_ledger
            .map(str::to_string)
            .ok_or_else(|| CliError::unavailable("the service has no active ledger")),
    }
}

pub async fn run(cli: Cli) -> Result<Outcome, CliError> {
    discovery::validate_app_id(&cli.app_id)?;

    if let Command::Service {
        command: ServiceCommand::Status,
    } = &cli.command
    {
        return service_status(&cli).await;
    }

    // Discovery comes first: a locked ledger must answer before any credential
    // is inspected, and a cold start must fail as unavailable even without a
    // token. Credentials are still resolved before the socket is opened.
    let discovery = discovery::load(&cli.app_id)?;
    if discovery.lock.status == "locked" {
        return Err(CliError::locked(
            "the profile is locked; unlock it in the Wealthlog app",
        ));
    }
    let token = token::resolve(cli.token_file.as_deref())?.ok_or_else(|| {
        CliError::auth("no token configured; pass --token-file or set WEALTHLOG_TOKEN")
    })?;
    let active = discovery.lock.active_ledger_id.clone();
    let needs_write_ledger = matches!(
        cli.command,
        Command::Ordinary {
            command: OrdinaryCommand::Create { .. } | OrdinaryCommand::Update { .. }
        } | Command::Pending {
            command: PendingCommand::Create { .. }
        } | Command::Transfers { .. }
            | Command::Holdings {
                command: HoldingsCommand::Patch { .. } | HoldingsCommand::Trade { .. }
            }
    );
    let ledger = if needs_write_ledger {
        required_ledger(&cli)?
    } else {
        resolved_ledger(&cli, active.as_deref()).await?
    };
    if needs_write_ledger {
        if let Some(active) = active.as_deref() {
            if active != ledger {
                return Err(CliError::profile_mismatch(
                    "the requested ledger is not the active ledger; switch the app profile first",
                    Some(active),
                    &ledger,
                ));
            }
        }
    }

    let session = Session::connect(&discovery, &token, ledger.clone())
        .await
        .map_err(|error| error.with_ledger(ledger.clone()))?;
    let result = execute(&session, &cli.command).await;
    session.close().await;
    let result = result.map_err(|error| error.with_ledger(ledger.clone()));

    let (request_key, request_id, data) = result?;
    Ok(Outcome {
        ledger_id: Some(ledger),
        request_key,
        request_id,
        data,
    })
}

async fn execute_portfolio_write(
    session: &Session,
    tool: &str,
    input: &str,
) -> Result<(Option<String>, Option<String>, Value), CliError> {
    let value = read_input(input)?;
    let key = require_request_key(&value)?;
    let (_, data) = session
        .call(tool, value)
        .await
        .map_err(|e| e.with_request_key(Some(key.clone())))?;
    Ok((Some(key), request_id_of_data(&data), data))
}

async fn execute(
    session: &Session,
    command: &Command,
) -> Result<(Option<String>, Option<String>, Value), CliError> {
    match command {
        Command::Accounts {
            command: AccountsCommand::List,
        } => {
            let (_, data) = session.call("get_accounts", serde_json::json!({})).await?;
            Ok((None, None, data))
        }
        Command::Ordinary { command } => match command {
            OrdinaryCommand::Context => {
                let (_, data) = session
                    .call("get_bookkeeping_context", serde_json::json!({}))
                    .await?;
                Ok((None, None, data))
            }
            OrdinaryCommand::List {
                page,
                page_size,
                account_ids,
                date_from,
                date_to,
            } => {
                let mut args = serde_json::json!({
                    "page": page,
                    "pageSize": page_size,
                });
                if !account_ids.is_empty() {
                    args["accountIds"] = serde_json::json!(account_ids);
                }
                if let Some(from) = date_from {
                    args["dateFrom"] = serde_json::json!(from);
                }
                if let Some(to) = date_to {
                    args["dateTo"] = serde_json::json!(to);
                }
                let (_, data) = session.call("query_ordinary_activities", args).await?;
                Ok((None, None, data))
            }
            OrdinaryCommand::Get { activity_id } => {
                let (_, data) = session
                    .call(
                        "get_ordinary_activity",
                        serde_json::json!({ "activityId": activity_id }),
                    )
                    .await?;
                Ok((None, None, data))
            }
            OrdinaryCommand::Create { input } => {
                let value = read_input(input)?;
                let request_key = require_request_key(&value)?;
                let (_, data) = session
                    .call("create_ordinary_activity", value.clone())
                    .await
                    .map_err(|error| error.with_request_key(Some(request_key.clone())))?;
                Ok((Some(request_key), request_id_of_data(&data), data))
            }
            OrdinaryCommand::Update { input } => {
                let value = read_input(input)?;
                let request_key = require_request_key(&value)?;
                if value
                    .get("expectedVersion")
                    .and_then(Value::as_i64)
                    .is_none()
                {
                    return Err(
                        CliError::usage("the update input must contain expectedVersion")
                            .with_request_key(Some(request_key.clone())),
                    );
                }
                let (_, data) = session
                    .call("update_ordinary_activity", value.clone())
                    .await
                    .map_err(|error| error.with_request_key(Some(request_key.clone())))?;
                Ok((Some(request_key), request_id_of_data(&data), data))
            }
        },
        Command::Holdings {
            command: HoldingsCommand::Get { account_id },
        } => {
            let (_, data) = session
                .call(
                    "get_investment_account_state",
                    serde_json::json!({"accountId":account_id}),
                )
                .await?;
            Ok((None, None, data))
        }
        Command::Holdings { command } => {
            let (tool, input) = match command {
                HoldingsCommand::Patch { input } => ("patch_holdings_snapshot", input),
                HoldingsCommand::Trade { input } => ("record_manual_trade", input),
                HoldingsCommand::Get { .. } => unreachable!(),
            };
            execute_portfolio_write(session, tool, input).await
        }
        Command::Transfers {
            command: TransfersCommand::Create { input },
        } => execute_portfolio_write(session, "create_internal_cash_transfer", input).await,
        Command::Pending { command } => match command {
            PendingCommand::List { include_resolved } => {
                let (_, data) = session
                    .call(
                        "list_pending_activities",
                        serde_json::json!({ "includeResolved": include_resolved }),
                    )
                    .await?;
                Ok((None, None, data))
            }
            PendingCommand::Create { input } => {
                let value = read_input(input)?;
                let request_key = require_request_key(&value)?;
                let (_, data) = session
                    .call("create_pending_activity", value.clone())
                    .await
                    .map_err(|error| error.with_request_key(Some(request_key.clone())))?;
                Ok((Some(request_key), request_id_of_data(&data), data))
            }
        },
        Command::Service { .. } => unreachable!("status returned before connecting"),
    }
}

fn request_id_of_data(data: &Value) -> Option<String> {
    data.get("result")
        .and_then(|result| result.get("requestId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

async fn service_status(cli: &Cli) -> Result<Outcome, CliError> {
    let path = discovery::lock_path(&cli.app_id)?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Outcome {
                ledger_id: None,
                request_key: None,
                request_id: None,
                data: serde_json::json!({
                    "state": "unavailable",
                    "lockFile": path.to_string_lossy(),
                }),
            })
        }
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot read {}: {error}",
                path.display()
            )))
        }
    };
    let lock: discovery::LockFile = serde_json::from_str(&raw).map_err(|error| {
        CliError::unavailable(format!(
            "{} is not a valid discovery file: {error}",
            path.display()
        ))
    })?;
    let socket = std::path::PathBuf::from(&lock.socket_path);
    let mut state = lock.status.clone();
    let mut probe_error = None;
    if matches!(state.as_str(), "ready" | "locked") {
        let discovery = discovery::Discovery {
            lock: lock.clone(),
            socket: socket.clone(),
        };
        match discovery::validate_socket(&socket) {
            Ok(()) => {
                if let Err(error) = crate::client::probe(&discovery).await {
                    state = "unavailable".to_string();
                    probe_error = Some(error.message);
                }
            }
            Err(error) => {
                state = "unavailable".to_string();
                probe_error = Some(error.message);
            }
        }
    }
    Ok(Outcome {
        ledger_id: lock.active_ledger_id.clone(),
        request_key: None,
        request_id: None,
        data: serde_json::json!({
            "state": state,
            "reportedState": lock.status,
            "instanceId": lock.instance_id,
            "port": lock.port,
            "pid": lock.pid,
            "startedAt": lock.started_at,
            "socketPath": lock.socket_path,
            "lockFile": path.to_string_lossy(),
            "probeError": probe_error,
        }),
    })
}

pub fn success_exit() -> u8 {
    EXIT_OK
}
