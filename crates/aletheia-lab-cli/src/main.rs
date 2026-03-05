use aletheia_core::{
    InputButton, InputEvent, InputState, ReplayLog, RomError, RomFormat, RomImage, RunDigest,
    load_rom_image,
};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
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
    /// Run a tiny multi-system suite and emit JSON + HTML summaries.
    Suite {
        #[arg(long, default_value_t = 256)]
        cycles: u64,
        #[arg(long)]
        replay: Option<PathBuf>,
        #[arg(long, default_value = "lab-output/suite")]
        output_dir: PathBuf,
    },
    /// Run a user ROM (`.gb`, `.gbc`, `.nes`, `.gba`) and emit report artifacts.
    RunRom {
        rom: PathBuf,
        #[arg(long, default_value_t = 100_000)]
        cycles: u64,
        #[arg(long)]
        replay: Option<PathBuf>,
        #[arg(long, default_value = "lab-output/run-rom")]
        output_dir: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SystemArg {
    GbDmg,
    Nes,
}

impl SystemArg {
    fn as_label(self) -> &'static str {
        match self {
            Self::GbDmg => "gb-dmg",
            Self::Nes => "nes",
        }
    }
}

#[derive(Debug, Serialize)]
struct SmokeReport {
    report_schema_version: u16,
    digest: RunDigest,
}

#[derive(Debug, Serialize)]
struct SuiteReport {
    report_schema_version: u16,
    cycles: u64,
    entries: Vec<SuiteEntry>,
}

#[derive(Debug, Serialize)]
struct SuiteEntry {
    system: String,
    success: bool,
    digest: Option<RunDigest>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct RunRomReport {
    report_schema_version: u16,
    rom: RomImage,
    cycles: u64,
    replay_events: usize,
    success: bool,
    digest: Option<RunDigest>,
    error: Option<String>,
}

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Determinism(#[from] aletheia_core::DeterminismError),
    #[error("{0}")]
    Rom(#[from] RomError),
    #[error("unsupported or unknown ROM format for '{path}'")]
    UnsupportedRomFormat { path: String },
    #[error("suite completed with {failed} failures")]
    SuiteFailed { failed: usize },
    #[error("ROM execution failed: {0}")]
    RomRunFailed(String),
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
            let digest = run_smoke(system, cycles, &replay_log)?;

            let report = SmokeReport {
                report_schema_version: 1,
                digest,
            };
            let json = serde_json::to_string_pretty(&report)?;

            if let Some(output_path) = output {
                write_text_file(&output_path, &format!("{json}\n"))?;
            } else {
                println!("{json}");
            }
        }
        Commands::Suite {
            cycles,
            replay,
            output_dir,
        } => {
            let replay_log = load_replay(replay)?;
            fs::create_dir_all(&output_dir)?;

            let systems = [SystemArg::GbDmg, SystemArg::Nes];
            let mut entries = Vec::with_capacity(systems.len());
            let mut failed = 0usize;

            for system in systems {
                match run_smoke(system, cycles, &replay_log) {
                    Ok(digest) => entries.push(SuiteEntry {
                        system: system.as_label().to_owned(),
                        success: true,
                        digest: Some(digest),
                        error: None,
                    }),
                    Err(error) => {
                        failed += 1;
                        entries.push(SuiteEntry {
                            system: system.as_label().to_owned(),
                            success: false,
                            digest: None,
                            error: Some(error.to_string()),
                        });
                    }
                }
            }

            let report = SuiteReport {
                report_schema_version: 1,
                cycles,
                entries,
            };
            let json = serde_json::to_string_pretty(&report)?;
            write_text_file(&output_dir.join("summary.json"), &format!("{json}\n"))?;
            write_text_file(
                &output_dir.join("summary.html"),
                &render_suite_html(&report),
            )?;
            write_text_file(
                &output_dir.join("replay.trace.txt"),
                &render_replay_trace(&replay_log),
            )?;

            if failed > 0 {
                return Err(CliError::SuiteFailed { failed });
            }
        }
        Commands::RunRom {
            rom,
            cycles,
            replay,
            output_dir,
        } => {
            fs::create_dir_all(&output_dir)?;
            let replay_log = load_replay(replay)?;
            let rom_image = load_rom_image(&rom)?;

            let digest_result = run_detected_rom(&rom_image, cycles, &replay_log);
            let (success, digest, error) = match digest_result {
                Ok(digest) => (true, Some(digest), None),
                Err(error) => (false, None, Some(error.to_string())),
            };

            let report = RunRomReport {
                report_schema_version: 1,
                rom: rom_image,
                cycles,
                replay_events: replay_log.events.len(),
                success,
                digest,
                error,
            };

            let json = serde_json::to_string_pretty(&report)?;
            write_text_file(&output_dir.join("run.json"), &format!("{json}\n"))?;
            write_text_file(&output_dir.join("run.html"), &render_run_rom_html(&report))?;
            write_text_file(
                &output_dir.join("replay.trace.txt"),
                &render_replay_trace(&replay_log),
            )?;

            if !report.success {
                return Err(CliError::RomRunFailed(
                    report
                        .error
                        .as_deref()
                        .unwrap_or("unknown ROM execution failure")
                        .to_owned(),
                ));
            }
        }
    }

