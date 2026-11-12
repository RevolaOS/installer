// SPDX-License-Identifier: GPL-3.0-only
use super::{
    firmware,
    job::{self, InstallEvent, InstallOutcome, InstallResult, Job},
    selected_bundle, start, take_checked, InstallState, Result,
};
use revola_bundle::assembly;
use revola_device::UploadHelper;
use tauri::{ipc::Channel, AppHandle, Manager};

/// Remove the RevolaOS files, then restore Apple's bootloader. Files go first because
/// the RevolaOS bootloader stops at a missing-file screen without them, while the
/// restored Apple bootloader ignores them. Removal is best effort: whatever refuses
/// to go is reported and the bootloader is restored anyway, so the iPod never ends up
/// with the RevolaOS bootloader and no firmware file. Both steps can run again.
#[tauri::command]
pub async fn uninstall(
    app: AppHandle,
    package_digest: String,
    on_event: Channel<InstallEvent>,
) -> std::result::Result<InstallResult, String> {
    start(app, move |app, state| {
        run(app, state, package_digest, on_event)
    })
    .await
}

fn run(
    app: &AppHandle,
    state: &InstallState,
    package_digest: String,
    channel: Channel<InstallEvent>,
) -> Result<InstallResult> {
    let bundle = selected_bundle(app, &package_digest)?;
    let helper = UploadHelper::from_bytes(
        bundle.file("usb_helper", "image")?,
        bundle.file("usb_helper", "descriptor")?,
    )?;
    let uninstaller = assembly::nor_uninstaller(&bundle)?;
    let checked = take_checked(app, &bundle)?;
    let identity = checked
        .report
        .identity
        .as_ref()
        .ok_or("Missing checked device identity.")?;
    let data = app.path().app_data_dir()?;
    let directory = job::backup_directory(&data, &identity.hardware_id)?;
    job::save_json(&directory.join("device.json"), &checked.report)?;
    job::save_json(
        &directory.join("inputs.json"),
        &serde_json::json!({"bundle": bundle.digest()}),
    )?;
    let mut job = Job::new(state, channel, directory)?;
    job.log(format!(
        "Installer {}; uninstall with package {} ({})",
        env!("CARGO_PKG_VERSION"),
        bundle.manifest().version,
        bundle.digest(),
    ));
    job.log(format!(
        "Device: {} serial={} hardware_id={:02x?}; logs={}",
        identity.model,
        identity.serial,
        identity.hardware_id,
        job.directory.display()
    ));
    let outcome = (|| {
        let mut session = checked.session;
        job.stage("checkBootloader", "Checking installed bootloader")?;
        let bootloader = session.inspect_bootloader(|p| {
            job.progress("Verifying installed bootloader", p.completed, p.total)
        })?;
        job.save_json("bootloader.json", &bootloader)?;
        job.log(format!(
            "Detected bootloader: apple={}; code_sha256={}",
            bootloader.apple, bootloader.fingerprint.sha256
        ));
        job.plan_uninstall(bootloader.apple)?;
        if !bootloader.apple {
            firmware::backup(
                &mut session,
                identity,
                assembly::BootloaderAction::Upgrade,
                &mut job,
            )?;
        }

        job.stage("removeFiles", "Removing RevolaOS files")?;
        let directory = job.directory.clone();
        let (session, removed) =
            session.remove_files_with_log(&helper, Some(&directory), |detail| job.log(detail))?;
        job.save_json("removal.json", &removed)?;
        job.emit(
            format!(
                "Removed: companion loader {}; RevolaOS {}; extensions {}; unfinished uploads {}",
                removed.companion, removed.firmware, removed.extensions, removed.staged_uploads
            ),
            None,
        );
        job.leftovers = removed.leftovers();
        if !job.leftovers.is_empty() {
            job.emit(
                format!(
                    "Could not remove {}; continuing so the bootloader is restored",
                    job.leftovers.join(", ")
                ),
                None,
            );
        }

        if bootloader.apple {
            job.begin_restart()?;
            session.restart()?;
            return Ok(InstallOutcome::FilesRemoved);
        }
        job.begin_bootloader_removal()?;
        session.install_bootloader(&uninstaller, |done, total| {
            job.emit(
                "Sending bootloader uninstaller",
                Some((done as u64, total as u64)),
            )
        })?;
        job.emit("Bootloader uninstaller sent", None);
        Ok(InstallOutcome::UninstallerSent)
    })();
    job.finish(outcome)
}
