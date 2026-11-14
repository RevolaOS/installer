// SPDX-License-Identifier: GPL-3.0-only
use super::{InstallState, Result};
use revola_bundle::assembly::{Artifacts, BootloaderAction};
use revola_device::{Identity, NorDump, SysCfg, NOR_SIZE};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::ipc::Channel;

#[derive(Clone, Serialize)]
pub struct InstallEvent {
    stage: String,
    detail: String,
    /// Progress of the current step, for the journal.
    completed: Option<u64>,
    total: Option<u64>,
    /// Share of the whole operation done, 0..1, never decreasing.
    progress: f64,
    elapsed_ms: u64,
    cancellable: bool,
    diagnostic: bool,
}

/// Phases in order with their typical seconds on a 6G over USB 2, measured
/// with `revola nor-dump`, `revola decrypt` and the upload reports. A phase
/// that will not run has weight 0 and leaves the total.
const PHASES: [(&str, u32); 8] = [
    ("checkBootloader", 15),
    ("storage", 5),
    ("backup", 148),
    ("decrypt", 801),
    ("assemble", 2),
    ("upload", 82),
    ("removeFiles", 5),
    ("handoff", 3),
];

fn phase_index(name: &str) -> usize {
    PHASES
        .iter()
        .position(|(phase, _)| *phase == name)
        .expect("known phase")
}

/// The phase a stage key belongs to.
fn phase_of(stage: &str) -> Option<usize> {
    let phase = match stage {
        "uploadLoader" | "uploadFirmware" | "uploadExtensions" | "uploadThemes"
        | "uploadPackages" => "upload",
        "bootloader" | "restart" => "handoff",
        other => other,
    };
    PHASES.iter().position(|(name, _)| *name == phase)
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum InstallOutcome {
    InstallerSent,
    FilesUpdated,
    UninstallerSent,
    FilesRemoved,
}

#[derive(Serialize)]
pub struct InstallResult {
    backup_path: String,
    outcome: InstallOutcome,
    operation: Option<&'static str>,
    /// Paths an uninstall could not delete from the iPod.
    leftovers: Vec<&'static str>,
}

pub(super) struct Job<'a> {
    pub(super) directory: PathBuf,
    device_directory: PathBuf,
    state: &'a InstallState,
    channel: Channel<InstallEvent>,
    journal: File,
    stage: String,
    log_error: Option<String>,
    cancellable: bool,
    operation: Option<&'static str>,
    bootloader_action: Option<BootloaderAction>,
    pub(super) leftovers: Vec<&'static str>,
    started: Instant,
    weights: [u32; PHASES.len()],
    phase: usize,
    fraction: f64,
    reported: f64,
}

