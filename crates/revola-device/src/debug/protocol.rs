// SPDX-License-Identifier: GPL-3.0-only
//! Vendor control requests of the USB Debug extension (device recipient).

use crate::{invalid, Result};
use serde::Serialize;

pub(crate) const VENDOR_IN: u8 = 0xc0;
pub(crate) const VENDOR_OUT: u8 = 0x40;

pub(crate) const INFO: u8 = 0xd0;
pub(crate) const READ: u8 = 0xd1;
pub(crate) const SET_ADDRESS: u8 = 0xd2;
pub(crate) const WRITE32: u8 = 0xd3;
pub(crate) const INPUT: u8 = 0xd4;
pub(crate) const PROFILE: u8 = 0xd5;
pub(crate) const SAMPLES: u8 = 0xd6;
pub(crate) const SCREEN: u8 = 0xd7;
pub(crate) const SCREEN_DATA: u8 = 0xd8;

pub(crate) const INFO_SIZE: usize = 64;
const INFO_MAGIC: u32 = u32::from_le_bytes(*b"RVUD");
const SCREEN_MAGIC: u32 = u32::from_le_bytes(*b"RVSC");
pub(crate) const PROTOCOL_VERSION: u16 = 1;

/// INPUT kinds, in the low byte of wValue; the argument goes in wIndex.
pub(crate) const INPUT_BUTTONS: u16 = 1;
pub(crate) const INPUT_TOUCH: u16 = 2;
pub(crate) const INPUT_LIFT: u16 = 3;

/// The request's name for error messages.
pub(crate) fn request_name(request: u8) -> &'static str {
    match request {
        INFO => "USB Debug INFO",
        READ => "USB Debug READ",
        SET_ADDRESS => "USB Debug SET_ADDRESS",
        WRITE32 => "USB Debug WRITE32",
        INPUT => "USB Debug INPUT",
        PROFILE => "USB Debug PROFILE",
        SAMPLES => "USB Debug SAMPLES",
        SCREEN => "USB Debug SCREEN",
        SCREEN_DATA => "USB Debug SCREEN_DATA",
        _ => "USB Debug request",
    }
}

pub(crate) fn split(address: u32) -> (u16, u16) {
    (address as u16, (address >> 16) as u16)
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// Offsets inside the core's extension records, as `debug_info()` reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RecordLayout {
    pub next: u16,
    pub owner: u16,
    pub directory: u16,
    pub live: u16,
    pub peak: u16,
    pub failure: u16,
}

/// Reply to INFO: identity, transfer limit and the addresses the host reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Info {
    pub version: u16,
    pub firmware_id: u32,
    pub core_build: u32,
    pub max_transfer: u32,
    pub display_base: u32,
    pub heap_total: u32,
    pub heap_free: u32,
    /// Address of the pointer to the first extension record.
    pub extensions: u32,
    pub record: RecordLayout,
}

impl Info {
    pub(crate) fn parse(b: &[u8]) -> Result<Self> {
        if b.len() < INFO_SIZE || u32_at(b, 0) != INFO_MAGIC {
            return Err(invalid(
                "No USB Debug reply; is the extension enabled on the iPod?",
            ));
        }
        let version = u16_at(b, 4);
        if version != PROTOCOL_VERSION {
            return Err(invalid(format!(
                "USB Debug protocol {version} on the iPod, this tool speaks {PROTOCOL_VERSION}"
            )));
        }
        let max_transfer = u32_at(b, 16);
        if !(64..=0x10000).contains(&max_transfer) {
            return Err(invalid(format!(
                "Implausible transfer limit {max_transfer}"
            )));
        }
        Ok(Self {
            version,
            firmware_id: u32_at(b, 8),
            core_build: u32_at(b, 12),
            max_transfer,
            display_base: u32_at(b, 20),
            heap_total: u32_at(b, 24),
            heap_free: u32_at(b, 28),
            extensions: u32_at(b, 32),
            record: RecordLayout {
                next: u16_at(b, 36),
                owner: u16_at(b, 38),
                directory: u16_at(b, 40),
                live: u16_at(b, 42),
                peak: u16_at(b, 44),
                failure: u16_at(b, 46),
            },
        })
    }
}

