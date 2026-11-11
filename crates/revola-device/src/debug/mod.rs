// SPDX-License-Identifier: GPL-3.0-only
//! Host side of the USB Debug extension: memory, input, screenshots, heap,
//! PC samples and screen descriptions over vendor control requests on EP0.
//!
//! Only device-recipient control transfers are used and no interface is
//! claimed, so the mass-storage driver can keep the iPod's interfaces.

mod display;
mod protocol;
mod screen;
#[cfg(test)]
mod tests;

pub use display::{png, HEIGHT, WIDTH};
pub use protocol::{
    event_kind, result_name, site_id, BootTimes, DrawnText, Event, HeapStats, Info, RecordLayout,
    Sample, SiteTiming, StackMark, EVENT_BYTES, EVENT_HEADER, SAMPLES_HEADER, SAMPLE_BYTES,
};
pub use screen::{Row, Screen};

use crate::{
    dfu::Transport,
    invalid,
    usb::{candidates, context, usb_error},
    DeviceSelector, Result,
};
use protocol::*;
use serde::Serialize;
use std::{
    thread::sleep,
    time::{Duration, Instant},
};

const NORMAL_MODE: u16 = 0x1261;
const TIMEOUT: Duration = Duration::from_secs(1);
/// Wheel positions per scroll step; the wheel task needs at least 6 to move.
const STEP_POSITIONS: u16 = 8;
const WHEEL_POSITIONS: u16 = 96;
/// Time the wheel task needs to see each injected position on its own; two
/// positions it sees together count as a touch at the second.
const STEP_PAUSE: Duration = Duration::from_millis(100);
/// List accumulators speed up motion that follows earlier motion within 400 ms
/// and count it once per row only after a 400 ms pause, so steps go that far
/// apart.
const STEP_INTERVAL: Duration = Duration::from_millis(450);
const PRESS_HOLD: Duration = Duration::from_millis(80);
const ROOT_TITLE: &str = "iPod";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Center = 0x1,
    Next = 0x2,
    Previous = 0x4,
    Play = 0x8,
    Menu = 0x10,
}

impl std::str::FromStr for Button {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "center" | "select" => Self::Center,
            "next" | "forward" => Self::Next,
            "previous" | "prev" | "back" => Self::Previous,
            "play" | "pause" => Self::Play,
            "menu" => Self::Menu,
            _ => {
                return Err(format!(
                    "Unknown button {s}; use menu, select, play, previous or next"
                ))
            }
        })
    }
}

/// What FAULT raises on the UI task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    UndefinedInstruction = 1,
    SoftwareAbort = 2,
    DataAbort = 3,
}

impl std::str::FromStr for Fault {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "undefined" => Ok(Self::UndefinedInstruction),
            "abort" => Ok(Self::SoftwareAbort),
            "data" => Ok(Self::DataAbort),
            _ => Err(format!("Unknown fault {s}; use undefined, abort or data")),
        }
    }
}

/// The hold switch as HOLD sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// Back to the real switch.
    Release = 0,
    Locked = 1,
    Unlocked = 2,
}

impl std::str::FromStr for Hold {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "on" | "locked" => Ok(Self::Locked),
            "off" | "unlocked" => Ok(Self::Unlocked),
            "release" => Ok(Self::Release),
            _ => Err(format!("Unknown hold state {s}; use on, off or release")),
        }
    }
}

/// Bytes an extension holds, from the core's records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExtensionHeap {
    pub file: String,
    pub owner: u32,
    pub live: u32,
    pub peak: u32,
    pub failure: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HeapReport {
    pub total: u32,
    pub free: u32,
    pub extensions: Vec<ExtensionHeap>,
}

struct Libusb(rusb::DeviceHandle<rusb::Context>);

impl Transport for Libusb {
    fn read(&mut self, ty: u8, req: u8, val: u16, idx: u16, data: &mut [u8]) -> Result<usize> {
        self.0
            .read_control(ty, req, val, idx, data, TIMEOUT)
            .map_err(|e| usb_error(request_name(req), e))
    }

