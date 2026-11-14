// SPDX-License-Identifier: GPL-3.0-only
use super::{
    job::{save, Job},
    Result,
};
use revola_bundle::{
    assembly::{self, Artifacts},
    preparation::{Ipsw, PreparedInputs},
    VerifiedBundle,
};
use revola_device::{DecryptOptions, Identity, Session, SysCfg};
use std::{fs, path::Path};

pub(super) fn backup(
    session: &mut Session,
    identity: &Identity,
    action: assembly::BootloaderAction,
    job: &mut Job<'_>,
) -> Result<Vec<u8>> {
    if action == assembly::BootloaderAction::Keep {
        job.stage("backup", "Checking saved boot firmware backup")?;
        if let Some(bytes) = job.reuse_nor(identity)? {
            return Ok(bytes);
        }
    }
    job.stage("backup", "Backing up boot firmware")?;
    job.log(if action == assembly::BootloaderAction::Keep {
        "BACKUP: no valid backup for this device; reading full NOR"
    } else {
        "BACKUP: fresh full NOR snapshot required before the bootloader changes"
    });
    // The dump reads the NOR once and reads it again shifted to verify.
    let nor = session.dump_nor(|p| match p.stage {
        "echo" => job.progress("Checking backup connection", p.completed, p.total),
        "nor_verify" => job.progress_in(
            "Verifying boot firmware backup",
            (0.5, 1.0),
            p.completed,
            p.total,
        ),
        _ => job.progress_in("Reading boot firmware", (0.0, 0.5), p.completed, p.total),
    })?;
    job.emit("Saving boot firmware backup", None);
    let syscfg = SysCfg::parse(&nor.bytes)?;
    if syscfg.identity()? != *identity {
        return Err("NOR identity differs from the checked iPod.".into());
    }
    job.save_nor(&nor, syscfg.size)?;
    job.emit("Boot firmware backup saved", None);

    Ok(nor.bytes)
}

/// Whether the decrypted firmware and loader are cached under `data`.
pub(super) fn cached(data: &Path) -> (bool, bool) {
    let cache = data.join("firmware");
    (
        cache.join("osos-2.0.5.bin").is_file(),
        cache.join("aupd-2.0.5.bin").is_file(),
    )
}

pub(super) fn assemble(
    session: &mut Session,
    ipsw: &Ipsw,
    nor: &[u8],
    bundle: &VerifiedBundle,
    data: &Path,
    hardware_policy: revola_device::firmware::HardwarePolicy,
    job: &mut Job<'_>,
) -> Result<Artifacts> {
    job.stage("decrypt", "Preparing Apple firmware")?;
    let cache = data.join("firmware");
    fs::create_dir_all(&cache)?;
    // The loader is a ninth of the firmware's bytes; see `Job::plan`.
    let (osos_band, aupd_band) = match cached(data) {
        (false, false) => ((0.0, 0.9), (0.9, 1.0)),
        _ => ((0.0, 1.0), (0.0, 1.0)),
    };
    let cache_file = cache.join("osos-2.0.5.bin");
    let osos = match fs::read(&cache_file) {
        Ok(image) => {
            job.emit("Verifying cached Apple firmware", None);
            ipsw.validate_plaintext(&image)?;
            job.emit("Using verified decrypted firmware", None);
            image
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let decrypted =
                session.decrypt(ipsw.ciphertext(), DecryptOptions::default(), None, |p| {
                    if p.stage == "echo" {
                        return job.progress("Decrypting Apple firmware", p.completed, p.total);
                    }
                    job.progress_in("Decrypting Apple firmware", osos_band, p.completed, p.total)
                })?;
            job.emit("Verifying decrypted Apple firmware", None);
            let image = ipsw.wrap_plaintext(&decrypted.plaintext)?;
            job.emit("Saving decrypted Apple firmware", None);
            save(&cache_file, &image)?;
            image
        }
        Err(e) => return Err(e.into()),
    };
    let cache_file = cache.join("aupd-2.0.5.bin");
    let aupd = match fs::read(&cache_file) {
        Ok(image) => {
            ipsw.validate_aupd_plaintext(&image)?;
            job.emit("Using verified decrypted Apple loader", None);
            image
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let decrypted = session.decrypt(
                ipsw.aupd_ciphertext(),
                DecryptOptions::default(),
                None,
                |p| {
                    if p.stage == "echo" {
                        return job.progress("Decrypting Apple loader", p.completed, p.total);
                    }
                    job.progress_in("Decrypting Apple loader", aupd_band, p.completed, p.total)
                },
            )?;
            ipsw.validate_aupd_plaintext(&decrypted.plaintext)?;
            save(&cache_file, &decrypted.plaintext)?;
            decrypted.plaintext
        }
        Err(e) => return Err(e.into()),
    };
    job.emit("Validating Apple firmware", None);
    let prepared = PreparedInputs::from_plaintext(ipsw, &osos, &aupd)?;
    job.stage("assemble", "Preparing RevolaOS")?;
    let artifacts = assembly::assemble_with_hardware_policy(
        bundle,
        &prepared.apple_inputs()?,
        nor,
        hardware_policy,
    )?;
    job.save_firmware(&artifacts)?;
    Ok(artifacts)
}
