// SPDX-License-Identifier: GPL-3.0-only
//! Screenshots from the display pipe: up to three layers composited the way the
//! hardware does it, read from the pipe's registers and the layers' memory.

use crate::Result;

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 240;
/// Bytes of the pipe's register window the compositor needs.
pub(crate) const REGISTER_BYTES: usize = 0x100;

const CONTROL: usize = 0x008;
const ORDER: usize = 0x0d4;
const SLOT0: usize = 0x058;
const SLOT_STEP: usize = 0x018;
const LAYERS: usize = 3;
/// Back to front, selected by the low byte of ORDER (firmware table 0x083f35ec).
const ORDERS: [[usize; LAYERS]; 6] = [
    [1, 2, 0],
    [2, 1, 0],
    [0, 2, 1],
    [2, 0, 1],
    [0, 1, 2],
    [1, 0, 2],
];

fn word(registers: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(registers[at..at + 4].try_into().unwrap())
}

/// One enabled layer, clipped to the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Layer {
    pub source: u32,
    pub stride: u32,
    pub bytes_per_pixel: usize,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// The enabled layers, back to front, from the pipe's registers.
pub(crate) fn layers(registers: &[u8]) -> Vec<Layer> {
    let control = word(registers, CONTROL);
    let order = ORDERS
        .get((word(registers, ORDER) & 0xff) as usize)
        .unwrap_or(&ORDERS[1]);
    order
        .iter()
        .filter_map(|&k| {
            if control & (0x40 >> k) == 0 {
                return None;
            }
            let base = SLOT0 + k * SLOT_STEP;
            let stride = word(registers, base);
            let bytes_per_pixel = match (word(registers, base + 4) >> 8) & 0xff {
                2..=5 => 2,
                6 | 7 => 4,
                _ => return None,
            };
            let mut source = word(registers, base + 8);
            let size = word(registers, base + 0xc);
            let position = word(registers, base + 0x14);
            let (mut w, mut h) = ((size >> 16) as i16 as i32, size as i16 as i32);
            let (mut x, mut y) = ((position >> 16) as i16 as i32, position as i16 as i32);
            if source == 0 || w <= 0 || h <= 0 {
                return None;
            }
            if x < 0 {
                source = source.wrapping_add((-x) as u32 * bytes_per_pixel as u32);
                w += x;
                x = 0;
            }
            if y < 0 {
                source = source.wrapping_add((-y) as u32 * stride);
                h += y;
                y = 0;
            }
            w = w.min(WIDTH as i32 - x);
            h = h.min(HEIGHT as i32 - y);
            (w > 0 && h > 0).then_some(Layer {
                source,
                stride,
                bytes_per_pixel,
                x: x as usize,
                y: y as usize,
                width: w as usize,
                height: h as usize,
            })
        })
        .collect()
}

/// Composite the enabled layers into 320x240 RGB, reading each layer's rows
/// through `read(address, bytes)`.
pub(crate) fn composite(
    registers: &[u8],
    mut read: impl FnMut(u32, usize) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut rgb = vec![0u8; WIDTH * HEIGHT * 3];
    for layer in layers(registers) {
        let row_bytes = layer.width * layer.bytes_per_pixel;
        let contiguous = layer.stride as usize == row_bytes;
        let block = if contiguous {
            read(layer.source, row_bytes * layer.height)?
        } else {
            Vec::new()
        };
        for row in 0..layer.height {
            let fetched;
            let pixels = if contiguous {
                &block[row * row_bytes..(row + 1) * row_bytes]
            } else {
                fetched = read(layer.source + row as u32 * layer.stride, row_bytes)?;
                &fetched[..]
            };
            let out = &mut rgb[((layer.y + row) * WIDTH + layer.x) * 3..];
            for (i, dst) in out
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .take(layer.width)
                .enumerate()
            {
                if layer.bytes_per_pixel == 2 {
                    let v = u16::from_le_bytes([pixels[i * 2], pixels[i * 2 + 1]]);
                    let (r, g, b) = ((v >> 11) & 0x1f, (v >> 5) & 0x3f, v & 0x1f);
                    dst.copy_from_slice(&[
                        (r << 3 | r >> 2) as u8,
                        (g << 2 | g >> 4) as u8,
                        (b << 3 | b >> 2) as u8,
                    ]);
                } else {
                    let p = &pixels[i * 4..i * 4 + 4];
                    let a = p[3] as u32;
                    for (c, s) in dst.iter_mut().zip([p[2], p[1], p[0]]) {
                        *c = ((s as u32 * a + *c as u32 * (255 - a)) / 255) as u8;
                    }
                }
            }
        }
    }
    Ok(rgb)
}

