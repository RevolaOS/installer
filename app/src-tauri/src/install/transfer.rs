// SPDX-License-Identifier: GPL-3.0-only
use super::{
    job::{InstallOutcome, Job},
    Result,
};
use revola_bundle::{
    assembly::{self, Artifacts},
    VerifiedBundle,
};
use revola_device::{Session, UploadFile, UploadHelper, UploadOptions};
use std::io::Cursor;

/// Each upload costs about three seconds of fixed work plus a second per MB
/// (upload reports), so a file weighs its bytes plus this many.
const FILE_OVERHEAD_BYTES: u64 = 3 << 20;
const BYTES_PER_SECOND: u64 = 1 << 20;

/// Share of the upload phase spent hashing every file before the helper launches.
const HASH_SHARE: f64 = 0.1;

/// Where each transfer sub-stage lies within a file's share, as (start, end).
fn band(stage: &str) -> Option<(f64, f64)> {
    Some(match stage {
        "upload" => (0.0, 0.55),
        "write" => (0.55, 0.9),
        "verify" => (0.9, 1.0),
        "complete" => (1.0, 1.0),
        _ => return None,
    })
}

pub(super) fn inspect_storage(
    session: Session,
    helper: &UploadHelper,
    bundle: &VerifiedBundle,
    job: &mut Job<'_>,
) -> Result<Session> {
    job.stage("storage", "Checking storage")?;
    let required = assembly::disk_bytes(bundle)?;
    let directory = job.directory.clone();
    let (returned, storage) =
        session.inspect_storage_with_diagnostics(helper, required, Some(&directory), |detail| {
            job.log(detail)
        })?;

    job.save_json("storage.json", &storage)?;
    job.emit(
        format!(
            "{} bytes free; companion present: {}; firmware present: {}",
            storage.free_bytes, storage.companion_exists, storage.firmware_exists
        ),
        None,
    );

    Ok(returned)
}

/// One file to upload: the name of its report, the stage shown while it uploads,
/// its name in progress messages, its device path and its bytes.
pub(super) struct Upload<'a> {
    pub(super) report: String,
    pub(super) stage: &'static str,
    pub(super) name: String,
    pub(super) destination: String,
    pub(super) bytes: &'a [u8],
}

/// The uploads in order: both images, then the extensions, the themes and the game
/// and app packages. Extension files follow the core so a failed upload never leaves
/// a core that expects extensions which are not there yet.
pub(super) fn uploads(artifacts: &Artifacts) -> Vec<Upload<'_>> {
    let mut files = vec![
        Upload {
            report: "uploadLoader".into(),
            stage: "uploadLoader",
            name: "companion loader".into(),
            destination: "/Revola/Firmware/loader.bin".into(),
            bytes: &artifacts.companion,
        },
        Upload {
            report: "uploadFirmware".into(),
            stage: "uploadFirmware",
            name: "RevolaOS".into(),
            destination: "/Revola/Firmware/revola.bin".into(),
            bytes: &artifacts.firmware,
        },
    ];
    for (prefix, stage, group) in [
        ("uploadExtension", "uploadExtensions", &artifacts.extensions),
        ("uploadTheme", "uploadThemes", &artifacts.themes),
        ("uploadPackage", "uploadPackages", &artifacts.packages),
    ] {
        files.extend(group.iter().enumerate().map(|(index, file)| Upload {
            report: format!("{prefix}{index}"),
            stage,
            name: file.id.clone(),
            destination: format!("/{}", file.path),
            bytes: &file.bytes,
        }));
    }
    files
}

pub(super) fn install(
    session: Session,
    helper: &UploadHelper,
    artifacts: &Artifacts,
    action: assembly::BootloaderAction,
    job: &mut Job<'_>,
) -> Result<InstallOutcome> {
    let uploads = uploads(artifacts);
    let mut cursors: Vec<Cursor<&[u8]>> = uploads.iter().map(|u| Cursor::new(u.bytes)).collect();
    let mut files: Vec<UploadFile<'_>> = cursors
        .iter_mut()
        .zip(&uploads)
        .map(|(input, upload)| UploadFile {
            input,
            options: UploadOptions {
                destination: upload.destination.clone(),
                overwrite: true,
            },
        })
        .collect();
    let weights: Vec<u64> = uploads
        .iter()
        .map(|u| FILE_OVERHEAD_BYTES + u.bytes.len() as u64)
        .collect();
    let total_weight: u64 = weights.iter().sum();
    job.set_weight("upload", (total_weight / BYTES_PER_SECOND) as u32);
    // Hashing runs over every file before the launch, so the upload phase
    // starts here and keeps its stage until the first file launches.
    job.stage(uploads[0].stage, &format!("Checking {}", uploads[0].name))?;
    let log_job = std::cell::RefCell::new(&mut *job);
    let mut current = None;
    let uploaded = session.upload_files(
        helper,
        &mut files,
        |p| {
            let Upload { stage, name, .. } = &uploads[p.file];
            let mut job = log_job.borrow_mut();
            if p.stage != "hash" && current != Some(p.file) {
                current = Some(p.file);
                job.stage(stage, &format!("Preparing {name}"))
                    .map_err(|e| revola_device::Error::Invalid(e.to_string()))?;
            }
            if p.total > 0 {
                let before: u64 = weights[..p.file].iter().sum();
                let through = |within: f64| {
                    (before as f64 + weights[p.file] as f64 * within) / total_weight as f64
                };
                let count = p.completed as f64 / p.total as f64;
                if p.stage == "hash" {
                    job.set_fraction(HASH_SHARE * through(count));
                } else if let Some((start, end)) = band(p.stage) {
                    let within = start + (end - start) * count;
                    job.set_fraction(HASH_SHARE + (1.0 - HASH_SHARE) * through(within));
                }
            }
            let detail = match p.stage {
                "hash" => format!("Checking {name}"),
                "launch" => "Starting transfer helper".into(),
                "upload" => format!("Uploading {name}"),
                "write" => format!("Writing {name}"),
                "verify" => format!("Verifying {name}"),
                "dfu-return" => "Reconnecting to iPod".into(),
                "complete" => format!("Verified {name}"),
                _ => format!("Preparing {name}"),
            };
            let progress = match p.stage {
                "hash" | "upload" | "write" | "verify" | "complete" => Some((p.completed, p.total)),
                _ => None,
            };
            job.emit(detail, progress);
            Ok(())
        },
        |detail| log_job.borrow_mut().log(detail),
    );
    let (session, reports) = match uploaded {
        Ok(uploaded) => (uploaded.session, uploaded.reports),
        Err(failure) => {
            for (upload, report) in uploads.iter().zip(&failure.reports) {
                if let Err(e) = job.save_json(&format!("{}.json", upload.report), report) {
                    job.log(format!("Could not save {}.json: {e}", upload.report));
                }
            }
            return Err(failure.into());
        }
    };
    for (upload, report) in uploads.iter().zip(&reports) {
        job.save_json(&format!("{}.json", upload.report), report)?;
    }
    if action == assembly::BootloaderAction::Keep {
        job.begin_restart()?;
        session.restart()?;
        job.emit(
            "UPDATE: disk files verified; restart requested; NOR unchanged",
            None,
        );
        return Ok(InstallOutcome::FilesUpdated);
    }
    job.begin_bootloader()?;
    session.install_bootloader(&artifacts.nor_installer, |done, total| {
        job.emit(
            "Sending bootloader installer",
            Some((done as u64, total as u64)),
        )
    })?;
    job.emit("Bootloader installer sent", None);
    Ok(InstallOutcome::InstallerSent)
}
