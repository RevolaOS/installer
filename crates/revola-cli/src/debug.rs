// SPDX-License-Identifier: GPL-3.0-only

use super::{
    ensure_new_output, parse_selector, print_json, read_bounded, write_new_output, CliResult,
};
use revola_device::{
    event_kind, png, result_name, site_id, BootTimes, Button, Debugger, DeviceSelector, Fault,
    HeapReport, Hold, Sample, Screen, SAMPLES_HEADER, SAMPLE_BYTES, SCREEN_HEIGHT, SCREEN_WIDTH,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    io::{self, Write},
    thread::sleep,
    time::{Duration, Instant},
};

const TOP_FUNCTIONS: usize = 20;
const TOP_CALLERS: usize = 3;
/// How long a reset iPod gets to boot and answer again.
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(90);
/// FLUSH counts its reset delay in units of 100 µs.
const FLUSH_DELAY_UNIT_US: u32 = 100;
/// Sites TIME_SITE can time at once.
const TIMED_SITES: usize = 8;

#[derive(Debug, PartialEq, Eq)]
enum Location {
    Address(u32),
    Symbol(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Until {
    Change,
    Stable,
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Info,
    Peek {
        at: Location,
        length: usize,
        out: Option<String>,
    },
    Poke {
        at: Location,
        value: u32,
    },
    Screenshot {
        output: String,
    },
    Press(Button),
    Scroll(i32),
    Heap {
        watch: Option<Duration>,
    },
    Perf {
        seconds: u32,
        rate: u16,
    },
    Screen,
    Select,
    Menu,
    Wait {
        until: Until,
        timeout: Duration,
    },
    Goto(String),
    Clock,
    Boot,
    HeapWatch {
        period_ms: u16,
        seconds: u32,
    },
    Stack {
        refill: bool,
    },
    Fault(Fault),
    Reset,
    Flush {
        /// In units of 100 µs.
        reset_after: Option<u16>,
    },
    TimeSite {
        /// Row names from sites.def or numeric ids.
        sites: Vec<String>,
        seconds: u32,
    },
    Events {
        seconds: u32,
        entries: u16,
    },
    Hold(Hold),
    Extensions,
    Extension {
        file: String,
        enabled: bool,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Args {
    action: Action,
    selector: Option<DeviceSelector>,
    symbols: Option<String>,
}

fn number(value: &str) -> Result<u32, String> {
    let result = match value.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => value.parse(),
    };
    result.map_err(|_| format!("Invalid number: {value}"))
}

fn location(value: &str) -> Location {
    match number(value) {
        Ok(address) => Location::Address(address),
        Err(_) => Location::Symbol(value.into()),
    }
}

pub(super) fn parse(args: &[String]) -> Result<Args, String> {
    let (command, rest) = args
        .split_first()
        .ok_or("Missing debug command; use revola debug --help")?;
    let mut positional = Vec::new();
    let mut options = BTreeMap::new();
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        if !arg.starts_with("--") {
            positional.push(arg.as_str());
            continue;
        }
        let value = rest
            .next()
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| format!("{arg} needs a value"))?;
        if options.insert(arg.as_str(), value.as_str()).is_some() {
            return Err(format!("Duplicate argument: {arg}"));
        }
    }
    let allowed: &[&str] = match command.as_str() {
        "peek" => &["--out", "--symbols"],
        "poke" => &["--symbols"],
        "heap" => &["--watch"],
        "perf" => &["--seconds", "--rate", "--symbols"],
        "wait" => &["--until", "--timeout"],
        "heap-watch" => &["--period", "--seconds"],
        "flush" => &["--reset-after"],
        "time-site" => &["--seconds"],
        "events" => &["--seconds", "--entries"],
        _ => &[],
    };
    if let Some(option) = options
        .keys()
        .find(|option| **option != "--device" && !allowed.contains(option))
    {
        return Err(format!("Unknown debug {command} argument: {option}"));
    }
    let positive = |option: &str, default: u32| -> Result<u32, String> {
        match options.get(option) {
            None => Ok(default),
            Some(value) => match number(value)? {
                0 => Err(format!("{option} must be nonzero")),
                n => Ok(n),
            },
        }
    };
    let action = match (command.as_str(), positional.as_slice()) {
        ("info", []) => Action::Info,
        ("peek", [at, length]) => Action::Peek {
            at: location(at),
            length: match number(length)? {
                0 => return Err("LENGTH must be nonzero".into()),
                n => n as usize,
            },
            out: options.get("--out").map(|s| s.to_string()),
        },
        ("poke", [at, value]) => Action::Poke {
            at: location(at),
            value: number(value)?,
        },
        ("screenshot", [output]) => Action::Screenshot {
            output: output.to_string(),
        },
        ("press", [button]) => Action::Press(button.parse()?),
        ("scroll", [steps]) => Action::Scroll(
            steps
                .parse()
                .map_err(|_| format!("Invalid scroll steps: {steps}"))?,
        ),
        ("heap", []) => Action::Heap {
            watch: match options.get("--watch") {
                None => None,
                Some(_) => Some(Duration::from_secs(positive("--watch", 1)?.into())),
            },
        },
        ("perf", []) => Action::Perf {
            seconds: positive("--seconds", 5)?,
            rate: u16::try_from(positive("--rate", 100)?).map_err(|_| "--rate is at most 65535")?,
        },
        ("screen", []) => Action::Screen,
        ("select", []) => Action::Select,
        ("menu", []) => Action::Menu,
        ("wait", []) => Action::Wait {
            until: match options.get("--until").copied() {
                None | Some("change") => Until::Change,
                Some("stable") => Until::Stable,
                Some(other) => return Err(format!("--until {other}: use change or stable")),
            },
            timeout: Duration::from_secs(positive("--timeout", 5)?.into()),
        },
        ("goto", [path]) => Action::Goto(path.to_string()),
        ("time-site", sites) if !sites.is_empty() && sites.len() <= TIMED_SITES => {
            Action::TimeSite {
                sites: sites.iter().map(|s| s.to_string()).collect(),
                seconds: positive("--seconds", 10)?,
            }
        }
        ("events", []) => Action::Events {
            seconds: positive("--seconds", 10)?,
            entries: u16::try_from(positive("--entries", 256)?)
                .map_err(|_| "--entries is at most 65535")?,
        },
        ("hold", [state]) => Action::Hold(state.parse()?),
        ("extensions", []) => Action::Extensions,
        ("extension", [file, state]) => Action::Extension {
            file: file.to_string(),
            enabled: match *state {
                "on" => true,
                "off" => false,
                other => return Err(format!("extension {file} {other}: use on or off")),
            },
        },
        ("clock", []) => Action::Clock,
        ("boot", []) => Action::Boot,
        ("heap-watch", []) => Action::HeapWatch {
            period_ms: u16::try_from(positive("--period", 50)?)
                .map_err(|_| "--period is at most 65535")?,
            seconds: positive("--seconds", 10)?,
        },
        ("stack", []) => Action::Stack { refill: false },
        ("stack", ["refill"]) => Action::Stack { refill: true },
        ("fault", [kind]) => Action::Fault(kind.parse()?),
        ("reset", []) => Action::Reset,
        ("flush", []) => Action::Flush {
            reset_after: match options.get("--reset-after") {
                None => None,
                Some(value) => Some(
                    u16::try_from(number(value)?.div_ceil(FLUSH_DELAY_UNIT_US))
                        .map_err(|_| "--reset-after is at most 6553500 µs")?,
                ),
            },
        },
        _ => return Err("Invalid debug command; use revola debug --help".into()),
    };
    let symbols = options.get("--symbols").map(|s| s.to_string());
    if let Action::Peek {
        at: Location::Symbol(name),
        ..
    }
    | Action::Poke {
        at: Location::Symbol(name),
        ..
    } = &action
    {
        if symbols.is_none() {
            return Err(format!(
                "{name} is not a number; pass --symbols LD_FILE to use names"
            ));
        }
    }
    Ok(Args {
        action,
        selector: options
            .get("--device")
            .map(|s| parse_selector(s))
            .transpose()?,
        symbols,
    })
}

/// `NAME = 0xADDRESS;` lines of the generated linker script, sorted by address.
fn parse_symbols(text: &str) -> Result<Vec<(u32, String)>, String> {
    let mut symbols = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("/*") {
            continue;
        }
        let parsed = line
            .strip_suffix(';')
            .and_then(|line| line.split_once('='))
            .and_then(|(name, value)| Some((number(value.trim()).ok()?, name.trim().to_string())));
        match parsed {
            Some(symbol) => symbols.push(symbol),
            None => {
                return Err(format!(
                    "Symbols line {}: expected NAME = 0xADDRESS;",
                    line_number + 1
                ))
            }
        }
    }
    symbols.sort();
    Ok(symbols)
}

