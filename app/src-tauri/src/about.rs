// SPDX-License-Identifier: GPL-3.0-only
use serde::Serialize;

#[derive(Serialize)]
pub struct AboutInfo {
    version: &'static str,
    notice: &'static str,
}

#[tauri::command]
pub fn about() -> AboutInfo {
    AboutInfo {
        version: env!("CARGO_PKG_VERSION"),
        notice: include_str!("../../../NOTICE"),
    }
}
