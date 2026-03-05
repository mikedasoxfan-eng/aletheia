use aletheia_core::{
    CheckpointDigest, DeterministicMachine, InputButton, InputEvent, InputState, ReplayLog,
    RomError, RomFormat, RomImage, RunDigest, load_rom_image,
};
use clap::{Parser, Subcommand, ValueEnum};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};
use rodio::{OutputStreamBuilder, Sink, buffer::SamplesBuffer};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
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
        #[arg(long)]
        checkpoint_cycle: Option<u64>,
        #[arg(long, default_value = "lab-output/run-rom")]
        output_dir: PathBuf,
    },
    /// Run a ROM with live frame rendering and audible playback (developer preview).
    PlayRom {
        rom: PathBuf,
        #[arg(long, default_value_t = 60)]
        fps: u32,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
        #[arg(long)]
        cycles_per_frame: Option<u64>,
        #[arg(long)]
        max_cycles: Option<u64>,
        #[arg(long)]
        no_audio: bool,
    },
    /// Run all supported ROM files in a directory tree and emit compatibility artifacts.
    Compat {
        rom_dir: PathBuf,
        #[arg(long, default_value_t = 100_000)]
        cycles: u64,
        #[arg(long)]
        replay: Option<PathBuf>,
        #[arg(long, default_value_t = 1)]
        jobs: usize,
        #[arg(long)]
        timeout_ms: Option<u64>,
        #[arg(long, default_value = "lab-output/compat")]
        output_dir: PathBuf,
    },
    /// Compare Aletheia ROM output against a reference JSON report.
    DiffRom {
        rom: PathBuf,
        #[arg(long)]
        reference_report: Option<PathBuf>,
        #[arg(long)]
        reference_exe: Option<PathBuf>,
        #[arg(long = "reference-arg")]
        reference_args: Vec<String>,
        #[arg(long)]
        reference_output: Option<PathBuf>,
        #[arg(long, default_value_t = 30_000)]
        reference_timeout_ms: u64,
        #[arg(long, default_value_t = 100_000)]
        cycles: u64,
        #[arg(long)]
        replay: Option<PathBuf>,
        #[arg(long, default_value = "lab-output/diff")]
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
    checkpoint_cycle: Option<u64>,
    success: bool,
    digest: Option<RunDigest>,
    checkpoint: Option<CheckpointDigest>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct CompatReport {
    report_schema_version: u16,
    cycles: u64,
    replay_events: usize,
    jobs: usize,
    timeout_ms: Option<u64>,
    total: usize,
    passed: usize,
    failed: usize,
    timed_out: usize,
    entries: Vec<CompatEntry>,
}

#[derive(Debug, Serialize)]
struct CompatEntry {
    rom_path: String,
    rom_format: String,
    success: bool,
    timed_out: bool,
    elapsed_ms: u128,
    digest: Option<RunDigest>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct ReferenceDigest {
    source_path: String,
    frame_hash: String,
    audio_hash: String,
    system: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiffReport {
    report_schema_version: u16,
    rom: RomImage,
    cycles: u64,
    replay_events: usize,
    local_success: bool,
    local_digest: Option<RunDigest>,
    local_error: Option<String>,
    reference: ReferenceDigest,
    reference_run: Option<ReferenceRunReport>,
    frame_match: bool,
    audio_match: bool,
    all_match: bool,
}

#[derive(Debug, Serialize, Clone)]
struct ReferenceRunReport {
    executable: String,
    args: Vec<String>,
    output_path: String,
    timeout_ms: u64,
}

#[derive(Debug, Clone)]
struct ReferenceRunConfig {
    executable: PathBuf,
    args: Vec<String>,
    output: PathBuf,
    timeout_ms: u64,
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
    #[error("compatibility run completed with {failed} failures")]
    CompatFailed { failed: usize },
    #[error("compatibility worker thread panicked")]
    CompatWorkerPanic,
    #[error("ROM execution timed out for '{rom}' after {timeout_ms}ms")]
    RomRunTimeout { rom: String, timeout_ms: u64 },
    #[error("failed to parse reference digest from '{path}'")]
    InvalidReferenceReport { path: String },
    #[error("reference source not provided; use --reference-report or --reference-exe")]
    MissingReferenceSource,
    #[error("reference emulator process failed: {exe} (exit code {code})")]
    ReferenceProcessFailed { exe: String, code: i32 },
    #[error("reference emulator process timed out: {exe} after {timeout_ms}ms")]
    ReferenceProcessTimedOut { exe: String, timeout_ms: u64 },
    #[error("live playback failed: {0}")]
    LivePlayback(String),
    #[error("diff mismatch for ROM '{rom}'")]
    DiffMismatch { rom: String },
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
            checkpoint_cycle,
            output_dir,
        } => {
            fs::create_dir_all(&output_dir)?;
            let replay_log = load_replay(replay)?;
            let rom_image = load_rom_image(&rom)?;

            let (success, digest, checkpoint, error) = match checkpoint_cycle {
                Some(cycle) => {
                    match run_detected_rom_with_checkpoint(&rom_image, cycles, &replay_log, cycle) {
                        Ok(checkpoint_result) => {
                            let success = checkpoint_result.digests_match;
                            let error = if success {
                                None
                            } else {
                                Some(format!(
                                    "checkpoint replay mismatch at cycle {}",
                                    checkpoint_result.checkpoint_cycle
                                ))
                            };
                            (
                                success,
                                Some(checkpoint_result.baseline.clone()),
                                Some(checkpoint_result),
                                error,
                            )
                        }
                        Err(error) => (
                            false,
                            None,
                            None,
                            Some(format!("checkpoint verification failed: {error}")),
                        ),
                    }
                }
                None => match run_detected_rom(&rom_image, cycles, &replay_log) {
                    Ok(digest) => (true, Some(digest), None, None),
                    Err(error) => (false, None, None, Some(error.to_string())),
                },
            };

            let report = RunRomReport {
                report_schema_version: 1,
                rom: rom_image,
                cycles,
                replay_events: replay_log.events.len(),
                checkpoint_cycle,
                success,
                digest,
                checkpoint,
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
        Commands::PlayRom {
            rom,
            fps,
            sample_rate,
            cycles_per_frame,
            max_cycles,
            no_audio,
        } => {
            let rom_image = load_rom_image(&rom)?;
            run_live_playback(
                rom_image,
                fps,
                sample_rate,
                cycles_per_frame,
                max_cycles,
                no_audio,
            )?;
        }
        Commands::Compat {
            rom_dir,
            cycles,
            replay,
            jobs,
            timeout_ms,
            output_dir,
        } => {
            fs::create_dir_all(&output_dir)?;
            let replay_log = load_replay(replay)?;
            let rom_paths = collect_supported_rom_paths(&rom_dir)?;
            let jobs = jobs.max(1);
            let mut entries =
                run_compat_entries(rom_paths, cycles, replay_log.clone(), jobs, timeout_ms)?;
            entries.sort_by(|a, b| a.rom_path.cmp(&b.rom_path));

            let passed = entries.iter().filter(|entry| entry.success).count();
            let failed = entries.len().saturating_sub(passed);
            let timed_out = entries.iter().filter(|entry| entry.timed_out).count();

            let report = CompatReport {
                report_schema_version: 1,
                cycles,
                replay_events: replay_log.events.len(),
                jobs,
                timeout_ms,
                total: entries.len(),
                passed,
                failed,
                timed_out,
                entries,
            };
            let json = serde_json::to_string_pretty(&report)?;
            write_text_file(&output_dir.join("compat.json"), &format!("{json}\n"))?;
            write_text_file(
                &output_dir.join("compat.html"),
                &render_compat_html(&report),
            )?;
            write_text_file(
                &output_dir.join("replay.trace.txt"),
                &render_replay_trace(&replay_log),
            )?;

            if failed > 0 {
                return Err(CliError::CompatFailed { failed });
            }
        }
        Commands::DiffRom {
            rom,
            reference_report,
            reference_exe,
            reference_args,
            reference_output,
            reference_timeout_ms,
            cycles,
            replay,
            output_dir,
        } => {
            fs::create_dir_all(&output_dir)?;
            let replay_log = load_replay(replay)?;
            let rom_image = load_rom_image(&rom)?;
            let (reference, reference_run) = load_reference_for_diff(
                &rom,
                cycles,
                &output_dir,
                reference_report,
                reference_exe,
                reference_args,
                reference_output,
                reference_timeout_ms,
            )?;

            let local_run = run_detected_rom(&rom_image, cycles, &replay_log);
            let (local_success, local_digest, local_error) = match local_run {
                Ok(digest) => (true, Some(digest), None),
                Err(error) => (false, None, Some(error.to_string())),
            };

            let frame_match = local_digest
                .as_ref()
                .map(|digest| digest.frame_hash == reference.frame_hash)
                .unwrap_or(false);
            let audio_match = local_digest
                .as_ref()
                .map(|digest| digest.audio_hash == reference.audio_hash)
                .unwrap_or(false);
            let all_match = local_success && frame_match && audio_match;

            let report = DiffReport {
                report_schema_version: 1,
                rom: rom_image,
                cycles,
                replay_events: replay_log.events.len(),
                local_success,
                local_digest,
                local_error,
                reference,
                reference_run,
                frame_match,
                audio_match,
                all_match,
            };

            let json = serde_json::to_string_pretty(&report)?;
            write_text_file(&output_dir.join("diff.json"), &format!("{json}\n"))?;
            write_text_file(&output_dir.join("diff.html"), &render_diff_html(&report))?;
            write_text_file(
                &output_dir.join("replay.trace.txt"),
                &render_replay_trace(&replay_log),
            )?;

            if !report.all_match {
                return Err(CliError::DiffMismatch {
                    rom: report.rom.path,
                });
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

fn run_detected_rom_with_checkpoint(
    rom: &RomImage,
    cycles: u64,
    replay: &ReplayLog,
    checkpoint_cycle: u64,
) -> Result<CheckpointDigest, CliError> {
    match rom.format {
        RomFormat::Gb | RomFormat::Gbc => aletheia_gb::run_rom_digest_with_checkpoint(
            cycles,
            replay,
            &rom.bytes,
            checkpoint_cycle,
        )
        .map_err(|error| CliError::RomRunFailed(error.to_string())),
        RomFormat::Nes => aletheia_nes::run_rom_digest_with_checkpoint(
            cycles,
            replay,
            &rom.bytes,
            checkpoint_cycle,
        )
        .map_err(|error| CliError::RomRunFailed(error.to_string())),
        RomFormat::Gba => aletheia_gba::run_rom_digest_with_checkpoint(
            cycles,
            replay,
            &rom.bytes,
            checkpoint_cycle,
        )
        .map_err(|error| CliError::RomRunFailed(error.to_string())),
        RomFormat::Unknown => Err(CliError::UnsupportedRomFormat {
            path: rom.path.clone(),
        }),
    }
}

#[derive(Debug, Clone, Copy)]
struct LiveProfile {
    width: usize,
    height: usize,
    cpu_hz: u64,
    default_cycles_per_frame: u64,
    scale: Scale,
    label: &'static str,
}

enum LiveCore {
    Gb(aletheia_gb::DmgCore),
    Nes(aletheia_nes::NesCore),
    Gba(aletheia_gba::GbaCore),
}

impl LiveCore {
    fn reset(&mut self) {
        match self {
            Self::Gb(core) => core.reset(),
            Self::Nes(core) => core.reset(),
            Self::Gba(core) => core.reset(),
        }
    }

    fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16) {
        match self {
            Self::Gb(core) => core.tick(cycle, input_events),
            Self::Nes(core) => core.tick(cycle, input_events),
            Self::Gba(core) => core.tick(cycle, input_events),
        }
    }
}

fn run_live_playback(
    rom: RomImage,
    fps: u32,
    sample_rate: u32,
    cycles_per_frame: Option<u64>,
    max_cycles: Option<u64>,
    no_audio: bool,
) -> Result<(), CliError> {
    let fps = fps.max(1);
    let profile = live_profile_for_format(rom.format)?;
    let cycles_per_frame = cycles_per_frame
        .unwrap_or(profile.default_cycles_per_frame)
        .max(1);
    let mut core = create_live_core(&rom)?;
    core.reset();

    let mut window = Window::new(
        &format!(
            "Aletheia Live [{}] - ESC quit, P pause",
            profile.label.to_uppercase()
        ),
        profile.width,
        profile.height,
        WindowOptions {
            scale: profile.scale,
            resize: false,
            ..WindowOptions::default()
        },
    )
    .map_err(|error| CliError::LivePlayback(error.to_string()))?;
    let frame_budget = Duration::from_secs_f64(1.0 / (fps as f64));
    window.set_target_fps(0);

    let mut frame_buffer = vec![0xFF_00_00_00u32; profile.width * profile.height];
    let mut key_state = HashMap::new();
    let keymap = [
        (Key::Z, InputButton::A),
        (Key::X, InputButton::B),
        (Key::Enter, InputButton::Start),
        (Key::Space, InputButton::Select),
        (Key::Up, InputButton::Up),
        (Key::Down, InputButton::Down),
        (Key::Left, InputButton::Left),
        (Key::Right, InputButton::Right),
    ];

    let mut audio_stream = if no_audio {
        None
    } else {
        let mut stream = OutputStreamBuilder::open_default_stream()
            .map_err(|error| CliError::LivePlayback(error.to_string()))?;
        stream.log_on_drop(false);
        let sink = Sink::connect_new(stream.mixer());
        Some((stream, sink))
    };
    let mut audio_phase_accumulator = 0u64;
    let mut total_cycles = 0u64;
    let mut paused = false;
    let mut frame_counter = 0u64;
    let mut title_update_counter = 0u32;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let frame_start = Instant::now();
        if window.is_key_pressed(Key::P, KeyRepeat::No) {
            paused = !paused;
        }

        if !paused {
            let events = collect_live_input_events(&window, &keymap, &mut key_state, total_cycles);
            let mut queued_audio = Vec::new();
            for offset in 0..cycles_per_frame {
                let cycle = total_cycles + offset;
                let input_events = if offset == 0 {
                    events.as_slice()
                } else {
                    &[] as &[InputEvent]
                };
                let (frame_sample, audio_sample) = core.tick(cycle, input_events);
                let pixel_index = (cycle as usize) % frame_buffer.len();
                frame_buffer[pixel_index] = colorize_live_sample(frame_sample, audio_sample, cycle);

                if audio_stream.is_some() {
                    audio_phase_accumulator += sample_rate as u64;
                    if audio_phase_accumulator >= profile.cpu_hz {
                        audio_phase_accumulator -= profile.cpu_hz;
                        queued_audio.push((audio_sample as f32) / (i16::MAX as f32));
                    }
                }
            }
            total_cycles += cycles_per_frame;
            frame_counter = frame_counter.wrapping_add(1);

            if let Some((_, sink)) = audio_stream.as_mut() {
                if !queued_audio.is_empty() {
                    sink.append(SamplesBuffer::new(1, sample_rate, queued_audio));
                }
            }
        }

        window
            .update_with_buffer(&frame_buffer, profile.width, profile.height)
            .map_err(|error| CliError::LivePlayback(error.to_string()))?;
        title_update_counter = title_update_counter.wrapping_add(1);
        if title_update_counter >= fps {
            let pause_label = if paused { " (paused)" } else { "" };
            window.set_title(&format!(
                "Aletheia Live [{}] cycles={} frames={}{}",
                profile.label.to_uppercase(),
                total_cycles,
                frame_counter,
                pause_label
            ));
            title_update_counter = 0;
        }

        if let Some(max_cycles) = max_cycles {
            if total_cycles >= max_cycles {
                break;
            }
        }

        let elapsed = frame_start.elapsed();
        if elapsed < frame_budget {
            thread::sleep(frame_budget - elapsed);
        }
    }

    if let Some((_, sink)) = audio_stream.as_mut() {
        sink.stop();
    }

    Ok(())
}

fn live_profile_for_format(format: RomFormat) -> Result<LiveProfile, CliError> {
    let profile = match format {
        RomFormat::Gb | RomFormat::Gbc => LiveProfile {
            width: 160,
            height: 144,
            cpu_hz: 4_194_304,
            default_cycles_per_frame: 4_194_304 / 60,
            scale: Scale::X4,
            label: "gb",
        },
        RomFormat::Nes => LiveProfile {
            width: 256,
            height: 240,
            cpu_hz: 1_789_773,
            default_cycles_per_frame: 1_789_773 / 60,
            scale: Scale::X2,
            label: "nes",
        },
        RomFormat::Gba => LiveProfile {
            width: 240,
            height: 160,
            cpu_hz: 16_777_216,
            default_cycles_per_frame: 16_777_216 / 60,
            scale: Scale::X2,
            label: "gba",
        },
        RomFormat::Unknown => {
            return Err(CliError::UnsupportedRomFormat {
                path: "unknown".to_owned(),
            });
        }
    };
    Ok(profile)
}

fn create_live_core(rom: &RomImage) -> Result<LiveCore, CliError> {
    match rom.format {
        RomFormat::Gb | RomFormat::Gbc => {
            let mut core = aletheia_gb::DmgCore::default();
            core.load_rom(&rom.bytes)
                .map_err(|error| CliError::LivePlayback(error.to_string()))?;
            Ok(LiveCore::Gb(core))
        }
        RomFormat::Nes => {
            let mut core = aletheia_nes::NesCore::default();
            core.load_rom(&rom.bytes)
                .map_err(|error| CliError::LivePlayback(error.to_string()))?;
            Ok(LiveCore::Nes(core))
        }
        RomFormat::Gba => {
            let mut core = aletheia_gba::GbaCore::default();
            core.load_rom(&rom.bytes);
            Ok(LiveCore::Gba(core))
        }
        RomFormat::Unknown => Err(CliError::UnsupportedRomFormat {
            path: rom.path.clone(),
        }),
    }
}

fn collect_live_input_events(
    window: &Window,
    keymap: &[(Key, InputButton)],
    key_state: &mut HashMap<Key, bool>,
    cycle: u64,
) -> Vec<InputEvent> {
    let mut events = Vec::new();
    for (key, button) in keymap {
        let now_pressed = window.is_key_down(*key);
        let was_pressed = *key_state.get(key).unwrap_or(&false);
        if now_pressed != was_pressed {
            events.push(InputEvent {
                cycle,
                port: 0,
                button: *button,
                state: if now_pressed {
                    InputState::Pressed
                } else {
                    InputState::Released
                },
            });
            key_state.insert(*key, now_pressed);
        }
    }
    events
}

fn colorize_live_sample(frame_sample: u8, audio_sample: i16, cycle: u64) -> u32 {
    let frame = frame_sample as u32;
    let audio = (((audio_sample as i32) + 32_768) as u32) >> 8;
    let drift = (cycle as u32) & 0xFF;
    let r = frame ^ drift;
    let g = audio ^ (drift.rotate_left(1) & 0xFF);
    let b = (frame ^ audio) & 0xFF;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

fn run_compat_entries(
    rom_paths: Vec<PathBuf>,
    cycles: u64,
    replay_log: ReplayLog,
    jobs: usize,
    timeout_ms: Option<u64>,
) -> Result<Vec<CompatEntry>, CliError> {
    if rom_paths.is_empty() {
        return Ok(Vec::new());
    }

    let chunk_size = ((rom_paths.len() + jobs.saturating_sub(1)) / jobs).max(1);
    let mut entries = Vec::with_capacity(rom_paths.len());

    thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in rom_paths.chunks(chunk_size) {
            let chunk_paths = chunk.to_vec();
            let worker_replay = replay_log.clone();
            handles.push(
                scope.spawn(move || {
                    run_compat_chunk(chunk_paths, cycles, worker_replay, timeout_ms)
                }),
            );
        }

        for handle in handles {
            let mut chunk_entries = handle.join().map_err(|_| CliError::CompatWorkerPanic)?;
            entries.append(&mut chunk_entries);
        }

        Ok::<(), CliError>(())
    })?;

    Ok(entries)
}

fn run_compat_chunk(
    rom_paths: Vec<PathBuf>,
    cycles: u64,
    replay_log: ReplayLog,
    timeout_ms: Option<u64>,
) -> Vec<CompatEntry> {
    rom_paths
        .into_iter()
        .map(|path| run_single_compat_entry(path, cycles, &replay_log, timeout_ms))
        .collect()
}

fn run_single_compat_entry(
    rom_path: PathBuf,
    cycles: u64,
    replay_log: &ReplayLog,
    timeout_ms: Option<u64>,
) -> CompatEntry {
    let started = Instant::now();
    let rom_path_str = rom_path.to_string_lossy().to_string();

    match load_rom_image(&rom_path) {
        Ok(rom_image) => {
            let rom_format = rom_image.format.as_label().to_owned();
            let run_result = match timeout_ms {
                Some(timeout_ms) => run_detected_rom_with_timeout(
                    rom_image.clone(),
                    cycles,
                    replay_log.clone(),
                    timeout_ms,
                ),
                None => run_detected_rom(&rom_image, cycles, replay_log),
            };

            let elapsed_ms = started.elapsed().as_millis();
            match run_result {
                Ok(digest) => CompatEntry {
                    rom_path: rom_path_str,
                    rom_format,
                    success: true,
                    timed_out: false,
                    elapsed_ms,
                    digest: Some(digest),
                    error: None,
                },
                Err(error) => CompatEntry {
                    rom_path: rom_path_str,
                    rom_format,
                    success: false,
                    timed_out: matches!(error, CliError::RomRunTimeout { .. }),
                    elapsed_ms,
                    digest: None,
                    error: Some(error.to_string()),
                },
            }
        }
        Err(error) => CompatEntry {
            rom_path: rom_path_str,
            rom_format: "unknown".to_owned(),
            success: false,
            timed_out: false,
            elapsed_ms: started.elapsed().as_millis(),
            digest: None,
            error: Some(error.to_string()),
        },
    }
}

fn run_detected_rom_with_timeout(
    rom: RomImage,
    cycles: u64,
    replay_log: ReplayLog,
    timeout_ms: u64,
) -> Result<RunDigest, CliError> {
    let rom_path = rom.path.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = run_detected_rom(&rom, cycles, &replay_log).map_err(|error| error.to_string());
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(Ok(digest)) => Ok(digest),
        Ok(Err(error)) => Err(CliError::RomRunFailed(error)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(CliError::RomRunTimeout {
            rom: rom_path,
            timeout_ms,
        }),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(CliError::RomRunFailed("ROM worker disconnected".to_owned()))
        }
    }
}

fn load_reference_for_diff(
    rom_path: &Path,
    cycles: u64,
    output_dir: &Path,
    reference_report: Option<PathBuf>,
    reference_exe: Option<PathBuf>,
    reference_args: Vec<String>,
    reference_output: Option<PathBuf>,
    reference_timeout_ms: u64,
) -> Result<(ReferenceDigest, Option<ReferenceRunReport>), CliError> {
    if let Some(reference_exe) = reference_exe {
        let default_output_dir = output_dir.join("reference");
        let output_path = reference_output.unwrap_or_else(|| default_output_dir.join("run.json"));
        let args = if reference_args.is_empty() {
            vec![
                "run-rom".to_owned(),
                "{rom}".to_owned(),
                "--cycles".to_owned(),
                "{cycles}".to_owned(),
                "--output-dir".to_owned(),
                "{output_dir}".to_owned(),
            ]
        } else {
            reference_args
        };
        let config = ReferenceRunConfig {
            executable: reference_exe,
            args,
            output: output_path,
            timeout_ms: reference_timeout_ms,
        };
        let run_report = run_reference_emulator(&config, rom_path, cycles)?;
        let digest = parse_reference_digest(&config.output)?;
        return Ok((digest, Some(run_report)));
    }

    if let Some(reference_report) = reference_report {
        let digest = parse_reference_digest(&reference_report)?;
        return Ok((digest, None));
    }

    Err(CliError::MissingReferenceSource)
}

fn run_reference_emulator(
    config: &ReferenceRunConfig,
    rom_path: &Path,
    cycles: u64,
) -> Result<ReferenceRunReport, CliError> {
    if let Some(parent) = config.output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let output_dir = config.output.parent().unwrap_or_else(|| Path::new("."));
    let rendered_args = config
        .args
        .iter()
        .map(|arg| render_reference_arg(arg, rom_path, cycles, &config.output, output_dir))
        .collect::<Vec<_>>();

    let mut child = Command::new(&config.executable)
        .args(&rendered_args)
        .spawn()?;
    let started = Instant::now();
    let timeout = Duration::from_millis(config.timeout_ms);
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                break;
            }
            return Err(CliError::ReferenceProcessFailed {
                exe: config.executable.to_string_lossy().to_string(),
                code: status.code().unwrap_or(-1),
            });
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CliError::ReferenceProcessTimedOut {
                exe: config.executable.to_string_lossy().to_string(),
                timeout_ms: config.timeout_ms,
            });
        }
        thread::sleep(Duration::from_millis(10));
    }

    Ok(ReferenceRunReport {
        executable: config.executable.to_string_lossy().to_string(),
        args: rendered_args,
        output_path: config.output.to_string_lossy().to_string(),
        timeout_ms: config.timeout_ms,
    })
}