fn load_symbols(path: &Option<String>) -> CliResult<Vec<(u32, String)>> {
    match path {
        Some(path) => Ok(parse_symbols(&String::from_utf8(read_bounded(
            path,
            4 << 20,
        )?)?)?),
        None => Ok(Vec::new()),
    }
}

fn resolve(at: &Location, symbols: &[(u32, String)]) -> CliResult<u32> {
    match at {
        Location::Address(address) => Ok(*address),
        Location::Symbol(name) => symbols
            .iter()
            .find(|(_, symbol)| symbol == name)
            .map(|(address, _)| *address)
            .ok_or_else(|| format!("No symbol {name} in the symbols file").into()),
    }
}

/// The nearest symbol at or below `address`, or the address itself.
fn function_name(address: u32, symbols: &[(u32, String)]) -> String {
    match symbols.partition_point(|(start, _)| *start <= address) {
        0 => format!("{address:#010x}"),
        i => symbols[i - 1].1.clone(),
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Caller {
    name: String,
    samples: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Function {
    name: String,
    samples: usize,
    callers: Vec<Caller>,
}

fn ranked(counts: HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked
}

/// Samples grouped by function, most frequent first, each with its top callers.
fn profile(samples: &[Sample], symbols: &[(u32, String)]) -> Vec<Function> {
    let mut functions: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for sample in samples {
        *functions
            .entry(function_name(sample.pc, symbols))
            .or_default()
            .entry(function_name(sample.lr, symbols))
            .or_default() += 1;
    }
    let mut out: Vec<Function> = functions
        .into_iter()
        .map(|(name, callers)| Function {
            name,
            samples: callers.values().sum(),
            callers: ranked(callers)
                .into_iter()
                .take(TOP_CALLERS)
                .map(|(name, samples)| Caller { name, samples })
                .collect(),
        })
        .collect();
    out.sort_by(|a, b| b.samples.cmp(&a.samples).then_with(|| a.name.cmp(&b.name)));
    out.truncate(TOP_FUNCTIONS);
    out
}

/// Sample for `seconds`, draining the ring twice per fill period.
fn collect(
    debugger: &mut Debugger,
    seconds: u32,
    rate: u16,
    capacity: usize,
) -> CliResult<(Vec<Sample>, u32)> {
    let pause = Duration::from_secs_f64(capacity as f64 / f64::from(rate) / 2.0)
        .max(Duration::from_millis(10));
    let deadline = Instant::now() + Duration::from_secs(seconds.into());
    let (mut samples, mut dropped) = (Vec::new(), 0u32);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        sleep(pause.min(left));
        loop {
            let (drained, lost) = debugger.samples()?;
            dropped = dropped.saturating_add(lost);
            let full = drained.len() >= capacity;
            samples.extend(drained);
            if !full {
                break;
            }
        }
    }
    Ok((samples, dropped))
}

fn hexdump(address: u32, bytes: &[u8]) -> String {
    let mut out = String::new();
    for (i, line) in bytes.chunks(16).enumerate() {
        let _ = write!(out, "{:08x} ", address.wrapping_add(i as u32 * 16));
        for k in 0..16 {
            match line.get(k) {
                Some(byte) => _ = write!(out, " {byte:02x}"),
                None => out.push_str("   "),
            }
        }
        out.push_str("  |");
        out.extend(line.iter().map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            }
        }));
        out.push_str("|\n");
    }
    out
}