impl<'a> Job<'a> {
    pub(super) fn new(
        state: &'a InstallState,
        channel: Channel<InstallEvent>,
        directory: PathBuf,
        operation: &str,
    ) -> Result<Self> {
        let device_directory = directory
            .parent()
            .ok_or("Missing device backup directory")?
            .to_owned();
        let journal = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("events.jsonl"))?;
        let mut weights = PHASES.map(|(_, seconds)| seconds);
        let skipped: &[&str] = if operation == "uninstall" {
            &["storage", "decrypt", "assemble", "upload"]
        } else {
            &["removeFiles"]
        };
        for phase in skipped {
            weights[phase_index(phase)] = 0;
        }
        Ok(Self {
            directory,
            device_directory,
            state,
            channel,
            journal,
            stage: "prepare".into(),
            log_error: None,
            cancellable: true,
            operation: None,
            bootloader_action: None,
            leftovers: Vec::new(),
            started: Instant::now(),
            weights,
            phase: 0,
            fraction: 0.0,
            reported: 0.0,
        })
    }

    /// `cached` says whether the decrypted firmware and loader are already on disk.
    pub(super) fn plan(&mut self, action: BootloaderAction, cached: (bool, bool)) -> Result<()> {
        let operation = match action {
            BootloaderAction::Install => "install",
            BootloaderAction::Keep | BootloaderAction::Upgrade => "update",
        };
        self.operation = Some(operation);
        self.bootloader_action = Some(action);
        if action == BootloaderAction::Keep && self.has_nor_snapshot() {
            self.weights[phase_index("backup")] = 0;
        }
        // The loader is a ninth of the firmware's bytes and decrypts at the same rate.
        let decrypt = &mut self.weights[phase_index("decrypt")];
        *decrypt = match cached {
            (true, true) => 0,
            (true, false) => *decrypt / 10,
            (false, true) => *decrypt - *decrypt / 10,
            (false, false) => *decrypt,
        };
        self.save_json(
            "plan.json",
            &serde_json::json!({
                "operation": operation,
                "bootloader_action": action,
            }),
        )?;
        self.emit(
            match action {
                BootloaderAction::Keep => {
                    "UPDATE: bootloader matches package; updating disk files only"
                }
                BootloaderAction::Install => {
                    "INSTALL: Apple bootloader detected; installing RevolaOS and dual boot"
                }
                BootloaderAction::Upgrade => {
                    "UPDATE: bootloader differs from package; installing new bootloader after a fresh NOR backup"
                }
            },
            None,
        );
        Ok(())
    }

    pub(super) fn plan_uninstall(&mut self, apple_bootloader: bool) -> Result<()> {
        self.operation = Some("uninstall");
        if apple_bootloader {
            self.weights[phase_index("backup")] = 0;
        }
        self.save_json(
            "plan.json",
            &serde_json::json!({"operation": "uninstall", "apple_bootloader": apple_bootloader}),
        )?;
        self.emit(
            if apple_bootloader {
                "UNINSTALL: Apple bootloader detected; removing RevolaOS files only"
            } else {
                "UNINSTALL: removing RevolaOS files, then restoring Apple's bootloader"
            },
            None,
        );
        Ok(())
    }

    pub(super) fn emit(&mut self, detail: impl Into<String>, progress: Option<(u64, u64)>) {
        self.emit_event(detail.into(), progress, false);
    }

    pub(super) fn log(&mut self, detail: impl Into<String>) {
        self.emit_event(detail.into(), None, true);
    }

    fn emit_event(&mut self, detail: String, progress: Option<(u64, u64)>, diagnostic: bool) {
        // A phase that regains its weight grows the total; the bar still never moves back.
        self.reported = self.reported.max(self.overall());
        let event = InstallEvent {
            stage: self.stage.clone(),
            detail,
            completed: progress.map(|p| p.0),
            total: progress.map(|p| p.1),
            progress: self.reported,
            elapsed_ms: self.started.elapsed().as_millis() as u64,
            cancellable: self.cancellable,
            diagnostic,
        };
        let saved = serde_json::to_writer(&mut self.journal, &event)
            .map_err(std::io::Error::other)
            .and_then(|()| self.journal.write_all(b"\n"));
        if let Err(error) = saved {
            self.log_error = Some(error.to_string());
        }
        let _ = self.channel.send(event);
    }

    fn check(&self) -> Result<()> {
        if let Some(error) = &self.log_error {
            return Err(error.clone().into());
        }
        if self
            .state
            .cancellation
            .lock()
            .map_err(|e| e.to_string())?
            .requested
        {
            return Err("Installation cancelled.".into());
        }
        Ok(())
    }

    pub(super) fn stage(&mut self, stage: &str, detail: &str) -> Result<()> {
        self.check()?;
        self.stage = stage.into();
        if let Some(phase) = phase_of(stage).filter(|&phase| phase > self.phase) {
            self.phase = phase;
            self.fraction = 0.0;
        }
        self.emit(detail, None);
        self.journal.sync_all()?;
        self.check()
    }

    /// Journals a step's own count without moving the bar.
    pub(super) fn progress(
        &mut self,
        detail: &str,
        completed: usize,
        total: usize,
    ) -> revola_device::Result<()> {
        self.emit(detail, Some((completed as u64, total as u64)));
        self.check()
            .map_err(|e| revola_device::Error::Invalid(e.to_string()))
    }

    /// Like `progress`, and moves the phase through `band` (start, end) of its share.
    pub(super) fn progress_in(
        &mut self,
        detail: &str,
        band: (f64, f64),
        completed: usize,
        total: usize,
    ) -> revola_device::Result<()> {
        if total > 0 {
            let (start, end) = band;
            self.set_fraction(start + (end - start) * completed as f64 / total as f64);
        }
        self.progress(detail, completed, total)
    }

    /// How far the current phase is, 0..1. A phase planned as skipped that
    /// reports progress anyway gets its default weight back.
    pub(super) fn set_fraction(&mut self, fraction: f64) {
        if self.weights[self.phase] == 0 {
            self.weights[self.phase] = PHASES[self.phase].1;
        }
        self.fraction = self.fraction.max(fraction.clamp(0.0, 1.0));
    }

    pub(super) fn set_weight(&mut self, phase: &str, seconds: u32) {
        self.weights[phase_index(phase)] = seconds;
    }

    fn overall(&self) -> f64 {
        let total: u32 = self.weights.iter().sum();
        if total == 0 {
            return 0.0;
        }
        let done: u32 = self.weights[..self.phase].iter().sum();
        (done as f64 + self.weights[self.phase] as f64 * self.fraction) / total as f64
    }

    /// Whether a complete NOR backup exists for this iPod. `reuse_nor` verifies it.
    fn has_nor_snapshot(&self) -> bool {
        let Ok(entries) = fs::read_dir(self.device_directory.join("nor")) else {
            return false;
        };
        entries.flatten().any(|entry| {
            fs::metadata(entry.path().join("nor.bin")).is_ok_and(|m| m.len() == NOR_SIZE as u64)
        })
    }

    pub(super) fn save_json(&self, name: &str, value: &impl Serialize) -> Result<()> {
        save_json(&self.directory.join(name), value)
    }

    pub(super) fn save_nor(&self, nor: &NorDump, syscfg_size: usize) -> Result<()> {
        let snapshot = self.device_directory.join("nor").join(&nor.report.sha256);
        fs::create_dir_all(&snapshot)?;
        save_or_verify(&snapshot.join("nor.bin"), &nor.bytes)?;
        save_or_verify(&snapshot.join("syscfg.bin"), &nor.bytes[..syscfg_size])?;
        self.save_json(
            "nor.json",
            &serde_json::json!({
                "backup": format!("../nor/{}/nor.bin", nor.report.sha256),
                "report": nor.report,
                "reused": false,
            }),
        )
    }

    pub(super) fn reuse_nor(&mut self, identity: &Identity) -> Result<Option<Vec<u8>>> {
        let snapshots = self.device_directory.join("nor");
        let entries = match fs::read_dir(&snapshots) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut paths = entries
            .map(|entry| entry.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        for path in paths {
            let Some(hash) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let file = path.join("nor.bin");
            let valid = (|| -> Result<Vec<u8>> {
                if fs::metadata(&file)?.len() != NOR_SIZE as u64 {
                    return Err("Invalid backup size".into());
                }
                let bytes = fs::read(&file)?;
                if revola_bundle::sha256(&bytes) != hash
                    || SysCfg::parse(&bytes)?.identity()? != *identity
                {
                    return Err("Backup checksum or device identity mismatch".into());
                }
                Ok(bytes)
            })();
            match valid {
                Ok(bytes) => {
                    self.save_json(
                        "nor.json",
                        &serde_json::json!({
                            "backup": format!("../nor/{hash}/nor.bin"),
                            "reused": true,
                            "sha256": hash,
                            "identity": identity,
                        }),
                    )?;
                    self.emit(
                        format!(
                            "Using verified backup for {} ({})",
                            identity.model, identity.serial
                        ),
                        None,
                    );
                    self.log(format!("Saved NOR backup: {}", file.display()));
                    return Ok(Some(bytes));
                }
                Err(error) => self.log(format!(
                    "Ignoring unusable NOR backup {}: {error}",
                    file.display()
                )),
            }
        }
        Ok(None)
    }

    pub(super) fn save_firmware(&self, artifacts: &Artifacts) -> Result<()> {
        let staged = self.directory.join("firmware");
        let report = artifacts.write(&staged)?;
        let latest = self.device_directory.join("firmware");
        match fs::remove_dir_all(&latest) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::rename(staged, latest)?;
        self.save_json("assembly.json", &report)
    }

    pub(super) fn begin_bootloader(&mut self) -> Result<()> {
        self.begin_handoff("bootloader", "Preparing bootloader installer")
    }

    pub(super) fn begin_bootloader_removal(&mut self) -> Result<()> {
        self.begin_handoff("bootloader", "Preparing bootloader uninstaller")
    }

    pub(super) fn begin_restart(&mut self) -> Result<()> {
        self.begin_handoff("restart", "Restarting iPod")
    }

    fn begin_handoff(&mut self, stage: &str, detail: &str) -> Result<()> {
        self.stage(stage, detail)?;
        {
            let mut cancellation = self.state.cancellation.lock().map_err(|e| e.to_string())?;
            if cancellation.requested {
                return Err("Installation cancelled.".into());
            }
            cancellation.allowed = false;
        }
        self.cancellable = false;
        self.emit(detail, None);
        self.journal.sync_all()?;
        self.check()
    }

    pub(super) fn finish(mut self, outcome: Result<InstallOutcome>) -> Result<InstallResult> {
        match outcome {
            Ok(outcome) => {
                self.phase = PHASES.len() - 1;
                self.fraction = 1.0;
                self.log(match outcome {
                    InstallOutcome::FilesUpdated => "UPDATE COMPLETE: disk files verified; NOR unchanged; restart requested; boot not verified",
                    InstallOutcome::InstallerSent => "BOOTLOADER HANDOFF: disk files verified; NOR installer sent; NOR completion not read back",
                    InstallOutcome::UninstallerSent => "UNINSTALL HANDOFF: disk files removed; NOR uninstaller sent; NOR completion not read back",
                    InstallOutcome::FilesRemoved => "UNINSTALL COMPLETE: disk files removed; NOR unchanged; restart requested; boot not verified",
                });
                if !self.leftovers.is_empty() {
                    self.log(format!("Left on the iPod: {}", self.leftovers.join(", ")));
                }
                save_json(
                    &self.directory.join("result.json"),
                    &serde_json::json!({"state": outcome, "operation": self.operation, "bootloader_action": self.bootloader_action, "leftovers": self.leftovers}),
                )?;
                self.journal.sync_all()?;
                Ok(InstallResult {
                    backup_path: self.directory.to_string_lossy().into_owned(),
                    outcome,
                    operation: self.operation,
                    leftovers: self.leftovers.clone(),
                })
            }
            Err(error) => {
                let detail = format!(
                    "{} failed during {}: {error} Backups and logs: {}. Re-enter DFU before retrying.",
                    self.operation.unwrap_or("install/update").to_uppercase(),
                    self.stage,
                    self.directory.display()
                );
                self.emit(&detail, None);
                let _ = self.journal.sync_all();
                let _ = save_json(
                    &self.directory.join("result.json"),
                    &serde_json::json!({"state": "failed", "operation": self.operation, "bootloader_action": self.bootloader_action, "stage": self.stage, "detail": detail}),
                );
                if let Ok(mut directory) = self.state.failed_directory.lock() {
                    *directory = Some(self.directory.clone());
                }
                Err(detail.into())
            }
        }
    }
}

pub(super) fn backup_directory(data: &Path, hardware_id: &[u8]) -> Result<PathBuf> {
    let device_key: String = hardware_id.iter().map(|b| format!("{b:02x}")).collect();
    let backups = data.join("backups").join(device_key);
    fs::create_dir_all(&backups)?;
    let directory = backups.join(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_nanos()
            .to_string(),
    );
    fs::create_dir(&directory)?;
    Ok(directory)
}

pub(super) fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if fs::read(path)? != bytes {
        return Err("Saved file readback differs from its source.".into());
    }
    Ok(())
}

fn save_or_verify(path: &Path, bytes: &[u8]) -> Result<()> {
    match fs::read(path) {
        Ok(saved) if saved == bytes => Ok(()),
        Ok(_) => Err(format!(
            "Existing backup does not match the verified device data: {}",
            path.display()
        )
        .into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => save(path, bytes),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn save_json(path: &Path, value: &impl Serialize) -> Result<()> {
    save(path, &serde_json::to_vec_pretty(value)?)
}

#[cfg(test)]
mod tests;
