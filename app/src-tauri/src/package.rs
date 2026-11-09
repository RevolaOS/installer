// SPDX-License-Identifier: GPL-3.0-only
mod releases;
#[cfg(test)]
mod tests;

use crate::firmware::FirmwareState;
use revola_bundle::{TrustedKey, VerifiedBundle};
use revola_device::firmware::Firmware;
use serde::Serialize;

use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

const RELEASE_PUBLIC_KEY: &str = "367231334963307a92e328fa271a59c32a4eed0167fb695cf1f0754948292379";

#[derive(Default)]
pub struct PackageState(pub Mutex<Option<Arc<VerifiedBundle>>>);

#[derive(Serialize)]
pub struct PackageInfo {
    version: String,
    digest: String,
    filename: Option<String>,
}

fn package_info(bundle: &VerifiedBundle, filename: Option<String>) -> Result<PackageInfo, String> {
    for component in ["usb_helper", "osos", "companion", "nor"] {
        if !bundle.manifest().components.contains_key(component) {
            return Err(format!("Package is missing the {component} component."));
        }
    }
    revola_device::UploadHelper::from_bytes(
        bundle
            .file("usb_helper", "image")
            .map_err(|e| e.to_string())?,
        bundle
            .file("usb_helper", "descriptor")
            .map_err(|e| e.to_string())?,
    )
    .and_then(|helper| helper.validate_platform())
    .map_err(|e| e.to_string())?;
    revola_bundle::assembly::disk_bytes(bundle).map_err(|e| e.to_string())?;
    revola_bundle::assembly::nor_installer(bundle).map_err(|e| e.to_string())?;
    Ok(PackageInfo {
        version: bundle.manifest().version.clone(),
        digest: bundle.digest().to_owned(),
        filename,
    })
}

#[tauri::command]
pub async fn choose_local_package(app: AppHandle) -> Result<Option<PackageInfo>, String> {
    let operation = app
        .state::<crate::device::UsbState>()
        .acquire()
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let state = app.state::<PackageState>();
        let mut selected = state
            .0
            .try_lock()
            .map_err(|_| "A package is still loading.")?;
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Choose a RevolaOS package")
            .add_filter("RevolaOS package", &["zip"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        app.state::<crate::device::UsbState>()
            .checked
            .lock()
            .map_err(|e| e.to_string())?
            .take();
        let path = file.into_path().map_err(|e| e.to_string())?;
        let bundle = VerifiedBundle::load_local_zip(&path, env!("CARGO_PKG_VERSION"))
            .map_err(|e| e.to_string())?;
        let info = package_info(
            &bundle,
            path.file_name().map(|n| n.to_string_lossy().into_owned()),
        )?;
        *selected = Some(Arc::new(bundle));
        Ok(Some(info))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Without an IPSW (uninstall) the package must match the pinned firmware target.
#[tauri::command]
pub async fn prepare_package(
    app: AppHandle,
    local_digest: Option<String>,
    firmware_sha256: Option<String>,
) -> Result<PackageInfo, String> {
    let operation = app
        .state::<crate::device::UsbState>()
        .acquire()
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        app.state::<crate::device::UsbState>()
            .checked
            .lock()
            .map_err(|e| e.to_string())?
            .take();
        let state = app.state::<PackageState>();
        let mut selected = state
            .0
            .try_lock()
            .map_err(|_| "A package is still loading.")?;
        let target = match firmware_sha256 {
            Some(sha256) => {
                let ipsw = app.state::<FirmwareState>().selected(&sha256)?;
                ipsw.metadata.target().map_err(|e| e.to_string())?
            }
            None => revola_device::firmware::current(),
        };
        if let Some(digest) = local_digest {
            let bundle = selected
                .as_ref()
                .filter(|b| b.digest() == digest)
                .ok_or("Choose the local package again.")?;
            require_target(&bundle.manifest().compatibility, target)?;
            return package_info(bundle, None);
        }
        let url = match option_env!("REVOLA_BUNDLE_MANIFEST_URL") {
            Some(url) => url.to_owned(),
            None => releases::latest_manifest_url(&target.target)?,
        };
        let key = option_env!("REVOLA_BUNDLE_PUBLIC_KEY").unwrap_or(RELEASE_PUBLIC_KEY);
        let key = TrustedKey::from_hex(key).map_err(|e| e.to_string())?;
        let cache = app
            .path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("bundles");
        let (_, bundle) = revola_bundle::fetch(&url, None, &key, &cache, env!("CARGO_PKG_VERSION"))
            .map_err(|e| e.to_string())?;
        require_target(&bundle.manifest().compatibility, target)?;
        let info = package_info(&bundle, None)?;
        *selected = Some(Arc::new(bundle));
        Ok(info)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn require_target(
    compatibility: &revola_bundle::Compatibility,
    target: &Firmware,
) -> Result<(), String> {
    if compatibility.target != target.target {
        return Err(format!(
            "This package is for {}. Choose the Classic package for Apple {}.",
            compatibility.target, target.ipsw.version
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn require_device(
    compatibility: &revola_bundle::Compatibility,
    report: &revola_device::CheckReport,
) -> Result<(), String> {
    require_device_with_policy(
        compatibility,
        report,
        revola_device::firmware::HardwarePolicy::RequireKnown,
    )
}

pub(crate) fn require_device_with_policy(
    compatibility: &revola_bundle::Compatibility,
    report: &revola_device::CheckReport,
    policy: revola_device::firmware::HardwarePolicy,
) -> Result<(), String> {
    let identity = report
        .identity
        .as_ref()
        .ok_or("Missing checked device identity.")?;
    if !policy.permits(identity.hardware_version, compatibility)
        || report.bootrom_sha256.as_deref() != Some(&compatibility.bootrom_sha256)
    {
        return Err(format!(
            "The selected firmware package does not support this {} iPod. Choose a compatible Classic package.",
            identity.model
        ));
    }
    Ok(())
}
