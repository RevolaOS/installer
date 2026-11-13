// SPDX-License-Identifier: GPL-3.0-only
//! Extension package checks. An extension is one `.rvx` file: header, manifest,
//! catalog of modules and assets, payloads. Layouts follow revola
//! `core/loader/rvx.h` and `tools/manifest.py`, and the checks are the ones the
//! core's loader makes, so a package that passes here loads on the device. All
//! fields are little-endian.

use crate::{
    invalid,
    le::{half, word},
    Core, Result,
};
use std::collections::BTreeSet;

const RVX_MAGIC: u32 = 0x3058_5652; // RVX0
const RVX_FORMAT: u32 = 1;
const HEADER: usize = 32;
const ENTRY: usize = 20;
const MODULE_HEADER: usize = 20;
const MAX_ENTRIES: usize = 0xffff;
const MAX_PAYLOAD: usize = 16 << 20;
const KIND_MODULE: u8 = 0;
const KIND_BITMAP: u8 = 2;
const CODEC_RAW: u8 = 0;
const CODEC_LZ: u8 = 1;

const MANIFEST_MAGIC: u32 = 0x464D_5652; // RVMF
const MANIFEST_VERSION: u32 = 3;
const MANIFEST_MAX: usize = 1024;
const MANIFEST_HEADER: usize = 48;
const MAX_CONTENT: usize = 4;
const MODULE_RECORD: usize = 6;
const SITE_RECORD: usize = 12;
const MAX_MODULES: usize = 15;
const MAX_SITES: usize = 8;
const TRIGGER_MANUAL: u8 = 2;

/// The loader's rule for a content folder under `/Revola/`: a capital, then 1 to 15
/// lower-case letters or digits, and not a core folder in any case.
fn content_folder_valid(name: &str) -> bool {
    const CORE_FOLDERS: [&str; 5] = ["Extensions", "Data", "Settings", "Firmware", "Crashes"];
    let mut chars = name.chars();
    let first = chars.next().is_some_and(|c| c.is_ascii_uppercase());
    let rest = chars.as_str();
    first
        && (1..=15).contains(&rest.len())
        && rest
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && !CORE_FOLDERS
            .iter()
            .any(|folder| folder.eq_ignore_ascii_case(name))
}

/// The firmware build id is the first eight hex digits of the decrypted OSOS SHA-256.
pub fn firmware_build_id(osos_sha256: &str) -> Result<u32> {
    osos_sha256
        .get(..8)
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .ok_or_else(|| invalid("Invalid OSOS fingerprint"))
}

/// Check one extension file against the bundle's core and its package id.
pub(crate) fn check_package(id: &str, file: &[u8], core: &Core) -> Result<()> {
    let bad = |what: &str| invalid(format!("{id}.rvx: {what}"));
    if file.len() < HEADER
        || word(file, 0) as u32 != RVX_MAGIC
        || word(file, 4) as u32 != RVX_FORMAT
    {
        return Err(bad("not a format 1 rvx file"));
    }
    if word(file, 8) >> 16 != core.api as usize {
        return Err(bad("built for another core API"));
    }
    if word(file, 12) as u32 != core.firmware_build_id {
        return Err(bad("built for another firmware"));
    }
    let manifest_bytes = word(file, 16);
    if manifest_bytes > MANIFEST_MAX || file.len() < HEADER + manifest_bytes {
        return Err(bad("manifest size"));
    }
    let modules = check_manifest(id, &file[HEADER..HEADER + manifest_bytes], core)?;
    let entries = word(file, 20);
    let resident = word(file, 24);
    let catalog = HEADER + manifest_bytes.div_ceil(4) * 4;
    let payloads = catalog + entries * ENTRY;
    if entries > MAX_ENTRIES
        || entries < modules
        || resident > entries - modules
        || payloads > file.len()
    {
        return Err(bad("catalog does not fit the file or the manifest"));
    }
    let mut names = BTreeSet::new();
    for index in 0..entries {
        let at = catalog + index * ENTRY;
        let (name, kind, codec) = (word(file, at), file[at + 4], file[at + 5]);
        let (offset, stored, bytes) =
            (word(file, at + 8), word(file, at + 12), word(file, at + 16));
        let module = index < modules;
        if (kind == KIND_MODULE) != module || kind > KIND_BITMAP || codec > CODEC_LZ {
            return Err(bad(&format!("entry {index} kind or codec")));
        }
        if (codec == CODEC_RAW && stored != bytes)
            || offset % 4 != 0
            || offset < payloads
            || stored > MAX_PAYLOAD
            || bytes > MAX_PAYLOAD
            || offset + stored > file.len()
        {
            return Err(bad(&format!("entry {index} payload outside the file")));
        }
        if !names.insert(name) {
            return Err(bad("two entries share a name"));
        }
        if module {
            check_module(&file[offset..offset + stored], codec)
                .map_err(|what| bad(&format!("module {index}: {what}")))?;
        }
    }
    Ok(())
}