    fn write(&mut self, ty: u8, req: u8, val: u16, idx: u16, data: &[u8]) -> Result<usize> {
        self.0
            .write_control(ty, req, val, idx, data, TIMEOUT)
            .map_err(|e| usb_error(request_name(req), e))
    }
}

pub struct Debugger {
    transport: Box<dyn Transport>,
    info: Info,
    /// When the last injected wheel step was sent.
    last_step: Option<Instant>,
}

impl Debugger {
    /// Open the selected iPod, or the only one in normal mode, and read INFO.
    pub fn open(selector: Option<DeviceSelector>) -> Result<Self> {
        let context = context()?;
        let mut found: Vec<_> = candidates(&context)?
            .into_iter()
            .filter(|(_, info)| {
                info.product_id == NORMAL_MODE && selector.is_none_or(|s| s == info.selector)
            })
            .collect();
        if found.len() != 1 {
            return Err(invalid(format!(
                "Expected one iPod in normal mode (05ac:1261), found {}",
                found.len()
            )));
        }
        let handle = found
            .remove(0)
            .0
            .open()
            .map_err(|e| usb_error("open (check USB permissions)", e))?;
        Self::with_transport(Box::new(Libusb(handle)))
    }

    pub(crate) fn with_transport(mut transport: Box<dyn Transport>) -> Result<Self> {
        let info = read_info(transport.as_mut())?;
        Ok(Self {
            transport,
            info,
            last_step: None,
        })
    }

    pub fn info(&self) -> &Info {
        &self.info
    }

    /// Read INFO again; heap figures in it are current as of the request.
    pub fn refresh(&mut self) -> Result<&Info> {
        self.info = read_info(self.transport.as_mut())?;
        Ok(&self.info)
    }