fn render_reference_arg(
    template: &str,
    rom_path: &Path,
    cycles: u64,
    output_path: &Path,
    output_dir: &Path,
) -> String {
    template
        .replace("{rom}", &rom_path.to_string_lossy())
        .replace("{cycles}", &cycles.to_string())
        .replace("{output}", &output_path.to_string_lossy())
        .replace("{output_dir}", &output_dir.to_string_lossy())
}

fn parse_reference_digest(path: &Path) -> Result<ReferenceDigest, CliError> {
    let raw = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&raw)?;

    if let Some(digest) = extract_digest_from_value(&value) {
        return Ok(ReferenceDigest {
            source_path: path.to_string_lossy().to_string(),
            frame_hash: digest.0,
            audio_hash: digest.1,
            system: digest.2,
        });
    }

    if let Some(entries) = value.get("entries").and_then(Value::as_array) {
        for entry in entries {
            if let Some(digest_node) = entry.get("digest") {
                if let Some((frame, audio, _)) = extract_digest_from_value(digest_node) {
                    let system = entry
                        .get("system")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    return Ok(ReferenceDigest {
                        source_path: path.to_string_lossy().to_string(),
                        frame_hash: frame,
                        audio_hash: audio,
                        system,
                    });
                }
            }
        }
    }

    Err(CliError::InvalidReferenceReport {
        path: path.to_string_lossy().to_string(),
    })
}

