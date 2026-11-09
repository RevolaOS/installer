// SPDX-License-Identifier: GPL-3.0-only
//! Extension package checks: the compiled manifest and the `.rvx` module headers.
//! Layouts follow revola `tools/manifest.py` and `tools/rvx.py`; all fields little-endian.

use crate::{invalid, Core, Result};
use std::collections::BTreeSet;

const MANIFEST_MAGIC: u32 = 0x464D_5652; // RVMF
const MANIFEST_VERSION: u32 = 1;
const MANIFEST_HEADER: usize = 36;
const MODULE_RECORD: usize = 8;
pub(crate) const MAX_MODULES: usize = 15;
const RVX_MAGIC: u32 = 0x3058_5652; // RVX0
const RVX_FORMAT: u32 = 1;
const RVX_HEADER: usize = 40;

/// The firmware build id is the first eight hex digits of the decrypted OSOS SHA-256.
pub fn firmware_build_id(osos_sha256: &str) -> Result<u32> {
    osos_sha256
        .get(..8)
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .ok_or_else(|| invalid("Invalid OSOS fingerprint"))
}

pub(crate) fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    })
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn half(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()))
}

/// Module names from `manifest.bin`, after checking the header, id and api against the bundle.
pub(crate) fn module_names(id: &str, manifest: &[u8], core: &Core) -> Result<Vec<String>> {
    let bad = |what: &str| invalid(format!("{id}/manifest.bin: {what}"));
    if manifest.len() < MANIFEST_HEADER
        || word(manifest, 0) != MANIFEST_MAGIC
        || word(manifest, 4) != MANIFEST_VERSION
    {
        return Err(bad("not a version 1 manifest"));
    }
    if word(manifest, 8) >> 16 != core.api {
        return Err(bad("built for another core API"));
    }
    let count = word(manifest, 16) as usize;
    let strings_bytes = word(manifest, 20) as usize;
    let records = MANIFEST_HEADER + count * MODULE_RECORD;
    if count == 0 || count > MAX_MODULES || manifest.len() != records + strings_bytes {
        return Err(bad("module count or string table size"));
    }
    let strings = &manifest[records..];
    let string = |offset: usize| -> Result<&str> {
        let tail = strings.get(offset..).ok_or_else(|| bad("string offset"))?;
        let end = tail
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| bad("unterminated string"))?;
        std::str::from_utf8(&tail[..end]).map_err(|_| bad("string encoding"))
    };
    if string(half(manifest, 24))? != id {
        return Err(bad("id differs from the package id"));
    }
    let mut names = Vec::with_capacity(count);
    for index in 0..count {
        let record = MANIFEST_HEADER + index * MODULE_RECORD;
        let name = string(half(manifest, record))?;
        let file = string(half(manifest, record + 2))?;
        let trigger = manifest[record + 4];
        if !crate::valid_module_name(&format!("{name}.rvx"))
            || file != format!("{name}.rvx")
            || names.iter().any(|n| n == name)
            || (index == 0) != (name == "main" && trigger == 0)
        {
            return Err(bad("module list"));
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

/// Check one module image against the bundle's core and its place in the extension.
pub(crate) fn check_module(id: &str, name: &str, image: &[u8], core: &Core) -> Result<()> {
    let bad = |what: &str| invalid(format!("{id}/{name}.rvx: {what}"));
    if image.len() < RVX_HEADER || word(image, 0) != RVX_MAGIC || word(image, 4) != RVX_FORMAT {
        return Err(bad("not a format 1 rvx module"));
    }
    if word(image, 8) >> 16 != core.api {
        return Err(bad("built for another core API"));
    }
    if word(image, 12) != core.firmware_build_id {
        return Err(bad("built for another firmware"));
    }
    if word(image, 16) != fnv1a(&format!("{id}/{name}")) {
        return Err(bad("module id differs from its name"));
    }
    let text = word(image, 20) as usize;
    let data = word(image, 24) as usize;
    let relocations = word(image, 32) as usize;
    let expected = text
        .checked_add(data)
        .and_then(|n| n.checked_add(relocations.checked_mul(4)?))
        .and_then(|n| n.checked_add(RVX_HEADER));
    if expected != Some(image.len()) {
        return Err(bad("size differs from its header"));
    }
    Ok(())
}

/// The files an extension package must contain for its manifest: the manifest plus one
/// image per module, nothing else.
pub(crate) fn expected_files(names: &[String]) -> BTreeSet<String> {
    names
        .iter()
        .map(|name| format!("{name}.rvx"))
        .chain(std::iter::once("manifest.bin".to_owned()))
        .collect()
}
