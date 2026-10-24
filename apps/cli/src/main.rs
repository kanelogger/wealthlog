#![allow(
    clippy::result_large_err,
    reason = "a one-shot CLI returns one structured error per process; boxing it would only add indirection"
)]

mod cli;
mod client;
mod discovery;
mod error;
mod token;

use clap::error::ErrorKind;
use clap::Parser;
use serde_json::Value;
use std::process::ExitCode;

use cli::{Cli, Outcome};
use error::CliError;

fn print_output(
    ok: bool,
    ledger_id: Option<&str>,
    request_key: Option<&str>,
    request_id: Option<&str>,
    data: Value,
    error: Option<&CliError>,
) {
    let value = serde_json::json!({
        "ok": ok,
        "ledgerId": ledger_id,
        "requestKey": request_key,
        "requestId": request_id,
        "data": data,
        "error": error.map(CliError::value),
    });
    println!("{value}");
}

#[tokio::main]
async fn main() -> ExitCode {
    let parsed = match Cli::try_parse() {
        Ok(parsed) => parsed,
        Err(error) => {
            // Help and version remain normal CLI output; argument errors go
            // through the same JSON contract as every other command.
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            let failure = CliError::usage(error.to_string().trim().to_string());
            print_output(false, None, None, None, Value::Null, Some(&failure));
            return ExitCode::from(failure.exit);
        }
    };
    match cli::run(parsed).await {
        Ok(Outcome {
            ledger_id,
            request_key,
            request_id,
            data,
        }) => {
            print_output(
                true,
                ledger_id.as_deref(),
                request_key.as_deref(),
                request_id.as_deref(),
                data,
                None,
            );
            ExitCode::from(cli::success_exit())
        }
        Err(error) => {
            print_output(
                false,
                error.ledger_id.as_deref(),
                error.request_key.as_deref(),
                error.request_id.as_deref(),
                Value::Null,
                Some(&error),
            );
            ExitCode::from(error.exit)
        }
    }
}