fn extract_digest_from_value(value: &Value) -> Option<(String, String, Option<String>)> {
    if let Some(digest) = value.get("digest") {
        return extract_digest_from_value(digest);
    }

    let frame = value.get("frame_hash")?.as_str()?.to_owned();
    let audio = value.get("audio_hash")?.as_str()?.to_owned();
    let system = value
        .get("system")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    Some((frame, audio, system))
}

fn collect_supported_rom_paths(root: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut out = Vec::new();
    collect_supported_rom_paths_impl(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_supported_rom_paths_impl(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), CliError> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_supported_rom_paths_impl(&path, out)?;
        } else if is_supported_rom_extension(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_supported_rom_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "gb" | "gbc" | "nes" | "gba"
            )
        })
        .unwrap_or(false)
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

fn report_style() -> &'static str {
    "body{font-family:'Segoe UI',Tahoma,Arial,sans-serif;margin:0;background:linear-gradient(180deg,#f5f8ff 0%,#eef2fb 100%);color:#16213a}main{max-width:1100px;margin:0 auto;padding:28px}h1{margin:0 0 8px 0;font-size:28px}p.meta{margin:0 0 20px 0;color:#31456e}.card{background:#fff;border:1px solid #d8e1f4;border-radius:12px;box-shadow:0 8px 28px rgba(20,35,70,.08);padding:16px 18px;margin-bottom:16px}table{width:100%;border-collapse:collapse}th,td{padding:10px;border-bottom:1px solid #ebf0fb;text-align:left;vertical-align:top}th{color:#28406d;background:#f7f9ff}tr:hover td{background:#fbfcff}.ok{color:#0b6a3f;font-weight:600}.fail{color:#a01937;font-weight:600}code{font-family:'Cascadia Code','Consolas',monospace;font-size:12px;word-break:break-all}.kv{display:grid;grid-template-columns:180px 1fr;gap:6px 12px}.label{color:#51638d}"
}

