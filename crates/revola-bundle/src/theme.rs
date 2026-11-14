// SPDX-License-Identifier: GPL-3.0-only
//! Theme file checks. A theme is one `.rvt` file: a header, then an appearance
//! pack for light and one for dark, either optional. The layout follows
//! `themes/rvt/data/rvt.h`, and the checks are at least the checks its reader
//! makes on the header before it opens a pack, the catalog fingerprint against
//! the bundle's core profile included. All fields are little-endian.

use crate::{
    decode_hex, invalid,
    le::{half, word},
    Core, Result,
};

const RVT_MAGIC: u32 = 0x3054_5652; // RVT0
const RVT_VERSION: usize = 2;

// Header field offsets, `RVT_HEADER_*`.
const MAGIC: usize = 0;
const VERSION: usize = 4;
const HEADER_BYTES: usize = 6;
const FINGERPRINT: usize = 8;
const APPEARANCES: usize = 16;
const LIGHT_OFFSET: usize = 20;
const LIGHT_BYTES: usize = 24;
const DARK_OFFSET: usize = 28;
const DARK_BYTES: usize = 32;
const CRC: usize = 36;
const ID: usize = 40;
const NAME: usize = 72;

// Sizes.
const HEADER: usize = 128;
const FINGERPRINT_BYTES: usize = 8;
const ID_BYTES: usize = 32;
const NAME_BYTES: usize = 48;
const PACK_HEADER: usize = 16;
/// `RVT_STYLE_BYTES`: eight role colours, the flags word, nine accent palettes of
/// five slots, one tint byte per catalog index.
const STYLE: usize = 8 * 4 + 4 + 9 * 5 * 4 + 640;
/// A pack is at least its header and the style prefix.
pub(crate) const MIN_PACK: usize = PACK_HEADER + STYLE;

const APPEARANCE_LIGHT: usize = 1;
const APPEARANCE_DARK: usize = 2;

/// The reader's rule for a theme id, which is also the file's stem:
/// `[a-z0-9][a-z0-9_-]{0,30}`, and not `classic`, the built-in look.
pub(crate) fn valid_id(value: &str) -> bool {
    value != "classic"
        && !value.is_empty()
        && value.len() < ID_BYTES
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
        && value.as_bytes()[0] != b'_'
        && value.as_bytes()[0] != b'-'
}

/// A NUL-terminated string field with zeros after the terminator.
fn text(field: &[u8]) -> Option<&str> {
    let end = field.iter().position(|b| *b == 0)?;
    if field[end..].iter().any(|b| *b != 0) {
        return None;
    }
    std::str::from_utf8(&field[..end]).ok()
}

/// CRC-32 of the header with its own CRC word as zero.
fn header_crc(header: &[u8]) -> u32 {
    let mut crc = crc32fast::Hasher::new();
    crc.update(&header[..CRC]);
    crc.update(&[0; 4]);
    crc.update(&header[CRC + 4..HEADER]);
    crc.finalize()
}

/// Check one theme file against its map key and the bundle's catalog fingerprint;
/// returns the header's name.
pub(crate) fn check_theme(id: &str, file: &[u8], core: &Core) -> Result<String> {
    let bad = |what: &str| invalid(format!("{id}.rvt: {what}"));
    if file.len() < HEADER || word(file, MAGIC) as u32 != RVT_MAGIC {
        return Err(bad("not an rvt file"));
    }
    if half(file, VERSION) != RVT_VERSION {
        return Err(bad("unsupported format version"));
    }
    let header = half(file, HEADER_BYTES);
    if header < HEADER || !header.is_multiple_of(4) || header > file.len() {
        return Err(bad("header size"));
    }
    if header_crc(file) != word(file, CRC) as u32 {
        return Err(bad("header checksum"));
    }
    if file[FINGERPRINT..FINGERPRINT + FINGERPRINT_BYTES]
        != decode_hex::<FINGERPRINT_BYTES>(&core.catalog_fingerprint)?
    {
        return Err(bad("built for another catalog"));
    }
    let mask = word(file, APPEARANCES);
    if mask == 0 || mask & !(APPEARANCE_LIGHT | APPEARANCE_DARK) != 0 {
        return Err(bad("appearance bits"));
    }
    for (bit, offset_at, bytes_at, name) in [
        (APPEARANCE_LIGHT, LIGHT_OFFSET, LIGHT_BYTES, "light"),
        (APPEARANCE_DARK, DARK_OFFSET, DARK_BYTES, "dark"),
    ] {
        let (offset, bytes) = (word(file, offset_at), word(file, bytes_at));
        let present = mask & bit != 0;
        if !present && (offset != 0 || bytes != 0) {
            return Err(bad(&format!("{name} pack placed but absent")));
        }
        if present
            && (offset < header
                || !offset.is_multiple_of(4)
                || bytes < MIN_PACK
                || offset.checked_add(bytes).is_none_or(|end| end > file.len()))
        {
            return Err(bad(&format!("{name} pack misplaced or too small")));
        }
    }
    if text(&file[ID..ID + ID_BYTES]).filter(|found| valid_id(found)) != Some(id) {
        return Err(bad("id invalid or differs from the file name"));
    }
    let name = text(&file[NAME..NAME + NAME_BYTES]).ok_or_else(|| bad("name encoding"))?;
    if name.is_empty() || name.chars().any(|c| c < ' ' || c == '\x7f') {
        return Err(bad("name is empty or has control characters"));
    }
    Ok(name.to_owned())
}
