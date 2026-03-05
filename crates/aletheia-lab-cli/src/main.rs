use aletheia_core::{InputButton, InputEvent, InputState, ReplayLog, RunDigest};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Parser)]
#[command(
    name = "aletheia-lab",
    about = "Aletheia deterministic compatibility lab"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run a deterministic smoke execution and emit JSON results.
    Smoke {
        #[arg(value_enum)]
        system: SystemArg,
        #[arg(long, default_value_t = 1024)]
        cycles: u64,
        #[arg(long)]
        replay: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SystemArg {
    GbDmg,
    Nes,
}

#[derive(Debug, Serialize)]
struct SmokeReport {
    report_schema_version: u16,
    digest: RunDigest,
}

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Determinism(#[from] aletheia_core::DeterminismError),
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), CliError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Smoke {
            system,
            cycles,
            replay,
            output,
        } => {
            let replay_log = load_replay(replay)?;
            let digest = match system {
                SystemArg::GbDmg => aletheia_gb::smoke_digest(cycles, &replay_log)?,
                SystemArg::Nes => aletheia_nes::smoke_digest(cycles, &replay_log)?,
            };

            let report = SmokeReport {
                report_schema_version: 1,
                digest,
            };
            let json = serde_json::to_string_pretty(&report)?;

            if let Some(output_path) = output {
                if let Some(parent) = output_path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)?;
                    }
                }
                fs::write(output_path, format!("{json}\n"))?;
            } else {
                println!("{json}");
            }
        }
    }

    Ok(())
}

fn load_replay(path: Option<PathBuf>) -> Result<ReplayLog, CliError> {
    match path {
        Some(path) => {
            let raw = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&raw)?)
        }
        None => Ok(default_replay_fixture()),
    }
}

fn default_replay_fixture() -> ReplayLog {
    ReplayLog::from(vec![
        InputEvent {
            cycle: 32,
            port: 0,
            button: InputButton::Start,
            state: InputState::Pressed,
        },
        InputEvent {
            cycle: 128,
            port: 0,
            button: InputButton::A,
            state: InputState::Pressed,
        },
        InputEvent {
            cycle: 129,
            port: 0,
            button: InputButton::A,
            state: InputState::Released,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fixture_is_versioned() {
        let replay = default_replay_fixture();
        assert_eq!(replay.version, ReplayLog::CURRENT_VERSION);
        assert_eq!(replay.events.len(), 3);
    }
}