fn render_suite_html(report: &SuiteReport) -> String {
    let mut rows = String::new();

    for entry in &report.entries {
        let status = if entry.success {
            "<span class=\"ok\">PASS</span>"
        } else {
            "<span class=\"fail\">FAIL</span>"
        };
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
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Aletheia Suite Report</title>\n<style>{}</style>\n</head>\n<body>\n<main>\n<h1>Aletheia Deterministic Suite</h1>\n<p class=\"meta\">Schema v{} | Cycles {}</p>\n<section class=\"card\">\n<table>\n<thead><tr><th>System</th><th>Status</th><th>Frame Hash</th><th>Audio Hash</th><th>Error</th></tr></thead>\n<tbody>\n{}\n</tbody>\n</table>\n</section>\n</main>\n</body>\n</html>\n",
        report_style(),
        report.report_schema_version,
        report.cycles,
        rows
    )
}

fn render_run_rom_html(report: &RunRomReport) -> String {
    let (status, frame_hash, audio_hash, error) = if let Some(digest) = &report.digest {
        (
            "<span class=\"ok\">PASS</span>",
            digest.frame_hash.as_str(),
            digest.audio_hash.as_str(),
            "",
        )
    } else {
        (
            "<span class=\"fail\">FAIL</span>",
            "-",
            "-",
            report.error.as_deref().unwrap_or("unknown ROM run failure"),
        )
    };
    let checkpoint_summary = match (&report.checkpoint_cycle, &report.checkpoint) {
        (Some(cycle), Some(checkpoint)) => {
            if checkpoint.digests_match {
                format!(
                    "<div class=\"kv\"><div class=\"label\">Checkpoint cycle</div><div>{}</div><div class=\"label\">Checkpoint replay</div><div><span class=\"ok\">MATCH</span></div></div>",
                    cycle
                )
            } else {
                format!(
                    "<div class=\"kv\"><div class=\"label\">Checkpoint cycle</div><div>{}</div><div class=\"label\">Checkpoint replay</div><div><span class=\"fail\">MISMATCH</span></div></div>",
                    cycle
                )
            }
        }
        (Some(cycle), None) => format!(
            "<div class=\"kv\"><div class=\"label\">Checkpoint cycle</div><div>{}</div><div class=\"label\">Checkpoint replay</div><div><span class=\"fail\">FAILED</span></div></div>",
            cycle
        ),
        _ => "<div class=\"kv\"><div class=\"label\">Checkpoint replay</div><div>disabled</div></div>"
            .to_owned(),
    };

    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Aletheia ROM Run</title>\n<style>{}</style>\n</head>\n<body>\n<main>\n<h1>Aletheia ROM Run</h1>\n<p class=\"meta\">Single-ROM deterministic execution summary</p>\n<section class=\"card\">\n<div class=\"kv\">\n<div class=\"label\">ROM path</div><div><code>{}</code></div>\n<div class=\"label\">Format</div><div>{}</div>\n<div class=\"label\">Size</div><div>{} bytes</div>\n<div class=\"label\">Cycles</div><div>{}</div>\n<div class=\"label\">Replay events</div><div>{}</div>\n<div class=\"label\">Status</div><div>{}</div>\n</div>\n</section>\n<section class=\"card\">{}</section>\n<section class=\"card\">\n<table>\n<thead><tr><th>Frame Hash</th><th>Audio Hash</th><th>Error</th></tr></thead>\n<tbody><tr><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr></tbody>\n</table>\n</section>\n</main>\n</body>\n</html>\n",
        report_style(),
        report.rom.path,
        report.rom.format.as_label(),
        report.rom.byte_len,
        report.cycles,
        report.replay_events,
        status,
        checkpoint_summary,
        frame_hash,
        audio_hash,
        error
    )
}