fn write_screen(screen: &Screen, out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "{} ({})", screen.title, screen.state)?;
    let width = screen
        .rows
        .iter()
        .map(|r| r.label.chars().count())
        .max()
        .unwrap_or(0);
    for row in &screen.rows {
        let marker = if row.selected { '>' } else { ' ' };
        match &row.value {
            Some(value) => writeln!(out, "{marker} {:width$}  {value}", row.label)?,
            None => writeln!(out, "{marker} {}", row.label)?,
        }
    }
    for text in screen.preview.iter().chain(&screen.text) {
        writeln!(out, "  {text}")?;
    }
    Ok(())
}

fn write_heap_line(elapsed: Duration, heap: &HeapReport, out: &mut impl Write) -> io::Result<()> {
    write!(
        out,
        "{:7.1} s  Apple {} of {} free",
        elapsed.as_secs_f64(),
        heap.free,
        heap.total
    )?;
    for e in &heap.extensions {
        write!(out, "  {} {} (peak {})", e.file, e.live, e.peak)?;
    }
    writeln!(out)
}

fn seconds(ticks: u32, hz: u32) -> f64 {
    f64::from(ticks) / f64::from(hz.max(1))
}

/// Seconds from `from` to `to` on the device clock, which wraps.
fn seconds_between(from: u32, to: u32, hz: u32) -> f64 {
    f64::from(to.wrapping_sub(from) as i32) / f64::from(hz.max(1))
}

