// SPDX-License-Identifier: GPL-3.0-only

use super::{
    protocol::{encode, *},
    screen::SELECTED_TEXT_VIEW,
    *,
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

type Request = (u8, u8, u16, u16, usize);

#[derive(Default)]
struct Device {
    info: Vec<u8>,
    /// Readable regions: base address and contents.
    memory: Vec<(u32, Vec<u8>)>,
    screens: VecDeque<Vec<u8>>,
    /// Replies to other IN requests, by request.
    replies: Vec<(u8, Vec<u8>)>,
    requests: Vec<Request>,
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Device>>);

impl Fake {
    fn new(info: Vec<u8>) -> Self {
        let fake = Self::default();
        fake.0.borrow_mut().info = info;
        fake
    }

    fn reply(&self, request: u8, bytes: Vec<u8>) {
        self.0.borrow_mut().replies.push((request, bytes));
    }

    fn map(&self, base: u32, bytes: Vec<u8>) {
        self.0.borrow_mut().memory.push((base, bytes));
    }

    fn debugger(&self) -> Debugger {
        let debugger = Debugger::with_transport(Box::new(self.clone())).unwrap();
        self.0.borrow_mut().requests.clear();
        debugger
    }

    fn requests(&self, request: u8) -> Vec<Request> {
        let device = self.0.borrow();
        device
            .requests
            .iter()
            .copied()
            .filter(|r| r.1 == request)
            .collect()
    }
}

impl Transport for Fake {
    fn read(&mut self, ty: u8, req: u8, val: u16, idx: u16, data: &mut [u8]) -> Result<usize> {
        let mut device = self.0.borrow_mut();
        device.requests.push((ty, req, val, idx, data.len()));
        assert_eq!(ty, VENDOR_IN);
        let reply = match req {
            INFO => device.info.clone(),
            READ => {
                let address = u32::from(val) | u32::from(idx) << 16;
                match device
                    .memory
                    .iter()
                    .find_map(|(base, bytes)| bytes.get(address.checked_sub(*base)? as usize..))
                {
                    Some(bytes) => bytes.to_vec(),
                    None => {
                        return Err(crate::Error::Usb {
                            operation: "debug request",
                            source: rusb::Error::Pipe,
                        })
                    }
                }
            }
            SCREEN_DATA => device.screens.pop_front().expect("Unexpected SCREEN_DATA"),
            other => device
                .replies
                .iter()
                .find(|(request, _)| *request == other)
                .map(|(_, bytes)| bytes.clone())
                .unwrap_or_else(|| panic!("Unexpected IN request {other:#x}")),
        };
        let n = reply.len().min(data.len());
        data[..n].copy_from_slice(&reply[..n]);
        Ok(n)
    }

    fn write(&mut self, ty: u8, req: u8, val: u16, idx: u16, data: &[u8]) -> Result<usize> {
        assert_eq!(ty, VENDOR_OUT);
        self.0
            .borrow_mut()
            .requests
            .push((ty, req, val, idx, data.len()));
        Ok(data.len())
    }
}

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn info_reply_is_parsed_and_checked() {
    let mut b = encode::info(512);
    for (at, v) in [
        (8, 0x0205),
        (12, 42),
        (24, 20 << 20),
        (28, 3 << 20),
        (32, 0x0800_1000),
    ] {
        put32(&mut b, at, v);
    }
    for (i, v) in [0, 4, 8, 0x2c, 0x30, 0x34].into_iter().enumerate() {
        put16(&mut b, 36 + i * 2, v);
    }
    assert_eq!(
        Info::parse(&b).unwrap(),
        Info {
            version: PROTOCOL_VERSION,
            firmware_id: 0x0205,
            core_build: 42,
            max_transfer: 512,
            display_base: 0x3890_0000,
            heap_total: 20 << 20,
            heap_free: 3 << 20,
            extensions: 0x0800_1000,
            record: RecordLayout {
                next: 0,
                owner: 4,
                file: 8,
                live: 0x2c,
                peak: 0x30,
                failure: 0x34,
            },
            clock_hz: 1_000_000,
            extension_name: 0,
        }
    );

    let mut bad_magic = encode::info(512);
    bad_magic[0] = b'X';
    let mut bad_version = encode::info(512);
    put16(&mut bad_version, 4, PROTOCOL_VERSION + 1);
    for (reply, message) in [
        (bad_magic, "is the extension enabled"),
        (
            encode::info(512)[..INFO_SIZE - 1].to_vec(),
            "is the extension enabled",
        ),
        (bad_version, "protocol 3"),
        (encode::info(32), "Implausible transfer limit"),
        (encode::info(0x10001), "Implausible transfer limit"),
    ] {
        let error = Debugger::with_transport(Box::new(Fake::new(reply)))
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn reads_are_split_at_the_transfer_limit() {
    let fake = Fake::new(encode::info(64));
    let bytes: Vec<u8> = (0..150).map(|i| i as u8).collect();
    fake.map(0x0801_fff0, bytes.clone());
    let mut debugger = fake.debugger();

    assert_eq!(debugger.read(0x0801_fff0, 150).unwrap(), bytes);
    assert_eq!(
        fake.requests(READ),
        [
            (VENDOR_IN, READ, 0xfff0, 0x0801, 64),
            (VENDOR_IN, READ, 0x0030, 0x0802, 64),
            (VENDOR_IN, READ, 0x0070, 0x0802, 22),
        ]
    );
    assert_eq!(debugger.read_u32(0x0801_fff4).unwrap(), 0x0706_0504);

    let error = debugger.read(0x0801_fff0, 151).unwrap_err().to_string();
    assert!(error.contains("0x08020070: 22 of 23 bytes"), "{error}");
    let error = debugger.read(0x0900_0000, 4).unwrap_err().to_string();
    assert!(
        error.contains("0x09000000: USB debug request: Pipe error"),
        "{error}"
    );
}

#[test]
fn writes_set_the_address_then_send_one_word_per_request() {
    let fake = Fake::new(encode::info(64));
    let mut debugger = fake.debugger();

    debugger
        .write(0x0800_0004, &[1, 2, 3, 4, 5, 6, 7, 8])
        .unwrap();
    assert_eq!(
        fake.0.borrow().requests,
        [
            (VENDOR_OUT, SET_ADDRESS, 0x0004, 0x0800, 0),
            (VENDOR_OUT, WRITE32, 0x0201, 0x0403, 0),
            (VENDOR_OUT, WRITE32, 0x0605, 0x0807, 0),
        ]
    );

    fake.0.borrow_mut().requests.clear();
    assert!(debugger.write(0x0800_0002, &[0; 4]).is_err());
    assert!(debugger.write(0x0800_0000, &[0; 6]).is_err());
    assert!(fake.0.borrow().requests.is_empty());
}

#[test]
fn scroll_steps_eight_positions_per_row_in_one_touch() {
    let fake = Fake::new(encode::info(64));
    let mut debugger = fake.debugger();

    let start = Instant::now();
    debugger.scroll(-3).unwrap();
    assert!(start.elapsed() >= STEP_INTERVAL * 2);
    let touch = |position| (VENDOR_OUT, INPUT, INPUT_TOUCH, position, 0);
    let lift = (VENDOR_OUT, INPUT, INPUT_LIFT, 0, 0);
    assert_eq!(
        fake.0.borrow().requests,
        [touch(0), touch(88), touch(80), touch(72), lift]
    );
}

#[test]
fn screen_waits_for_the_capture_and_describes_it() {
    let fake = Fake::new(encode::info(512));
    let text = |text: &str, top, left, right, view_class| DrawnText {
        text: text.into(),
        top,
        left,
        bottom: top + 21,
        right,
        color: 0,
        view_class,
    };
    let mut pending = encode::screen(&[]);
    put16(&mut pending, 4, 0);
    let captured = encode::screen(&[
        text("Settings", 2, 6, 113, 1),
        text("About", 23, 5, 154, 1),
        text("Shuffle", 48, 5, 154, SELECTED_TEXT_VIEW),
        text("Off", 48, 107, 154, 1),
    ]);
    fake.0.borrow_mut().screens.extend([pending, captured]);
    let mut debugger = fake.debugger();

    let screen = debugger.screen().unwrap();
    assert_eq!(fake.requests(SCREEN), [(VENDOR_OUT, SCREEN, 0, 0, 0)]);
    assert_eq!(fake.requests(SCREEN_DATA).len(), 2);
    assert_eq!((screen.state, screen.title.as_str()), ("list", "Settings"));
    assert_eq!(screen.selected, Some(1));
    assert_eq!(
        screen.rows[1],
        Row {
            index: 1,
            label: "Shuffle".into(),
            value: Some("Off".into()),
            selected: true,
        }
    );

    let mut bad = encode::screen(&[text("Settings", 2, 6, 113, 1)]);
    put32(&mut bad, 8, 13);
    fake.0.borrow_mut().screens.push_back(bad);
    assert!(debugger.screen().is_err());
}

#[test]
fn heap_walks_the_extension_records() {
    const HEAD: u32 = 0x0800_1000;
    let mut info = encode::info(512);
    for (at, v) in [(24, 0x0140_0000), (28, 0x0030_0000), (32, HEAD)] {
        put32(&mut info, at, v);
    }
    for (i, v) in [0, 4, 8, 0x2c, 0x30, 0x34].into_iter().enumerate() {
        put16(&mut info, 36 + i * 2, v);
    }
    let record = |next: u32, owner: u32, file: &str, live: u32, peak: u32, failure: u8| {
        let mut b = vec![0; 0x38];
        for (at, v) in [(0, next), (4, owner), (0x2c, live), (0x30, peak)] {
            put32(&mut b, at, v);
        }
        b[8..8 + file.len()].copy_from_slice(file.as_bytes());
        b[0x34] = failure;
        b
    };
    let fake = Fake::new(info);
    fake.map(HEAD, 0x0800_2000u32.to_le_bytes().to_vec());
    fake.map(
        0x0800_2000,
        record(0x0800_3000, 1, "usb-debug", 4096, 8192, 0),
    );
    fake.map(0x0800_3000, record(0, 2, &"x".repeat(33), 0, 512, 3));
    let mut debugger = fake.debugger();

    let heap = debugger.heap().unwrap();
    assert_eq!(fake.requests(INFO).len(), 1);
    assert_eq!(
        fake.requests(READ),
        [
            (VENDOR_IN, READ, 0x1000, 0x0800, 4),
            (VENDOR_IN, READ, 0x2000, 0x0800, 0x38),
            (VENDOR_IN, READ, 0x3000, 0x0800, 0x38),
        ]
    );
    assert_eq!(
        heap,
        HeapReport {
            total: 0x0140_0000,
            free: 0x0030_0000,
            extensions: vec![
                ExtensionHeap {
                    file: "usb-debug".into(),
                    owner: 1,
                    live: 4096,
                    peak: 8192,
                    failure: 0,
                },
                ExtensionHeap {
                    file: "x".repeat(33),
                    owner: 2,
                    live: 0,
                    peak: 512,
                    failure: 3,
                },
            ],
        }
    );

    put32(&mut fake.0.borrow_mut().info, 32, 0);
    assert!(debugger.heap().unwrap().extensions.is_empty());
    assert_eq!(fake.requests(READ).len(), 3);
}

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn samples_carry_their_time() {
    let mut b = words(&[2, 5]);
    b.extend(words(&[
        0x0800_1000,
        0x0800_2000,
        100,
        0x0800_1004,
        0x0800_2004,
        110,
    ]));
    let (samples, dropped) = parse_samples(&b).unwrap();
    assert_eq!(dropped, 5);
    assert_eq!(
        samples[1],
        Sample {
            pc: 0x0800_1004,
            lr: 0x0800_2004,
            time: 110
        }
    );
    assert!(parse_samples(&b[..b.len() - 1]).is_err());
}

#[test]
fn instruments_parse_and_send_their_arguments() {
    let fake = Fake::new(encode::info(512));
    fake.reply(CLOCK, words(&[123_456]));
    fake.reply(BOOT, words(&[2_000_000, 0, 2_400_000, 7, 8]));
    fake.reply(HEAP_STATS, words(&[10, 300, 200, 400, 20 << 20]));
    fake.reply(
        STACK,
        words(&[0x0900_0000, 0x2_0000, 0x0901_f000, 0x0901_e000]),
    );
    let mut debugger = fake.debugger();

    assert_eq!(debugger.clock().unwrap(), 123_456);
    let boot = debugger.boot_times().unwrap();
    assert_eq!(
        (
            boot.entry,
            boot.first_idle,
            boot.flush_at_boot,
            boot.flush_written
        ),
        (2_000_000, 2_400_000, 7, 8)
    );
    let heap = debugger.heap_stats().unwrap();
    assert_eq!((heap.samples, heap.min, heap.max), (10, 200, 400));
    let stack = debugger.stack(true).unwrap();
    assert_eq!(stack.used(), 0x2000);
    assert_eq!(fake.requests(STACK)[0].2, 1);

    debugger.heap_watch(50).unwrap();
    debugger.fault(Fault::SoftwareAbort).unwrap();
    debugger.flush(Some(25)).unwrap();
    debugger.flush(None).unwrap();
    debugger.reset().unwrap();
    assert_eq!(fake.requests(HEAP_WATCH)[0].2, 50);
    assert_eq!(fake.requests(FAULT)[0].2, 2);
    let flushes = fake.requests(FLUSH);
    assert_eq!((flushes[0].2, flushes[0].3), (FLUSH_THEN_RESET, 25));
    assert_eq!((flushes[1].2, flushes[1].3), (0, 0));
    assert_eq!(fake.requests(RESET).len(), 1);
}

#[test]
fn inconsistent_stack_replies_are_refused() {
    assert!(parse_stack(&words(&[0x0900_0000, 0x2_0000, 0x0901_f000, 0x0902_0000])).is_err());
    assert!(parse_stack(&words(&[0x0900_0000, 0x2_0000])).is_err());
}

#[test]
fn site_ids_match_the_core() {
    // From the generated rv_sites.h.
    assert_eq!(site_id("TextView_DrawString_entry"), 0x5258_7f80);
    assert_eq!(site_id("UI_DisplayPreparedLayout_call"), 0xd21f_3e6a);
}

#[test]
fn timings_and_events_parse() {
    let mut b = words(&[2]);
    b.extend(words(&[
        0x5258_7f80,
        124,
        50_583,
        174,
        1279,
        0x7986_a911,
        24,
        417,
        2,
        117,
    ]));
    let timings = parse_timings(&b).unwrap();
    assert_eq!(timings.len(), 2);
    assert_eq!(
        timings[0],
        SiteTiming {
            site: 0x5258_7f80,
            calls: 124,
            total: 50_583,
            min: 174,
            max: 1279
        }
    );
    assert!(parse_timings(&words(&[3, 0, 0, 0, 0, 0])).is_err());

    let mut b = words(&[2, 1]);
    b.extend(words(&[
        100,
        event_kind::FLUSH_BEGIN,
        44,
        200,
        event_kind::FLUSH_END,
        1,
    ]));
    let (events, dropped) = parse_event_log(&b).unwrap();
    assert_eq!(dropped, 1);
    assert_eq!(
        events[1],
        Event {
            time: 200,
            kind: event_kind::FLUSH_END,
            argument: 1
        }
    );
    assert!(parse_event_log(&b[..b.len() - 4]).is_err());
}

#[test]
fn extension_writes_the_name_then_waits_for_the_result() {
    let mut info = encode::info(512);
    put32(&mut info, 52, 0x0be4_c208);
    let fake = Fake::new(info);
    fake.reply(EXTENSION_RESULT, words(&[2, 6]));
    let mut debugger = fake.debugger();
    assert_eq!(debugger.set_extension("hello_world", false).unwrap(), 6);
    let address = fake.requests(SET_ADDRESS);
    assert_eq!((address[0].2, address[0].3), (0xc208, 0x0be4));
    let words_sent = fake.requests(WRITE32);
    assert_eq!(words_sent.len(), NAME_BYTES / 4);
    assert_eq!(
        (words_sent[0].2, words_sent[0].3),
        (u16::from_le_bytes(*b"he"), u16::from_le_bytes(*b"ll"))
    );
    assert_eq!(fake.requests(EXTENSION)[0].2, 0);
    assert!(debugger
        .set_extension(&"x".repeat(NAME_BYTES), true)
        .is_err());
    assert!(debugger.set_extension("", true).is_err());
}

#[test]
fn hold_and_site_timing_send_their_arguments() {
    let fake = Fake::new(encode::info(512));
    let mut debugger = fake.debugger();
    debugger.hold(Hold::Unlocked).unwrap();
    debugger.time_site(Some(0x5258_7f80)).unwrap();
    debugger.time_site(None).unwrap();
    debugger.events(64).unwrap();
    assert_eq!(fake.requests(HOLD)[0].2, 2);
    let sites = fake.requests(TIME_SITE);
    assert_eq!((sites[0].2, sites[0].3), (0x7f80, 0x5258));
    assert_eq!((sites[1].2, sites[1].3), (0, 0));
    assert_eq!(fake.requests(EVENTS)[0].2, 64);
}
