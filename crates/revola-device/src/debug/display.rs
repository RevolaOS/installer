// SPDX-License-Identifier: GPL-3.0-only
//! Display-pipe screenshots matching the VM alpha compositor. With no active
//! window the result is black; LCD port writes and retained frames are unavailable.

use super::symbols::*;
use crate::Result;

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 240;
/// Bytes of the pipe's register window the compositor needs.
pub(crate) const REGISTER_BYTES: usize = 0x100;

/// Stacking positions of layer 0, layers 1..3, and video, respectively.
const ORDERS: [[usize; 3]; 6] = [
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

/// RGB windows and video in back-to-front order (video is window 5).
fn windows(registers: &[u8]) -> Vec<usize> {
    let order = ORDERS
        .get((word(registers, ORDER) & 0xff) as usize)
        .unwrap_or(&ORDERS[1]);
    let mut windows = vec![4];
    for position in 0..3 {
        for (column, &rank) in order.iter().enumerate() {
            if rank == position {
                match column {
                    0 => windows.push(0),
                    1 => windows.extend([3, 2, 1]),
                    _ => windows.push(5),
                }
            }
        }
    }
    windows
}

fn layer(registers: &[u8], k: usize) -> Option<Layer> {
    if word(registers, CONTROL) & ((LAYER0_ENABLE as u32) >> k) == 0 {
        return None;
    }
    let base = SLOT0 + k * SLOT_STEP;
    let stride = word(registers, base + SLOT_STRIDE);
    let bytes_per_pixel = match (word(registers, base + SLOT_FORMAT) >> 8) & 0xff {
        2..=5 => 2,
        6 | 7 => 4,
        _ => return None,
    };
    let mut source = word(registers, base + SLOT_SOURCE);
    let size = word(registers, base + SLOT_SIZE);
    let position = word(registers, base + SLOT_POSITION);
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
}

fn rgb565(r: u32, g: u32, b: u32) -> u16 {
    (((r & 0xf8) << 8) | ((g & 0xfc) << 3) | (b >> 3)) as u16
}

// Limited-range BT.601 coefficients in 16.16 fixed point.
fn yuv565(y: u8, cb: u8, cr: u8) -> u16 {
    let l = 76284 * (i32::from(y) - 16);
    let cb = i32::from(cb) - 128;
    let cr = i32::from(cr) - 128;
    let r = ((l + 104595 * cr) >> 16).clamp(0, 255);
    let g = ((l - 25690 * cb - 53281 * cr) >> 16).clamp(0, 255);
    let b = ((l + 132186 * cb) >> 16).clamp(0, 255);
    rgb565(r as u32, g as u32, b as u32)
}

fn draw_video(
    registers: &[u8],
    frame: &mut [u16],
    read: &mut impl FnMut(u32, usize) -> Result<Vec<u8>>,
) -> Result<()> {
    let r = |at| word(registers, at);
    let start = r(VIDEO_SOURCE_START);
    let end = r(VIDEO_SOURCE_END);
    let sx0 = start >> 16;
    let sy0 = start & 0xffff;
    let sw = (end >> 16) as i32 - sx0 as i32;
    let sh = (end & 0xffff) as i32 - sy0 as i32;
    let size = r(VIDEO_SIZE);
    let pos = r(VIDEO_POSITION);
    let w = (size >> 16) as i16 as i32;
    let h = size as i16 as i32;
    let x = (pos >> 16) as i16 as i32;
    let y = pos as i16 as i32;
    if r(CONTROL) & VIDEO_ENABLE as u32 == 0
        || r(VIDEO_PLANE0) == 0
        || sw <= 0
        || sh <= 0
        || w <= 0
        || h <= 0
        || r(VIDEO_MODE) != VIDEO_MODE_YUV420 as u32
    {
        return Ok(());
    }
    // The VM's row scratch holds 640 luma and 320 samples per chroma plane.
    let sw = sw.min(640) as usize;
    let strides = r(VIDEO_STRIDES);
    let step = r(VIDEO_SCALE);
    for dy in 0..h {
        let py = y + dy;
        let sy = sy0 + ((dy as u64 * u64::from(step & 0xffff)) >> 12) as u32;
        if py < 0 {
            continue;
        }
        if py >= HEIGHT as i32 || sy >= sy0 + sh as u32 {
            break;
        }
        let yr = read(
            r(VIDEO_PLANE0).wrapping_add(sy * (strides & 0xffff) + sx0),
            sw,
        )?;
        let cb = read(
            r(VIDEO_PLANE1).wrapping_add((sy / 2) * (strides >> 16) + sx0 / 2),
            sw.div_ceil(2),
        )?;
        let cr = read(
            r(VIDEO_PLANE3).wrapping_add((sy / 2) * (strides >> 16) + sx0 / 2),
            sw.div_ceil(2),
        )?;
        for dx in 0..w {
            let px = x + dx;
            let sx = ((dx as u64 * u64::from(step >> 16)) >> 12) as usize;
            if px < 0 {
                continue;
            }
            if px >= WIDTH as i32 || sx >= sw {
                break;
            }
            frame[py as usize * WIDTH + px as usize] = yuv565(yr[sx], cb[sx / 2], cr[sx / 2]);
        }
    }
    Ok(())
}

/// Composite into RGB565, then expand to 320x240 RGB for PNG encoding.
pub(crate) fn composite(
    registers: &[u8],
    mut read: impl FnMut(u32, usize) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut frame = vec![0u16; WIDTH * HEIGHT];
    for window in windows(registers) {
        if window == 5 {
            draw_video(registers, &mut frame, &mut read)?;
            continue;
        }
        let Some(layer) = layer(registers, window) else {
            continue;
        };
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
                fetched = read(
                    layer.source.wrapping_add(row as u32 * layer.stride),
                    row_bytes,
                )?;
                &fetched[..]
            };
            for (i, dst) in frame[(layer.y + row) * WIDTH + layer.x..]
                .iter_mut()
                .take(layer.width)
                .enumerate()
            {
                if layer.bytes_per_pixel == 2 {
                    *dst = u16::from_le_bytes([pixels[i * 2], pixels[i * 2 + 1]]);
                } else {
                    let p = &pixels[i * 4..i * 4 + 4];
                    let a = u32::from(p[3]);
                    if a == 0 {
                        continue;
                    }
                    let d = u32::from(*dst);
                    let dr = (d >> 8) & 0xf8;
                    let dg = (d >> 3) & 0xfc;
                    let db = (d << 3) & 0xf8;
                    *dst = rgb565(
                        (u32::from(p[2]) * a + dr * (255 - a)) / 255,
                        (u32::from(p[1]) * a + dg * (255 - a)) / 255,
                        (u32::from(p[0]) * a + db * (255 - a)) / 255,
                    );
                }
            }
        }
    }
    Ok(frame
        .into_iter()
        .flat_map(|v| {
            let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
            [
                (r << 3 | r >> 2) as u8,
                (g << 2 | g >> 4) as u8,
                (b << 3 | b >> 2) as u8,
            ]
        })
        .collect())
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
            put(base + SLOT_STRIDE, stride);
            put(base + SLOT_FORMAT, format << 8);
            put(base + SLOT_SOURCE, source);
            put(base + SLOT_SIZE, size);
            put(base + SLOT_POSITION, position);
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
            windows(&regs)
                .into_iter()
                .filter(|&k| k < 5)
                .filter_map(|k| layer(&regs, k))
                .map(|l| l.source)
                .collect::<Vec<_>>(),
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
        assert_eq!(&rgb[..3], &[255, 130, 132]);
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
        let layer = layer(&regs, 0).unwrap();
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

    fn put(r: &mut [u8], at: usize, value: u32) {
        r[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn video_registers() -> Vec<u8> {
        let mut r = registers(&[], VIDEO_ENABLE as u32, 1);
        for (at, value) in [
            (VIDEO_MODE, VIDEO_MODE_YUV420 as u32),
            (VIDEO_STRIDES, 4 | (2 << 16)),
            (VIDEO_SOURCE_END, (4 << 16) | 2),
            (VIDEO_PLANE0, 0x1000),
            (VIDEO_PLANE1, 0x2000),
            (VIDEO_PLANE3, 0x3000),
            (VIDEO_SCALE, 0x10001000),
            (VIDEO_SIZE, (4 << 16) | 2),
        ] {
            put(&mut r, at, value);
        }
        r
    }

    #[test]
    fn orders_rank_columns_and_keep_layer_four_at_back() {
        let expected = [
            vec![4, 5, 0, 3, 2, 1],
            vec![4, 5, 3, 2, 1, 0],
            vec![4, 0, 5, 3, 2, 1],
            vec![4, 3, 2, 1, 5, 0],
            vec![4, 0, 3, 2, 1, 5],
            vec![4, 3, 2, 1, 0, 5],
        ];
        for (order, expected) in expected.iter().enumerate() {
            assert_eq!(&windows(&registers(&[], 0, order as u32)), expected);
        }
        assert_eq!(windows(&registers(&[], 0, 255)), expected[1]);
    }

    #[test]
    fn each_rgb_window_obeys_its_enable_bit() {
        for k in 0..5 {
            let slots = [(k, 2, 3, 0x1000, (1 << 16) | 1, 0)];
            let r = registers(&slots, (LAYER0_ENABLE >> k) as u32, 1);
            let rgb = composite(&r, |_, n| Ok([0x00, 0xf8].repeat(n / 2))).unwrap();
            assert_eq!(&rgb[..3], &[255, 0, 0]);
            composite(&registers(&slots, 0, 1), |_, _| panic!("disabled read")).unwrap();
        }
    }

    #[test]
    fn video_uses_limited_range_and_correct_chroma_planes() {
        assert_eq!(yuv565(16, 128, 128), 0);
        assert_eq!(yuv565(235, 128, 128), 0xffff);
        assert_eq!(yuv565(81, 90, 240), 0xf800);
        let rgb = composite(&video_registers(), |address, n| {
            Ok(vec![
                match address {
                    0x1000..=0x1007 => 81,
                    0x2000..=0x2001 => 90,
                    0x3000..=0x3001 => 240,
                    _ => panic!("unexpected plane"),
                };
                n
            ])
        })
        .unwrap();
        assert_eq!(&rgb[..12], &[255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0]);
        assert_eq!(&rgb[WIDTH * 3..WIDTH * 3 + 3], &[255, 0, 0]);
        assert_eq!(&rgb[12..15], &[0, 0, 0]);
    }

    #[test]
    fn video_crop_scale_and_negative_position_preserve_source_coordinates() {
        let mut r = video_registers();
        put(&mut r, VIDEO_SOURCE_START, (2 << 16) | 2);
        put(&mut r, VIDEO_SOURCE_END, (6 << 16) | 4);
        put(&mut r, VIDEO_STRIDES, 8 | (4 << 16));
        put(&mut r, VIDEO_SCALE, 0x08000800);
        put(&mut r, VIDEO_SIZE, (8 << 16) | 4);
        put(&mut r, VIDEO_POSITION, 0xffffffff);
        let rgb = composite(&r, |address, n| {
            Ok(match address {
                0x1012 | 0x101a => {
                    assert_eq!(n, 4);
                    vec![16, 235, 16, 235]
                }
                0x2005 | 0x3005 => {
                    assert_eq!(n, 2);
                    vec![128; 2]
                }
                _ => panic!("unexpected cropped read {address:x}"),
            })
        })
        .unwrap();
        assert_eq!(
            &rgb[..12],
            &[0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0]
        );
        assert_eq!(&rgb[3 * WIDTH * 3..3 * WIDTH * 3 + 3], &[0, 0, 0]);
    }

    #[test]
    fn transparent_rgb_reveals_video_and_next_frame_starts_black() {
        let mut r = video_registers();
        put(&mut r, CONTROL, (VIDEO_ENABLE | LAYER0_ENABLE) as u32);
        for (offset, value) in [
            (SLOT_STRIDE, 4),
            (SLOT_FORMAT, 7 << 8),
            (SLOT_SOURCE, 0x4000),
            (SLOT_SIZE, (1 << 16) | 1),
        ] {
            put(&mut r, SLOT0 + offset, value);
        }
        let rgb = composite(&r, |address, n| {
            Ok(if address == 0x4000 {
                vec![255, 255, 255, 0]
            } else {
                vec![if address < 0x2000 { 235 } else { 128 }; n]
            })
        })
        .unwrap();
        assert_eq!(&rgb[..3], &[255, 255, 255]);
        let rgb = composite(&registers(&[], 0, 1), |_, _| panic!("no active windows")).unwrap();
        assert!(rgb.iter().all(|&v| v == 0));
    }

    #[test]
    fn invalid_video_is_skipped_without_reads() {
        for (at, value) in [
            (CONTROL, 0),
            (VIDEO_MODE, 0),
            (VIDEO_PLANE0, 0),
            (VIDEO_SOURCE_END, 0),
            (VIDEO_SIZE, 0),
        ] {
            let mut r = video_registers();
            put(&mut r, at, value);
            composite(&r, |_, _| panic!("invalid video read")).unwrap();
        }
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