    Ok(())
}

fn run_smoke(
    system: SystemArg,
    cycles: u64,
    replay_log: &ReplayLog,
) -> Result<RunDigest, CliError> {
    let digest = match system {
        SystemArg::GbDmg => aletheia_gb::smoke_digest(cycles, replay_log)?,
        SystemArg::Nes => aletheia_nes::smoke_digest(cycles, replay_log)?,
    };
    Ok(digest)
}

fn run_detected_rom(
    rom: &RomImage,
    cycles: u64,
    replay: &ReplayLog,
) -> Result<RunDigest, CliError> {
    match rom.format {
        RomFormat::Gb | RomFormat::Gbc => aletheia_gb::run_rom_digest(cycles, replay, &rom.bytes)
            .map_err(|error| CliError::RomRunFailed(error.to_string())),
        RomFormat::Nes => aletheia_nes::run_rom_digest(cycles, replay, &rom.bytes)
            .map_err(|error| CliError::RomRunFailed(error.to_string())),
        RomFormat::Gba => aletheia_gba::run_rom_digest(cycles, replay, &rom.bytes)
            .map_err(|error| CliError::RomRunFailed(error.to_string())),
        RomFormat::Unknown => Err(CliError::UnsupportedRomFormat {
            path: rom.path.clone(),
        }),
    }
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

fn write_text_file(path: &Path, content: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, content)?;
    Ok(())
}