fn render_compat_html(report: &CompatReport) -> String {
    let mut rows = String::new();
    let timeout_label = report
        .timeout_ms
        .map(|timeout| timeout.to_string())
        .unwrap_or_else(|| "none".to_owned());

    for entry in &report.entries {
        let status = if entry.success {
            "<span class=\"ok\">PASS</span>"
        } else {
            "<span class=\"fail\">FAIL</span>"
        };
        let frame_hash = entry
            .digest
            .as_ref()
            .map(|digest| digest.frame_hash.as_str())
            .unwrap_or("-");
        let audio_hash = entry
            .digest
            .as_ref()
            .map(|digest| digest.audio_hash.as_str())
            .unwrap_or("-");
        let error = entry.error.as_deref().unwrap_or("");

        let _ = writeln!(
            rows,
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr>",
            entry.rom_path,
            entry.rom_format,
            status,
            if entry.timed_out { "yes" } else { "no" },
            frame_hash,
            audio_hash,
            if error.is_empty() {
                format!("{} ms", entry.elapsed_ms)
            } else {
                format!("{} ({} ms)", error, entry.elapsed_ms)
            }
        );
    }

    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Aletheia Compatibility Report</title>\n<style>{}</style>\n</head>\n<body>\n<main>\n<h1>Aletheia Compatibility Report</h1>\n<p class=\"meta\">Cycles {} | Replay events {} | Jobs {} | Timeout {}</p>\n<section class=\"card\">\n<div class=\"kv\">\n<div class=\"label\">Total ROMs</div><div>{}</div>\n<div class=\"label\">Passed</div><div>{}</div>\n<div class=\"label\">Failed</div><div>{}</div>\n<div class=\"label\">Timed out</div><div>{}</div>\n</div>\n</section>\n<section class=\"card\">\n<table>\n<thead><tr><th>ROM</th><th>Format</th><th>Status</th><th>Timed Out</th><th>Frame Hash</th><th>Audio Hash</th><th>Error / Elapsed</th></tr></thead>\n<tbody>\n{}\n</tbody>\n</table>\n</section>\n</main>\n</body>\n</html>\n",
        report_style(),
        report.cycles,
        report.replay_events,
        report.jobs,
        timeout_label,
        report.total,
        report.passed,
        report.failed,
        report.timed_out,
        rows
    )
}