fn print_boot(boot: &BootTimes, hz: u32, json: bool, out: &mut impl Write) -> CliResult<()> {
    if json {
        return print_json(&serde_json::json!({"boot": boot, "hz": hz}));
    }
    let point = |ticks: u32| {
        if ticks == 0 {
            "not recorded".to_string()
        } else {
            format!("{:.3} s", seconds(ticks, hz))
        }
    };
    writeln!(out, "Clock at usb-debug's entry: {}", point(boot.entry))?;
    writeln!(out, "First main menu: {}", point(boot.main_menu))?;
    writeln!(out, "First UI timer tick: {}", point(boot.first_idle))?;
    writeln!(
        out,
        "FLUSH sequence: {} at boot, {} last written",
        boot.flush_at_boot, boot.flush_written
    )?;
    Ok(())
}

fn print_screen(screen: &Screen, json: bool) -> CliResult<()> {
    if json {
        print_json(screen)
    } else {
        Ok(write_screen(screen, &mut io::stdout().lock())?)
    }
}

pub(super) fn run(args: Args, json: bool) -> CliResult<u8> {
    let symbols = load_symbols(&args.symbols)?;
    if let Action::Screenshot { output }
    | Action::Peek {
        out: Some(output), ..
    } = &args.action
    {
        ensure_new_output(output)?;
    }
    if let Action::Peek { at, .. } | Action::Poke { at, .. } = &args.action {
        resolve(at, &symbols)?;
    }
    let mut debugger = Debugger::open(args.selector)?;
    let mut out = io::stdout().lock();
    match args.action {
        Action::Info => {
            let info = debugger.info();
            if json {
                print_json(info)?;
            } else {
                writeln!(
                    out,
                    "Protocol: {}\nFirmware ID: {:#010x}\nCore build: {:#010x}\nMax transfer: {} bytes\nDisplay pipe: {:#010x}\nApple heap: {} of {} bytes free",
                    info.version,
                    info.firmware_id,
                    info.core_build,
                    info.max_transfer,
                    info.display_base,
                    info.heap_free,
                    info.heap_total
                )?;
            }
        }
        Action::Peek {
            at,
            length,
            out: file,
        } => {
            let address = resolve(&at, &symbols)?;
            let bytes = debugger.read(address, length)?;
            if let Some(file) = &file {
                write_new_output(file, &bytes)?;
            }
            if json {
                let mut value = serde_json::json!({"address": address, "length": length});
                match file {
                    Some(file) => value["output"] = file.into(),
                    None => {
                        value["hex"] = bytes
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                            .into()
                    }
                }
                print_json(&value)?;
            } else if let Some(file) = file {
                writeln!(out, "Wrote {length} bytes from {address:#010x} to {file}")?;
            } else {
                out.write_all(hexdump(address, &bytes).as_bytes())?;
            }
        }
        Action::Poke { at, value } => {
            let address = resolve(&at, &symbols)?;
            debugger.write(address, &value.to_le_bytes())?;
            let readback = debugger.read_u32(address)?;
            if json {
                print_json(
                    &serde_json::json!({"address": address, "value": value, "readback": readback}),
                )?;
            } else {
                writeln!(
                    out,
                    "{address:#010x}: wrote {value:#010x}, reads {readback:#010x}"
                )?;
            }
            if readback != value {
                return Ok(2);
            }
        }
        Action::Screenshot { output } => {
            let rgb = debugger.screenshot()?;
            write_new_output(&output, &png(&rgb, SCREEN_WIDTH, SCREEN_HEIGHT))?;
            if json {
                print_json(
                    &serde_json::json!({"output": output, "width": SCREEN_WIDTH, "height": SCREEN_HEIGHT}),
                )?;
            } else {
                writeln!(
                    out,
                    "Wrote {SCREEN_WIDTH}x{SCREEN_HEIGHT} screenshot to {output}"
                )?;
            }
        }
        Action::Press(button) => {
            debugger.press(button)?;
            let name = format!("{button:?}").to_lowercase();
            if json {
                print_json(&serde_json::json!({"button": name}))?;
            } else {
                writeln!(out, "Pressed {name}")?;
            }
        }
        Action::Scroll(steps) => {
            debugger.scroll(steps)?;
            if json {
                print_json(&serde_json::json!({"steps": steps}))?;
            } else {
                writeln!(out, "Scrolled {steps} steps")?;
            }
        }
        Action::Heap {
            watch: Some(interval),
        } => {
            let start = Instant::now();
            loop {
                let heap = debugger.heap()?;
                if json {
                    serde_json::to_writer(
                        &mut out,
                        &serde_json::json!({"seconds": start.elapsed().as_secs_f64(), "heap": heap}),
                    )?;
                    writeln!(out)?;
                } else {
                    write_heap_line(start.elapsed(), &heap, &mut out)?;
                }
                out.flush()?;
                sleep(interval);
            }
        }
        Action::Heap { watch: None } => {
            let heap = debugger.heap()?;
            if json {
                print_json(&heap)?;
            } else {
                writeln!(
                    out,
                    "Apple heap: {} of {} bytes free",
                    heap.free, heap.total
                )?;
                writeln!(
                    out,
                    "{:<33} {:>10} {:>10} {:>8}",
                    "Extension", "Live", "Peak", "Failure"
                )?;
                for e in &heap.extensions {
                    writeln!(
                        out,
                        "{:<33} {:>10} {:>10} {:>8}",
                        e.file, e.live, e.peak, e.failure
                    )?;
                }
            }
        }
        Action::Perf { seconds, rate } => {
            let capacity = (debugger.info().max_transfer as usize - SAMPLES_HEADER) / SAMPLE_BYTES;
            let entries = u16::try_from(capacity).unwrap_or(u16::MAX);
            debugger.profile_start(rate, entries)?;
            let collected = collect(&mut debugger, seconds, rate, capacity);
            let stopped = debugger.profile_stop();
            let (samples, dropped) = collected?;
            stopped?;
            let functions = profile(&samples, &symbols);
            if json {
                print_json(&serde_json::json!({
                    "seconds": seconds,
                    "rate": rate,
                    "samples": samples.len(),
                    "dropped": dropped,
                    "functions": functions,
                }))?;
            } else {
                writeln!(
                    out,
                    "{} samples over {seconds} s at {rate} Hz, {dropped} dropped",
                    samples.len()
                )?;
                let total = samples.len().max(1) as f64;
                for f in &functions {
                    writeln!(
                        out,
                        "{:6.2}% {:>7}  {}",
                        f.samples as f64 * 100.0 / total,
                        f.samples,
                        f.name
                    )?;
                    for c in &f.callers {
                        writeln!(out, "         {:>7}    from {}", c.samples, c.name)?;
                    }
                }
            }
        }
        Action::Screen => print_screen(&debugger.screen()?, json)?,
        Action::Select => print_screen(&debugger.press_verified(Button::Center)?, json)?,
        Action::Menu => print_screen(&debugger.press_verified(Button::Menu)?, json)?,
        Action::Wait { until, timeout } => {
            let (screen, met) = match until {
                Until::Change => {
                    let before = debugger.screen()?;
                    let after = debugger.wait_change(&before, timeout)?;
                    let changed = after != before;
                    (after, changed)
                }
                Until::Stable => (debugger.wait_stable(timeout)?, true),
            };
            print_screen(&screen, json)?;
            if !met {
                return Ok(2);
            }
        }
        Action::Clock => {
            let now = debugger.clock()?;
            let hz = debugger.info().clock_hz;
            if json {
                print_json(&serde_json::json!({"ticks": now, "hz": hz}))?;
            } else {
                writeln!(out, "{now} ticks ({} s at {hz} Hz)", seconds(now, hz))?;
            }
        }
        Action::Boot => {
            let boot = debugger.boot_times()?;
            let hz = debugger.info().clock_hz;
            print_boot(&boot, hz, json, &mut out)?;
        }
        Action::HeapWatch { period_ms, seconds } => {
            debugger.heap_watch(period_ms)?;
            sleep(Duration::from_secs(seconds.into()));
            let stats = debugger.heap_stats();
            let stopped = debugger.heap_watch(0);
            let stats = stats?;
            stopped?;
            if json {
                print_json(&stats)?;
            } else {
                writeln!(
                    out,
                    "{} samples every {period_ms} ms: free {} now, {} lowest, {} highest, of {}",
                    stats.samples, stats.latest, stats.min, stats.max, stats.total
                )?;
            }
        }
        Action::Stack { refill } => {
            let stack = debugger.stack(refill)?;
            if json {
                print_json(
                    &serde_json::json!({"stack": stack, "used": stack.used(), "refill": refill}),
                )?;
            } else {
                writeln!(
                    out,
                    "UI stack {:#010x}+{:#x}: {} of {} bytes used at the deepest (filled below {:#010x}){}",
                    stack.base,
                    stack.size,
                    stack.used(),
                    stack.size,
                    stack.filled_top,
                    if refill { "; refilled" } else { "" }
                )?;
            }
        }
        Action::Fault(fault) => {
            debugger.fault(fault)?;
            let name = format!("{fault:?}");
            if json {
                print_json(&serde_json::json!({"fault": name}))?;
            } else {
                writeln!(out, "Raised {name} on the UI task")?;
            }
        }
        Action::Reset => {
            debugger.reset()?;
            drop(debugger);
            let mut debugger = Debugger::reconnect(None, RECONNECT_TIMEOUT)?;
            let boot = debugger.boot_times()?;
            let hz = debugger.info().clock_hz;
            print_boot(&boot, hz, json, &mut out)?;
        }
        Action::Flush { reset_after: None } => {
            debugger.flush(None)?;
            // The flush runs at the next UI timer tick.
            sleep(Duration::from_millis(200));
            let boot = debugger.boot_times()?;
            if json {
                print_json(&serde_json::json!({"sequence": boot.flush_written}))?;
            } else {
                writeln!(out, "Flushed with sequence number {}", boot.flush_written)?;
            }
        }
        Action::Flush {
            reset_after: Some(delay),
        } => {
            let before = debugger.boot_times()?.flush_written;
            debugger.flush(Some(delay))?;
            drop(debugger);
            let mut debugger = Debugger::reconnect(None, RECONNECT_TIMEOUT)?;
            let found = debugger.boot_times()?.flush_at_boot;
            let survived = if found == before + 1 {
                "newest"
            } else if found == before {
                "previous"
            } else {
                "neither"
            };
            if json {
                print_json(&serde_json::json!({
                    "reset_after_us": u32::from(delay) * FLUSH_DELAY_UNIT_US,
                    "written": before + 1,
                    "found": found,
                    "survived": survived,
                }))?;
            } else {
                writeln!(
                    out,
                    "Reset {} µs after the flush began; wrote sequence {}, found {found} at boot: {survived} copy",
                    u32::from(delay) * FLUSH_DELAY_UNIT_US,
                    before + 1
                )?;
            }
            if survived == "neither" {
                return Ok(2);
            }
        }
        Action::TimeSite { sites, seconds } => {
            let ids: Vec<(String, u32)> = sites
                .iter()
                .map(|s| (s.clone(), number(s).unwrap_or_else(|_| site_id(s))))
                .collect();
            let timings = debugger.time_sites(
                &ids.iter().map(|(_, id)| *id).collect::<Vec<_>>(),
                Duration::from_secs(seconds.into()),
            )?;
            let rows: Vec<_> = ids
                .iter()
                .map(|(name, id)| (name, timings.iter().find(|t| t.site == *id)))
                .collect();
            if json {
                let value: Vec<_> = rows
                    .iter()
                    .map(|(name, t)| serde_json::json!({"site": name, "timing": t}))
                    .collect();
                print_json(&value)?;
            } else {
                for (name, t) in &rows {
                    match t {
                        Some(t) => writeln!(
                            out,
                            "{name}: {} calls, {} µs total, {:.1} µs mean, {} min, {} max",
                            t.calls,
                            t.total,
                            f64::from(t.total) / f64::from(t.calls.max(1)),
                            t.min,
                            t.max
                        )?,
                        None => writeln!(
                            out,
                            "{name}: not timed (unknown site, no filter possible, or an exclusive handler holds it)"
                        )?,
                    }
                }
            }
            if rows.iter().any(|(_, t)| t.is_none()) {
                return Ok(2);
            }
        }
        Action::Events { seconds, entries } => {
            debugger.events(entries)?;
            let start = debugger.clock()?;
            let hz = debugger.info().clock_hz;
            let deadline = Instant::now() + Duration::from_secs(seconds.into());
            let mut dropped = 0u32;
            let mut all = Vec::new();
            let result = loop {
                sleep(Duration::from_millis(250));
                match debugger.event_log() {
                    Ok((events, lost)) => {
                        dropped = dropped.saturating_add(lost);
                        all.extend(events);
                    }
                    Err(e) => break Err(e),
                }
                if Instant::now() > deadline {
                    break Ok(());
                }
            };
            let stopped = debugger.events(0);
            result?;
            stopped?;
            if json {
                print_json(
                    &serde_json::json!({"hz": hz, "start": start, "dropped": dropped, "events": all}),
                )?;
            } else {
                for e in &all {
                    writeln!(
                        out,
                        "{:10.6} s  {} {:#x}",
                        seconds_between(start, e.time, hz),
                        event_kind::name(e.kind),
                        e.argument
                    )?;
                }
                writeln!(out, "{} events, {dropped} dropped", all.len())?;
            }
        }
        Action::Hold(hold) => {
            debugger.hold(hold)?;
            let name = format!("{hold:?}").to_lowercase();
            if json {
                print_json(&serde_json::json!({"hold": name}))?;
            } else {
                writeln!(out, "Hold {name}")?;
            }
        }
        Action::Extensions => {
            let heap = debugger.heap()?;
            if json {
                print_json(&heap.extensions)?;
            } else {
                for e in &heap.extensions {
                    writeln!(
                        out,
                        "{:<33} {:>10} bytes  failure {}",
                        e.file, e.live, e.failure
                    )?;
                }
            }
        }
        Action::Extension { file, enabled } => {
            let code = debugger.set_extension(&file, enabled)?;
            if json {
                print_json(
                    &serde_json::json!({"file": file, "enabled": enabled, "result": result_name(code)}),
                )?;
            } else {
                writeln!(
                    out,
                    "{file} {}: {}",
                    if enabled { "on" } else { "off" },
                    result_name(code)
                )?;
            }
            if code != 0 {
                return Ok(2);
            }
        }
        Action::Goto(path) => {
            let (steps, screen) = debugger.goto(&path)?;
            if json {
                print_json(&serde_json::json!({"steps": steps, "screen": screen}))?;
            } else {
                for step in &steps {
                    writeln!(out, "{step}")?;
                }
                write_screen(&screen, &mut out)?;
            }
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_line(s: &str) -> Result<Args, String> {
        parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn commands_parse_positionals_and_their_own_options() {
        let peek = parse_line("peek 0x08001000 64 --out dump.bin --device 1:2").unwrap();
        assert_eq!(
            peek.action,
            Action::Peek {
                at: Location::Address(0x0800_1000),
                length: 64,
                out: Some("dump.bin".into()),
            }
        );
        assert_eq!(peek.selector, Some(DeviceSelector { bus: 1, address: 2 }));
        assert_eq!(parse_line("scroll -3").unwrap().action, Action::Scroll(-3));
        assert_eq!(
            parse_line("heap --watch 2").unwrap().action,
            Action::Heap {
                watch: Some(Duration::from_secs(2))
            }
        );
        assert_eq!(
            parse_line("press select").unwrap().action,
            Action::Press(Button::Center)
        );
        assert_eq!(
            parse_line("perf --rate 250").unwrap().action,
            Action::Perf {
                seconds: 5,
                rate: 250
            }
        );
        assert_eq!(
            parse_line("wait --until stable --timeout 2")
                .unwrap()
                .action,
            Action::Wait {
                until: Until::Stable,
                timeout: Duration::from_secs(2)
            }
        );
        assert_eq!(
            parse_line("poke App_object 0x1 --symbols osos.ld")
                .unwrap()
                .action,
            Action::Poke {
                at: Location::Symbol("App_object".into()),
                value: 1
            }
        );
        assert_eq!(
            parse_line("flush --reset-after 250").unwrap().action,
            Action::Flush {
                reset_after: Some(3)
            }
        );
        assert_eq!(
            parse_line("heap-watch --period 20").unwrap().action,
            Action::HeapWatch {
                period_ms: 20,
                seconds: 10
            }
        );
        assert_eq!(
            parse_line("stack refill").unwrap().action,
            Action::Stack { refill: true }
        );
        assert_eq!(
            parse_line("fault abort").unwrap().action,
            Action::Fault(Fault::SoftwareAbort)
        );
        assert_eq!(
            parse_line("time-site TextView_DrawString_entry 0x7986a911 --seconds 3")
                .unwrap()
                .action,
            Action::TimeSite {
                sites: vec!["TextView_DrawString_entry".into(), "0x7986a911".into()],
                seconds: 3
            }
        );
        assert_eq!(
            parse_line("extension hello_world off").unwrap().action,
            Action::Extension {
                file: "hello_world".into(),
                enabled: false
            }
        );
        assert_eq!(
            parse_line("hold release").unwrap().action,
            Action::Hold(Hold::Release)
        );
        assert_eq!(
            parse_line("events --entries 64").unwrap().action,
            Action::Events {
                seconds: 10,
                entries: 64
            }
        );
        for line in [
            "",
            "info extra",
            "time-site",
            "time-site a b c d e f g h i",
            "extension hello_world maybe",
            "hold sideways",
            "flush --reset-after 6553600",
            "stack deep",
            "fault prefetch",
            "heap-watch --period 0",
            "peek 0x1000",
            "peek 0x1000 0",
            "peek App_object 4",
            "poke 0x1000 0x100000000",
            "screen --out x",
            "press shuffle",
            "perf --seconds 0",
            "heap --watch 0",
            "perf --rate 70000",
            "wait --until never",
            "heap --device 1:2 --device 1:3",
            "goto",
            "reboot",
        ] {
            assert!(parse_line(line).is_err(), "{line}");
        }
    }

    #[test]
    fn symbols_resolve_names_and_attribute_addresses() {
        let symbols = parse_symbols(
            "/* SPDX-License-Identifier: GPL-3.0-only */\nB_Function = 0x08002000;\nA_Function = 0x08001000;\n",
        )
        .unwrap();
        assert_eq!(
            resolve(&Location::Symbol("B_Function".into()), &symbols).unwrap(),
            0x0800_2000
        );
        assert!(resolve(&Location::Symbol("C".into()), &symbols).is_err());
        assert_eq!(function_name(0x0800_1ffe, &symbols), "A_Function");
        assert_eq!(function_name(0x0800_2000, &symbols), "B_Function");
        assert_eq!(function_name(0x0800_0ffc, &symbols), "0x08000ffc");
        assert!(parse_symbols("A = nowhere;").is_err());

        let sample = |pc, lr| Sample { pc, lr, time: 0 };
        let functions = profile(
            &[
                sample(0x0800_2004, 0x0800_1010),
                sample(0x0800_2008, 0x0800_1010),
                sample(0x0800_2008, 0x0800_0100),
                sample(0x0800_1004, 0x0800_2010),
            ],
            &symbols,
        );
        assert_eq!(functions[0].name, "B_Function");
        assert_eq!(functions[0].samples, 3);
        assert_eq!(
            functions[0].callers,
            [
                Caller {
                    name: "A_Function".into(),
                    samples: 2
                },
                Caller {
                    name: "0x08000100".into(),
                    samples: 1
                },
            ]
        );
        assert_eq!(functions[1].name, "A_Function");
    }

    #[test]
    fn hexdump_pads_the_last_line() {
        assert_eq!(
            hexdump(0x0800_0000, b"iPod\x00\x01"),
            format!(
                "08000000  69 50 6f 64 00 01{}  |iPod..|\n",
                " ".repeat(10 * 3)
            )
        );
    }
}