/// A module payload: struct rvx_module, text and data, relocations.
fn check_module(payload: &[u8], codec: u8) -> std::result::Result<(), &'static str> {
    if codec != CODEC_RAW || payload.len() < MODULE_HEADER {
        return Err("not a raw module payload");
    }
    let (text, data, relocations, entry) = (
        word(payload, 0),
        word(payload, 4),
        word(payload, 12),
        word(payload, 16),
    );
    if text % 4 != 0 || data % 4 != 0 || entry >= text {
        return Err("segment layout");
    }
    let expected = text
        .checked_add(data)
        .and_then(|n| n.checked_add(relocations.checked_mul(4)?))
        .and_then(|n| n.checked_add(MODULE_HEADER));
    if expected != Some(payload.len()) {
        return Err("size differs from its header");
    }
    Ok(())
}

/// The module count of the manifest, after the loader's checks and the id against the package.
fn check_manifest(id: &str, manifest: &[u8], core: &Core) -> Result<usize> {
    let bad = |what: &str| invalid(format!("{id}.rvx manifest: {what}"));
    if manifest.len() < MANIFEST_HEADER
        || word(manifest, 0) as u32 != MANIFEST_MAGIC
        || word(manifest, 4) as u32 != MANIFEST_VERSION
    {
        return Err(bad("not a version 3 manifest"));
    }
    if word(manifest, 8) >> 16 != core.api as usize {
        return Err(bad("built for another core API"));
    }
    let (modules, sites, strings_bytes) =
        (word(manifest, 16), word(manifest, 20), word(manifest, 24));
    if modules == 0 || modules > MAX_MODULES || sites > MAX_SITES {
        return Err(bad("module or site count"));
    }
    let table = MANIFEST_HEADER + sites * SITE_RECORD + modules * MODULE_RECORD;
    if strings_bytes == 0
        || manifest.len() != table + strings_bytes
        || manifest[manifest.len() - 1] != 0
    {
        return Err(bad("string table size"));
    }
    let strings = &manifest[table..];
    let string = |offset: usize| -> Result<&str> {
        let tail = strings
            .get(offset..)
            .filter(|tail| !tail.is_empty())
            .ok_or_else(|| bad("string offset"))?;
        let end = tail.iter().position(|b| *b == 0).unwrap();
        std::str::from_utf8(&tail[..end]).map_err(|_| bad("string encoding"))
    };
    for text in 0..7 {
        string(half(manifest, 28 + 2 * text))?;
    }
    let folders = word(manifest, 44);
    if folders > MAX_CONTENT {
        return Err(bad("content folder count"));
    }
    let mut folder = half(manifest, 42);
    for _ in 0..folders {
        let name = string(folder)?;
        if !content_folder_valid(name) {
            return Err(bad("content folder name"));
        }
        folder += name.len() + 1;
    }
    if string(half(manifest, 28))? != id {
        return Err(bad("id differs from the package id"));
    }
    for site in 0..sites {
        string(half(manifest, MANIFEST_HEADER + site * SITE_RECORD + 10))?;
    }
    let mut names = BTreeSet::new();
    for index in 0..modules {
        let record = MANIFEST_HEADER + sites * SITE_RECORD + index * MODULE_RECORD;
        let name = string(half(manifest, record))?;
        let trigger = manifest[record + 2];
        string(half(manifest, record + 4))?;
        if trigger > TRIGGER_MANUAL
            || !names.insert(name)
            || (index == 0) != (name == "main" && trigger == 0)
        {
            return Err(bad("module list"));
        }
    }
    Ok(modules)
}
