// SPDX-License-Identifier: GPL-3.0-only
use super::{
    job::{InstallOutcome, Job},
    Result,
};
use revola_bundle::{
    assembly::{self, Artifacts},
    VerifiedBundle,
};
use revola_device::{Session, UploadHelper, UploadOptions};
use std::io::Cursor;

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

pub(super) fn install(
    mut session: Session,
    helper: &UploadHelper,
    artifacts: &Artifacts,
    action: assembly::BootloaderAction,
    job: &mut Job<'_>,
) -> Result<InstallOutcome> {
    let mut files = vec![
        (
            "uploadLoader".to_owned(),
            "companion loader".to_owned(),
            "/Revola/Firmware/loader.bin".to_owned(),
            artifacts.companion.as_slice(),
        ),
        (
            "uploadFirmware".into(),
            "RevolaOS".into(),
            "/Revola/Firmware/revola.bin".into(),
            &artifacts.firmware,
        ),
    ];
    // Extension files follow the core so a failed upload never leaves a core
    // that expects extensions which are not there yet.
    for (index, file) in artifacts.extensions.iter().enumerate() {
        files.push((
            format!("uploadExtension{index}"),
            file.id.clone(),
            format!("/{}", file.path),
            &file.bytes,
        ));
    }
    for (stage, name, destination, bytes) in files {
        let stage = stage.as_str();
        let label = if stage.starts_with("uploadExtension") {
            "uploadExtensions"
        } else {
            stage
        };
        job.stage(label, &format!("Preparing {name}"))?;
        let log_job = std::cell::RefCell::new(&mut *job);
        let uploaded = session.upload_with_log(
            helper,
            &mut Cursor::new(bytes),
            UploadOptions {
                destination: destination.clone(),
                overwrite: true,
            },
            |p| {
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
                    "hash" | "upload" | "write" | "verify" | "complete" => {
                        Some((p.completed, p.total))
                    }
                    _ => None,
                };
                log_job.borrow_mut().emit(detail, progress);
            },
            |detail| log_job.borrow_mut().log(detail),
        )?;
        session = uploaded.session;
        job.save_json(&format!("{stage}.json"), &uploaded.report)?;
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
