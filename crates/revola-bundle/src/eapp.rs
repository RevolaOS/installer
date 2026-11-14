// SPDX-License-Identifier: GPL-3.0-only
//! Package checks. A package is the folder `eapp-pack` writes (revola
//! `sdk/app/eapp_package.py`): `Manifest.plist`, the image, the launching artwork
//! and any file the app reads by name. The checks are the ones that keep Apple's
//! game host from refusing the folder: the manifest belongs to the folder, names
//! every other file with its size, and points at files that are there. The image
//! itself was checked when it was packed.

use crate::{invalid, Result};
use plist::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const MANIFEST: &str = "Manifest.plist";
pub const MAX_FILES: usize = 16;

/// A file name inside the package folder: printable ASCII without the characters
/// FAT refuses, at most 64 bytes, and not a directory reference.
pub(crate) fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b"/\\:*?\"<>|".contains(&b))
}

/// Check one package folder against its map key; returns the manifest's `Name`.
pub(crate) fn check_package(id: &str, files: &BTreeMap<&str, &[u8]>) -> Result<String> {
    let bad = |what: &str| invalid(format!("{id}/{MANIFEST}: {what}"));
    let manifest = files.get(MANIFEST).ok_or_else(|| bad("missing"))?;
    let manifest = Value::from_reader_xml(*manifest)
        .map_err(|_| bad("not an XML property list"))?
        .into_dictionary()
        .ok_or_else(|| bad("not a dictionary"))?;
    let text = |key: &str| {
        manifest
            .get(key)
            .and_then(Value::as_string)
            .ok_or_else(|| bad(&format!("{key} is missing or not a string")))
    };
    let name = text("Name")?;
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(bad("Name is empty or has control characters"));
    }
    if text("GUID")? != id || text("UserDataPath")? != id {
        return Err(bad("GUID or UserDataPath differs from the folder name"));
    }
    text("Version")?;
    if manifest
        .get("HeapSize")
        .and_then(Value::as_unsigned_integer)
        .is_none_or(|heap| heap == 0)
    {
        return Err(bad("HeapSize is missing or zero"));
    }
    let mut listed = BTreeSet::new();
    let entries = manifest
        .get("Files")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("Files is missing or not an array"))?;
    for entry in entries {
        let entry = entry
            .as_dictionary()
            .ok_or_else(|| bad("a Files entry is not a dictionary"))?;
        let path = entry
            .get("Path")
            .and_then(Value::as_string)
            .ok_or_else(|| bad("a Files entry has no Path"))?;
        let bytes = files
            .get(path)
            .filter(|_| path != MANIFEST)
            .ok_or_else(|| bad(&format!("{path} is listed but not in the folder")))?;
        if entry.get("Size").and_then(Value::as_unsigned_integer) != Some(bytes.len() as u64) {
            return Err(bad(&format!("{path} size differs from the file")));
        }
        if !listed.insert(path) {
            return Err(bad(&format!("{path} is listed twice")));
        }
    }
    if let Some(file) = files
        .keys()
        .find(|file| **file != MANIFEST && !listed.contains(*file))
    {
        return Err(bad(&format!("{file} is in the folder but not listed")));
    }
    let platform = manifest
        .get("Platforms")
        .and_then(Value::as_array)
        .filter(|platforms| platforms.len() == 1)
        .and_then(|platforms| platforms[0].as_dictionary())
        .ok_or_else(|| bad("Platforms is not one dictionary"))?;
    for key in ["ExecutablePath", "LaunchingArtwork"] {
        let path = platform
            .get(key)
            .and_then(Value::as_string)
            .ok_or_else(|| bad(&format!("{key} is missing or not a string")))?;
        if !listed.contains(path) {
            return Err(bad(&format!("{key} is not a listed file")));
        }
    }
    Ok(name.to_owned())
}