    pub fn read(&mut self, address: u32, length: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(length);
        while out.len() < length {
            let at = address
                .checked_add(out.len() as u32)
                .ok_or_else(|| invalid("Read wraps the address space"))?;
            let n = (length - out.len()).min(self.info.max_transfer as usize);
            let (value, index) = split(at);
            let mut chunk = vec![0; n];
            // The device stalls READ outside the memory it allows.
            let got = self
                .transport
                .read(VENDOR_IN, READ, value, index, &mut chunk)
                .map_err(|e| {
                    invalid(format!(
                        "Read at {at:#010x}: {e}; the address may be outside readable memory"
                    ))
                })?;
            if got != n {
                return Err(invalid(format!(
                    "Read at {at:#010x}: {got} of {n} bytes; the address may be outside readable memory"
                )));
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    pub fn read_u32(&mut self, address: u32) -> Result<u32> {
        let b = self.read(address, 4)?;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }

    /// Write whole words from `address` (word aligned).
    pub fn write(&mut self, address: u32, bytes: &[u8]) -> Result<()> {
        if !address.is_multiple_of(4) || !bytes.len().is_multiple_of(4) {
            return Err(invalid("Writes are whole aligned words"));
        }
        let (value, index) = split(address);
        self.out(SET_ADDRESS, value, index)?;
        for word in bytes.as_chunks::<4>().0 {
            let (value, index) = split(u32::from_le_bytes(*word));
            self.out(WRITE32, value, index)?;
        }
        Ok(())
    }

    pub fn buttons(&mut self, mask: u16) -> Result<()> {
        self.out(INPUT, INPUT_BUTTONS, mask)
    }

    pub fn press(&mut self, button: Button) -> Result<()> {
        self.buttons(button as u16)?;
        sleep(PRESS_HOLD);
        self.buttons(0)?;
        sleep(STEP_PAUSE);
        Ok(())
    }

    /// Turn the wheel by `steps` list rows: positive is clockwise (down a list).
    pub fn scroll(&mut self, steps: i32) -> Result<()> {
        if steps == 0 {
            return Ok(());
        }
        let mut position: u16 = 0;
        self.out(INPUT, INPUT_TOUCH, position)?;
        sleep(STEP_PAUSE);
        for _ in 0..steps.unsigned_abs() {
            if let Some(last) = self.last_step {
                sleep(STEP_INTERVAL.saturating_sub(last.elapsed()));
            }
            position = if steps > 0 {
                (position + STEP_POSITIONS) % WHEEL_POSITIONS
            } else {
                (position + WHEEL_POSITIONS - STEP_POSITIONS) % WHEEL_POSITIONS
            };
            self.out(INPUT, INPUT_TOUCH, position)?;
            self.last_step = Some(Instant::now());
        }
        sleep(STEP_PAUSE);
        self.out(INPUT, INPUT_LIFT, 0)?;
        sleep(STEP_PAUSE);
        Ok(())
    }

    /// The composited screen as 320x240 RGB.
    pub fn screenshot(&mut self) -> Result<Vec<u8>> {
        let registers = self.read(self.info.display_base, display::REGISTER_BYTES)?;
        display::composite(&registers, |address, n| self.read(address, n))
    }

    pub fn heap(&mut self) -> Result<HeapReport> {
        let info = self.refresh()?.clone();
        let layout = info.record;
        let span = [
            layout.next,
            layout.owner,
            layout.live,
            layout.peak,
            layout.failure,
        ]
        .into_iter()
        .map(|o| o as usize + 4)
        .chain([layout.file as usize + 33])
        .max()
        .unwrap();
        let mut extensions = Vec::new();
        let mut record = if info.extensions == 0 {
            0
        } else {
            self.read_u32(info.extensions)?
        };
        while record != 0 && extensions.len() < 256 {
            let b = self.read(record, span)?;
            let word =
                |o: u16| u32::from_le_bytes(b[o as usize..o as usize + 4].try_into().unwrap());
            let name = &b[layout.file as usize..layout.file as usize + 33];
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            extensions.push(ExtensionHeap {
                file: String::from_utf8_lossy(&name[..end]).into_owned(),
                owner: word(layout.owner),
                live: word(layout.live),
                peak: word(layout.peak),
                failure: b[layout.failure as usize],
            });
            record = word(layout.next);
        }
        Ok(HeapReport {
            total: info.heap_total,
            free: info.heap_free,
            extensions,
        })
    }

    /// Start PC sampling at `rate` Hz into a ring of `entries` samples.
    pub fn profile_start(&mut self, rate: u16, entries: u16) -> Result<()> {
        if rate == 0 || entries == 0 {
            return Err(invalid("Rate and ring size must be nonzero"));
        }
        self.out(PROFILE, rate, entries)
    }

    pub fn profile_stop(&mut self) -> Result<()> {
        self.out(PROFILE, 0, 0)
    }

    /// Drain samples; returns them and how many the ring dropped.
    pub fn samples(&mut self) -> Result<(Vec<Sample>, u32)> {
        let mut b = vec![0; self.info.max_transfer as usize];
        let n = self.transport.read(VENDOR_IN, SAMPLES, 0, 0, &mut b)?;
        parse_samples(&b[..n])
    }

    /// The device's clock, in `info().clock_hz` ticks; wraps.
    pub fn clock(&mut self) -> Result<u32> {
        parse_clock(&self.read_reply(CLOCK, 0, 4)?)
    }

    pub fn boot_times(&mut self) -> Result<BootTimes> {
        parse_boot(&self.read_reply(BOOT, 0, 20)?)
    }

    /// Sample Apple's free heap every `period_ms` from now on; 0 stops.
    pub fn heap_watch(&mut self, period_ms: u16) -> Result<()> {
        self.out(HEAP_WATCH, period_ms, 0)
    }

    pub fn heap_stats(&mut self) -> Result<HeapStats> {
        parse_heap_stats(&self.read_reply(HEAP_STATS, 0, 20)?)
    }

    /// The UI stack's watermark; with `refill`, the device refills the stack
    /// at its next UI timer tick, after this reply.
    pub fn stack(&mut self, refill: bool) -> Result<StackMark> {
        parse_stack(&self.read_reply(STACK, refill as u16, 16)?)
    }

    pub fn fault(&mut self, fault: Fault) -> Result<()> {
        self.out(FAULT, fault as u16, 0)
    }

    /// Warm reset about 50 ms after the reply.
    pub fn reset(&mut self) -> Result<()> {
        self.out(RESET, 0, 0)
    }

    /// Store the next FLUSH sequence number and flush the settings; with
    /// `reset_after` (100 µs units), reset that long after the flush began, or
    /// as soon as it returns for 0.
    pub fn flush(&mut self, reset_after: Option<u16>) -> Result<()> {
        match reset_after {
            None => self.out(FLUSH, 0, 0),
            Some(delay) => self.out(FLUSH, FLUSH_THEN_RESET, delay),
        }
    }

    /// Time a core site from the device's next UI timer tick; `None` stops
    /// timing every site.
    pub fn time_site(&mut self, site: Option<u32>) -> Result<()> {
        let (value, index) = split(site.unwrap_or(0));
        self.out(TIME_SITE, value, index)
    }

    pub fn timings(&mut self) -> Result<Vec<SiteTiming>> {
        parse_timings(&self.read_reply(TIMINGS, 0, 4 + 8 * 20)?)
    }

    /// Start the event log with room for `entries`; 0 stops it.
    pub fn events(&mut self, entries: u16) -> Result<()> {
        self.out(EVENTS, entries, 0)
    }

    /// Drain the event log; returns the events and how many it dropped.
    pub fn event_log(&mut self) -> Result<(Vec<Event>, u32)> {
        let n = self.info.max_transfer as usize;
        parse_event_log(&self.read_reply(EVENT_LOG, 0, n)?)
    }

    pub fn hold(&mut self, hold: Hold) -> Result<()> {
        self.out(HOLD, hold as u16, 0)
    }

    /// Turn the extension in `file`.rvx on or off as Settings > Extensions
    /// does; returns its rv_result code.
    pub fn set_extension(&mut self, file: &str, enabled: bool) -> Result<i32> {
        if file.is_empty() || file.len() >= NAME_BYTES || file.contains('\0') {
            return Err(invalid(format!(
                "Extension file names have 1 to {} bytes",
                NAME_BYTES - 1
            )));
        }
        let mut name = [0u8; NAME_BYTES];
        name[..file.len()].copy_from_slice(file.as_bytes());
        self.write(self.info.extension_name, &name)?;
        self.out(EXTENSION, enabled as u16, 0)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            sleep(Duration::from_millis(50));
            match parse_extension_result(&self.read_reply(EXTENSION_RESULT, 0, 8)?)? {
                ExtensionResult::Done(code) => return Ok(code),
                _ if Instant::now() > deadline => {
                    return Err(invalid("The extension change did not finish within 5 s"))
                }
                _ => {}
            }
        }
    }

    /// Open the iPod again once it is back after a reset.
    pub fn reconnect(selector: Option<DeviceSelector>, timeout: Duration) -> Result<Self> {
        // Let the old connection drop first.
        sleep(Duration::from_secs(1));
        let deadline = Instant::now() + timeout;
        loop {
            match Self::open(selector) {
                Ok(debugger) => return Ok(debugger),
                Err(e) if Instant::now() > deadline => {
                    return Err(invalid(format!(
                        "The iPod did not answer within {} s of the reset: {e}",
                        timeout.as_secs()
                    )))
                }
                Err(_) => sleep(Duration::from_millis(500)),
            }
        }
    }

    /// Capture the strings of the next full redraw and describe the screen.
    pub fn drawn(&mut self) -> Result<Vec<DrawnText>> {
        self.out(SCREEN, 0, 0)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut b = vec![0; self.info.max_transfer as usize];
        loop {
            sleep(Duration::from_millis(30));
            let n = self.transport.read(VENDOR_IN, SCREEN_DATA, 0, 0, &mut b)?;
            if let Some(texts) = parse_screen(&b[..n])? {
                return Ok(texts);
            }
            if Instant::now() > deadline {
                return Err(invalid("The UI did not redraw within 3 s"));
            }
        }
    }

    pub fn screen(&mut self) -> Result<Screen> {
        Ok(screen::describe(&self.drawn()?))
    }

    /// Press a button until the screen changes (up to three tries); returns the
    /// screen once it has settled, as a push draws more than one frame.
    pub fn press_verified(&mut self, button: Button) -> Result<Screen> {
        let before = self.screen()?;
        for _ in 0..3 {
            self.press(button)?;
            let after = self.wait_change(&before, Duration::from_secs(2))?;
            if after != before {
                return self.wait_stable(Duration::from_secs(2));
            }
        }
        Err(invalid(format!(
            "{button:?}: screen unchanged after 3 presses"
        )))
    }

    pub fn wait_change(&mut self, from: &Screen, timeout: Duration) -> Result<Screen> {
        let deadline = Instant::now() + timeout;
        loop {
            let now = self.screen()?;
            if now != *from || Instant::now() > deadline {
                return Ok(now);
            }
            sleep(Duration::from_millis(100));
        }
    }

    pub fn wait_stable(&mut self, timeout: Duration) -> Result<Screen> {
        let deadline = Instant::now() + timeout;
        let mut last = self.screen()?;
        loop {
            sleep(Duration::from_millis(200));
            let now = self.screen()?;
            if now == last || Instant::now() > deadline {
                return Ok(now);
            }
            last = now;
        }
    }

    /// Navigate by row labels from the main menu, e.g. "Settings/About".
    /// Returns the steps taken and the final screen.
    pub fn goto(&mut self, path: &str) -> Result<(Vec<String>, Screen)> {
        let mut steps = Vec::new();
        let mut screen = self.screen()?;
        for _ in 0..12 {
            if screen.title == ROOT_TITLE {
                break;
            }
            screen = self.press_verified(Button::Menu)?;
            steps.push(format!("menu -> '{}'", screen.title));
        }
        if screen.title != ROOT_TITLE {
            return Err(invalid("Could not reach the main menu"));
        }
        for label in path.split('/').filter(|s| !s.is_empty()) {
            self.scroll_to(label, &mut steps)?;
            screen = self.press_verified(Button::Center)?;
            steps.push(format!("select '{label}' -> '{}'", screen.title));
        }
        Ok((steps, screen))
    }

    fn scroll_to(&mut self, label: &str, steps: &mut Vec<String>) -> Result<Screen> {
        let mut screen = self.screen()?;
        let mut seen_bottom = false;
        for _ in 0..200 {
            let selected = screen.selected.unwrap_or(0);
            let direction: i32 = match screen.row(label) {
                Some(row) if row.selected => return Ok(screen),
                Some(row) => (row.index as i32 - selected as i32).signum(),
                // Not visible: search down, then up from the bottom.
                None if !seen_bottom => 1,
                None => -1,
            };
            let before = screen.clone();
            self.scroll(direction)?;
            // The UI applies the step after the request; the preview pane beside
            // the list changes on its own, so only the rows count. No change
            // means the end of the list.
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                screen = self.screen()?;
                if screen.rows != before.rows || Instant::now() > deadline {
                    break;
                }
                sleep(Duration::from_millis(100));
            }
            steps.push(format!(
                "{} -> {}",
                if direction > 0 { "down" } else { "up" },
                screen
                    .selected
                    .and_then(|i| screen.rows.get(i))
                    .map_or("?", |r| r.label.as_str())
            ));
            if screen.rows == before.rows {
                if seen_bottom && direction < 0 {
                    break;
                }
                seen_bottom = true;
            }
        }
        Err(invalid(format!("No row '{label}' on '{}'", screen.title)))
    }

    fn read_reply(&mut self, request: u8, value: u16, length: usize) -> Result<Vec<u8>> {
        let mut b = vec![0; length];
        let n = self.transport.read(VENDOR_IN, request, value, 0, &mut b)?;
        b.truncate(n);
        Ok(b)
    }

    fn out(&mut self, request: u8, value: u16, index: u16) -> Result<()> {
        self.transport
            .write(VENDOR_OUT, request, value, index, &[])?;
        Ok(())
    }
}

fn read_info(transport: &mut dyn Transport) -> Result<Info> {
    let mut b = [0; INFO_SIZE];
    let n = transport.read(VENDOR_IN, INFO, 0, 0, &mut b)?;
    Info::parse(&b[..n])
}