fn render_suite_html(report: &SuiteReport) -> String {
    let mut rows = String::new();

    for entry in &report.entries {
        let status = if entry.success { "PASS" } else { "FAIL" };
        let (frame_hash, audio_hash, error) = if let Some(digest) = &entry.digest {
            (digest.frame_hash.as_str(), digest.audio_hash.as_str(), "")
        } else {
            (
                "-",
                "-",
                entry.error.as_deref().unwrap_or("unknown failure"),
            )
        };

        let _ = writeln!(
            rows,
            "<tr><td>{}</td><td>{}</td><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr>",
            entry.system, status, frame_hash, audio_hash, error
        );
    }

    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Aletheia Suite Report</title>\n<style>body{{font-family:Segoe UI,Arial,sans-serif;padding:24px}}table{{border-collapse:collapse;width:100%}}th,td{{border:1px solid #ddd;padding:8px;text-align:left}}th{{background:#f4f4f4}}code{{font-size:12px}}</style>\n</head>\n<body>\n<h1>Aletheia Deterministic Suite</h1>\n<p>Schema v{} | Cycles {}</p>\n<table>\n<thead><tr><th>System</th><th>Status</th><th>Frame Hash</th><th>Audio Hash</th><th>Error</th></tr></thead>\n<tbody>\n{}\n</tbody>\n</table>\n</body>\n</html>\n",
        report.report_schema_version, report.cycles, rows
    )
}

fn render_run_rom_html(report: &RunRomReport) -> String {
    let (status, frame_hash, audio_hash, error) = if let Some(digest) = &report.digest {
        (
            "PASS",
            digest.frame_hash.as_str(),
            digest.audio_hash.as_str(),
            "",
        )
    } else {
        (
            "FAIL",
            "-",
            "-",
            report.error.as_deref().unwrap_or("unknown ROM run failure"),
        )
    };

    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Aletheia ROM Run</title>\n<style>body{{font-family:Segoe UI,Arial,sans-serif;padding:24px}}table{{border-collapse:collapse;width:100%}}th,td{{border:1px solid #ddd;padding:8px;text-align:left}}th{{background:#f4f4f4}}code{{font-size:12px}}</style>\n</head>\n<body>\n<h1>Aletheia ROM Run</h1>\n<p><strong>ROM:</strong> {}</p>\n<p><strong>Format:</strong> {} | <strong>Size:</strong> {} bytes | <strong>Cycles:</strong> {}</p>\n<p><strong>Status:</strong> {}</p>\n<table>\n<thead><tr><th>Frame Hash</th><th>Audio Hash</th><th>Error</th></tr></thead>\n<tbody><tr><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr></tbody>\n</table>\n</body>\n</html>\n",
        report.rom.path,
        report.rom.format.as_label(),
        report.rom.byte_len,
        report.cycles,
        status,
        frame_hash,
        audio_hash,
        error
    )
}

fn render_replay_trace(replay: &ReplayLog) -> String {
    let mut output = String::new();
    output.push_str("# Replay Events\n");
    output.push_str("# cycle,port,button,state\n");
    for event in replay.sorted_events() {
        let _ = writeln!(
            output,
            "{},{},{:?},{:?}",
            event.cycle, event.port, event.button, event.state
        );
    }
    output
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

    #[test]
    fn suite_html_contains_rows_for_each_entry() {
        let report = SuiteReport {
            report_schema_version: 1,
            cycles: 10,
            entries: vec![
                SuiteEntry {
                    system: "gb-dmg".to_owned(),
                    success: true,
                    digest: Some(RunDigest {
                        schema_version: 1,
                        replay_version: 1,
                        system: aletheia_core::SystemId::GbDmg,
                        executed_cycles: 10,
                        applied_events: 0,
                        frame_hash: "abc".to_owned(),
                        audio_hash: "def".to_owned(),
                    }),
                    error: None,
                },
                SuiteEntry {
                    system: "nes".to_owned(),
                    success: false,
                    digest: None,
                    error: Some("boom".to_owned()),
                },
            ],
        };

        let html = render_suite_html(&report);
        assert!(html.contains("gb-dmg"));
        assert!(html.contains("nes"));
        assert!(html.contains("PASS"));
        assert!(html.contains("FAIL"));
    }

    #[test]
    fn run_rom_html_renders_basic_metadata() {
        let report = RunRomReport {
            report_schema_version: 1,
            rom: RomImage {
                path: "C:/tmp/test.gba".to_owned(),
                format: RomFormat::Gba,
                byte_len: 1024,
                blake3: "hash".to_owned(),
                metadata: aletheia_core::RomMetadata::Unknown,
                bytes: vec![],
            },
            cycles: 64,
            replay_events: 0,
            success: true,
            digest: Some(RunDigest {
                schema_version: 1,
                replay_version: 1,
                system: aletheia_core::SystemId::Gba,
                executed_cycles: 64,
                applied_events: 0,
                frame_hash: "ff".to_owned(),
                audio_hash: "aa".to_owned(),
            }),
            error: None,
        };

        let html = render_run_rom_html(&report);
        assert!(html.contains("test.gba"));
        assert!(html.contains("gba"));
        assert!(html.contains("PASS"));
    }

    #[test]
    fn replay_trace_renders_header_and_lines() {
        let replay = default_replay_fixture();
        let trace = render_replay_trace(&replay);
        assert!(trace.contains("# Replay Events"));
        assert!(trace.contains("cycle,port,button,state"));
    }
}