fn render_diff_html(report: &DiffReport) -> String {
    let local_frame = report
        .local_digest
        .as_ref()
        .map(|digest| digest.frame_hash.as_str())
        .unwrap_or("-");
    let local_audio = report
        .local_digest
        .as_ref()
        .map(|digest| digest.audio_hash.as_str())
        .unwrap_or("-");
    let status = if report.all_match {
        "<span class=\"ok\">MATCH</span>"
    } else {
        "<span class=\"fail\">MISMATCH</span>"
    };
    let reference_run = report.reference_run.as_ref().map(|run| {
        format!(
            "<section class=\"card\"><div class=\"kv\"><div class=\"label\">Reference executable</div><div><code>{}</code></div><div class=\"label\">Rendered args</div><div><code>{}</code></div><div class=\"label\">Output report</div><div><code>{}</code></div><div class=\"label\">Timeout</div><div>{} ms</div></div></section>",
            run.executable,
            run.args.join(" "),
            run.output_path,
            run.timeout_ms
        )
    }).unwrap_or_default();

    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Aletheia Diff Report</title>\n<style>{}</style>\n</head>\n<body>\n<main>\n<h1>Aletheia Differential Report</h1>\n<p class=\"meta\">ROM <code>{}</code> | Format {} | Result {}</p>\n{}\n<section class=\"card\">\n<table>\n<thead><tr><th>Source</th><th>Frame Hash</th><th>Audio Hash</th></tr></thead>\n<tbody>\n<tr><td>Aletheia</td><td><code>{}</code></td><td><code>{}</code></td></tr>\n<tr><td>Reference (<code>{}</code>)</td><td><code>{}</code></td><td><code>{}</code></td></tr>\n</tbody>\n</table>\n</section>\n<section class=\"card\">\n<div class=\"kv\"><div class=\"label\">Frame Match</div><div>{}</div><div class=\"label\">Audio Match</div><div>{}</div><div class=\"label\">Local Error</div><div>{}</div></div>\n</section>\n</main>\n</body>\n</html>\n",
        report_style(),
        report.rom.path,
        report.rom.format.as_label(),
        status,
        reference_run,
        local_frame,
        local_audio,
        report.reference.source_path,
        report.reference.frame_hash,
        report.reference.audio_hash,
        report.frame_match,
        report.audio_match,
        report.local_error.as_deref().unwrap_or("")
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
            checkpoint_cycle: None,
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
            checkpoint: None,
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

    #[test]
    fn parses_reference_digest_from_aletheia_schema() {
        let value = serde_json::json!({
            "digest": {
                "frame_hash": "a",
                "audio_hash": "b",
                "system": "Nes"
            }
        });
        let parsed = extract_digest_from_value(&value).expect("digest");
        assert_eq!(parsed.0, "a");
        assert_eq!(parsed.1, "b");
        assert_eq!(parsed.2.as_deref(), Some("Nes"));
    }

    #[test]
    fn parses_reference_digest_from_flat_schema() {
        let value = serde_json::json!({
            "frame_hash": "x",
            "audio_hash": "y"
        });
        let parsed = extract_digest_from_value(&value).expect("digest");
        assert_eq!(parsed.0, "x");
        assert_eq!(parsed.1, "y");
    }

    #[test]
    fn supported_extension_check_works() {
        assert!(is_supported_rom_extension(Path::new("a.gb")));
        assert!(is_supported_rom_extension(Path::new("a.GBA")));
        assert!(!is_supported_rom_extension(Path::new("a.txt")));
    }

    #[test]
    fn reference_arg_templates_are_rendered() {
        let rom = Path::new("C:/roms/game.gba");
        let output = Path::new("C:/tmp/ref/run.json");
        let output_dir = Path::new("C:/tmp/ref");
        let rendered = render_reference_arg(
            "run {rom} --cycles {cycles} --out {output} --dir {output_dir}",
            rom,
            1234,
            output,
            output_dir,
        );
        assert!(rendered.contains("C:/roms/game.gba"));
        assert!(rendered.contains("1234"));
        assert!(rendered.contains("C:/tmp/ref/run.json"));
        assert!(rendered.contains("C:/tmp/ref"));
    }

    #[test]
    fn diff_requires_reference_source() {
        let error = load_reference_for_diff(
            Path::new("test.gba"),
            100,
            Path::new("lab-output/diff"),
            None,
            None,
            vec![],
            None,
            2000,
        )
        .expect_err("missing reference source should fail");
        assert!(matches!(error, CliError::MissingReferenceSource));
    }

    #[test]
    fn live_profile_supports_gba() {
        let profile = live_profile_for_format(RomFormat::Gba).expect("gba live profile");
        assert_eq!(profile.width, 240);
        assert_eq!(profile.height, 160);
        assert_eq!(profile.cpu_hz, 16_777_216);
    }

    #[test]
    fn live_colorizer_is_stable_for_same_inputs() {
        let a = colorize_live_sample(0x12, -300, 42);
        let b = colorize_live_sample(0x12, -300, 42);
        assert_eq!(a, b);
    }
}