/// One PC sample: the interrupted program counter and link register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub pc: u32,
    pub lr: u32,
}

/// SAMPLES: {count, dropped} then count (pc, lr) pairs.
pub(crate) fn parse_samples(b: &[u8]) -> Result<(Vec<Sample>, u32)> {
    if b.len() < 8 {
        return Err(invalid("Short SAMPLES reply"));
    }
    let count = u32_at(b, 0) as usize;
    if 8 + count * 8 > b.len() {
        return Err(invalid("SAMPLES count exceeds the reply"));
    }
    let samples = (0..count)
        .map(|i| Sample {
            pc: u32_at(b, 8 + i * 8),
            lr: u32_at(b, 12 + i * 8),
        })
        .collect();
    Ok((samples, u32_at(b, 4)))
}

/// A string the UI drew during a SCREEN capture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DrawnText {
    pub text: String,
    pub top: i16,
    pub left: i16,
    pub bottom: i16,
    pub right: i16,
    /// 0xRRGGBB
    pub color: u32,
    pub view_class: u32,
}

/// SCREEN_DATA: header {magic, status, count, bytes}, then records
/// {top, left, bottom, right: i16; color, view_class: u32; length: u8; text}
/// padded to 4 bytes. `None` while the capture is pending.
pub(crate) fn parse_screen(b: &[u8]) -> Result<Option<Vec<DrawnText>>> {
    if b.len() < 12 || u32_at(b, 0) != SCREEN_MAGIC {
        return Err(invalid("Bad SCREEN_DATA reply"));
    }
    if u16_at(b, 4) == 0 {
        return Ok(None);
    }
    let count = u16_at(b, 6) as usize;
    let bytes = u32_at(b, 8) as usize;
    if bytes > b.len() {
        return Err(invalid("SCREEN_DATA longer than the reply"));
    }
    let mut at = 12;
    let mut texts = Vec::with_capacity(count);
    for _ in 0..count {
        if at + 17 > bytes {
            return Err(invalid("Truncated SCREEN_DATA record"));
        }
        let length = b[at + 16] as usize;
        let end = at + 17 + length;
        if end > bytes {
            return Err(invalid("Truncated SCREEN_DATA text"));
        }
        let i16_at = |k: usize| u16_at(b, at + k) as i16;
        texts.push(DrawnText {
            text: String::from_utf8_lossy(&b[at + 17..end]).into_owned(),
            top: i16_at(0),
            left: i16_at(2),
            bottom: i16_at(4),
            right: i16_at(6),
            color: u32_at(b, at + 8) & 0xff_ffff,
            view_class: u32_at(b, at + 12),
        });
        at = (end + 3) & !3;
    }
    Ok(Some(texts))
}

#[cfg(test)]
pub(crate) mod encode {
    //! The device side of the formats, for tests.
    use super::*;

    pub fn info(max_transfer: u32) -> Vec<u8> {
        let mut b = vec![0; INFO_SIZE];
        b[0..4].copy_from_slice(b"RVUD");
        b[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        b[16..20].copy_from_slice(&max_transfer.to_le_bytes());
        b[20..24].copy_from_slice(&0x3890_0000u32.to_le_bytes());
        b
    }

    pub fn screen(texts: &[DrawnText]) -> Vec<u8> {
        let mut b = vec![0; 12];
        b[0..4].copy_from_slice(b"RVSC");
        b[4..6].copy_from_slice(&1u16.to_le_bytes());
        b[6..8].copy_from_slice(&(texts.len() as u16).to_le_bytes());
        for t in texts {
            for v in [t.top, t.left, t.bottom, t.right] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend_from_slice(&t.color.to_le_bytes());
            b.extend_from_slice(&t.view_class.to_le_bytes());
            b.push(t.text.len() as u8);
            b.extend_from_slice(t.text.as_bytes());
            while !b.len().is_multiple_of(4) {
                b.push(0);
            }
        }
        let n = b.len() as u32;
        b[8..12].copy_from_slice(&n.to_le_bytes());
        b
    }
}