/// 8-bit RGB PNG with stored (uncompressed) deflate blocks.
pub fn png(rgb: &[u8], width: usize, height: usize) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut raw = Vec::with_capacity((width * 3 + 1) * height);
    for row in rgb.chunks_exact(width * 3).take(height) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks = raw.chunks(0xffff).collect::<Vec<_>>();
    for (i, block) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        let n = block.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&(b << 16 | a).to_be_bytes());
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registers(slots: &[(usize, u32, u32, u32, u32, u32)], control: u32, order: u32) -> Vec<u8> {
        let mut r = vec![0u8; REGISTER_BYTES];
        let mut put = |at: usize, v: u32| r[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(CONTROL, control);
        put(ORDER, order);
        for &(k, stride, format, source, size, position) in slots {
            let base = SLOT0 + k * SLOT_STEP;
            put(base, stride);
            put(base + 4, format << 8);
            put(base + 8, source);
            put(base + 0xc, size);
            put(base + 0x14, position);
        }
        r
    }

    #[test]
    fn composites_enabled_layers_in_order_with_alpha() {
        // Layer 1: opaque RGB565 red; layer 0: ARGB white at half alpha, in front (order 1).
        let regs = registers(
            &[
                (0, 4 * 320, 7, 0x1000_0000, 320 << 16 | 240, 0),
                (1, 2 * 320, 3, 0x2000_0000, 320 << 16 | 240, 0),
                (2, 0, 7, 0x3000_0000, 320 << 16 | 240, 0),
            ],
            0x60, // layers 0 and 1
            1,
        );
        assert_eq!(
            layers(&regs).iter().map(|l| l.source).collect::<Vec<_>>(),
            [0x2000_0000, 0x1000_0000]
        );
        let rgb = composite(&regs, |address, n| {
            Ok(match address {
                0x2000_0000 => [0x00, 0xf8].repeat(n / 2),
                0x1000_0000 => [0xff, 0xff, 0xff, 0x80].repeat(n / 4),
                other => panic!("unexpected read {other:#x}"),
            })
        })
        .unwrap();
        assert_eq!(&rgb[..3], &[255, 128, 128]);
    }

    #[test]
    fn clips_negative_positions_and_reads_strided_rows() {
        let regs = registers(
            &[(
                0,
                4 * 400,
                7,
                0x1000_0000,
                400 << 16 | 10,
                (-10i16 as u16 as u32) << 16 | 5,
            )],
            0x40,
            1,
        );
        let layer = layers(&regs)[0];
        assert_eq!(
            (layer.x, layer.y, layer.width, layer.height),
            (0, 5, 320, 10)
        );
        assert_eq!(layer.source, 0x1000_0000 + 40);
        let mut reads = 0;
        composite(&regs, |_, n| {
            reads += 1;
            Ok(vec![0xff; n])
        })
        .unwrap();
        assert_eq!(reads, 10);
    }

    #[test]
    fn png_has_valid_structure() {
        let image = png(&[7u8; 4 * 3 * 2], 4, 2);
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&image[12..16], b"IHDR");
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert!(image.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]));
    }
}
