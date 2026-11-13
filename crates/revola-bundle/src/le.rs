// SPDX-License-Identifier: GPL-3.0-only
//! Little-endian field readers for the on-device file formats.

pub(crate) fn word(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

pub(crate) fn half(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()))
}
