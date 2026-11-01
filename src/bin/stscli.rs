// `stscli`: terminal client for a running Statesu server.
// Also included by the legacy statesu-cli entrypoint.

use std::process::ExitCode;

use clap::Parser;
use statesu::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("stscli: cannot start async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(cli.run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("stscli: {err}");
            err.exit_code()
        }
    }
}
