// SPDX-License-Identifier: GPL-3.0-only

use revola_device::{
    check_saved, discover, CheckReport, Cleanup, DeviceSelector, Event, Session, SysCfg,
};
mod bundle;
mod debug;
mod decrypt;
mod firmware;
mod progress;
mod upload;
use progress::Progress;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    env,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    process::ExitCode,
};

const HELP: &str = r#"Revola device tools

Usage:
  revola upload --input FILE --destination /PATH
    (--helper BUILD_DIRECTORY | --bundle DIRECTORY --key PUBLIC_KEY_FILE)
    [--overwrite] [--device BUS:ADDRESS] [--json]
  revola bundle export --spec FILE --base DIR --out NEW_DIR [--json]
  revola bundle pack --directory DIR --out NEW_ZIP [--json]
  revola bundle inspect --directory DIR --key PUBLIC_KEY_FILE [--json]
  revola bundle fetch --url HTTPS_MANIFEST_URL --key PUBLIC_KEY_FILE
    --cache DIR [--sha256 MANIFEST_SHA256] [--json]
  revola bundle sign --directory DIR --seed SECRET_SEED_FILE [--json]
  revola bundle assemble --directory DIR --key PUBLIC_KEY_FILE
    --inputs DECRYPTED_APPLE_DIR --nor BACKUP_FILE --out NEW_DIR [--json]
  revola bundle apply-recipe --recipe FILE --data FILE
    --inputs DECRYPTED_APPLE_DIR --out NEW_FILE [--nor BACKUP_FILE] [--json]
  revola firmware inspect --ipsw FILE [--json]
  revola firmware extract --ipsw FILE --output NEW_IMG1 [--json]
  revola firmware unpack --ipsw FILE --out NEW_DIR [--json]
  revola firmware extract-efi --input PLAINTEXT_LOADER --out NEW_DIR [--json]
  revola firmware prepare --ipsw FILE --osos PLAINTEXT
    --aupd PLAINTEXT_BODY --out NEW_DIR [--json]
  revola firmware decrypt-nor --input BACKUP --output NEW_FILE
    [--device BUS:ADDRESS] [--json]
  revola devices [--json]
  revola dfu-state [--device BUS:ADDRESS] [--json]
  revola check [--device BUS:ADDRESS] [--json]
  revola nor-dump --output NEW_FILE [--device BUS:ADDRESS] [--json]
  revola check-files --bootrom FILE --syscfg FILE [--json]
  revola syscfg FILE [--json]

  revola decrypt --input FILE --output NEW_FILE
    [--skip-bytes BYTES] [--offset BYTES] [--size BYTES] [--chunk BYTES] [--iv HEX]
    [--reference FILE] [--device BUS:ADDRESS] [--json]

  revola debug COMMAND [--device BUS:ADDRESS] [--json]
    info
    peek ADDRESS|SYMBOL LENGTH [--out NEW_FILE] [--symbols LD_FILE]
    poke ADDRESS|SYMBOL VALUE [--symbols LD_FILE]
    screenshot NEW_PNG
    press menu|select|play|previous|next
    scroll STEPS
    heap [--watch SECONDS]
    perf [--seconds N] [--rate HZ] [--symbols LD_FILE]
    screen
    select | menu
    wait [--until change|stable] [--timeout SECONDS]
    goto PATH
    clock | boot
    heap-watch [--period MS] [--seconds N]
    stack [refill]
    fault undefined|abort
    reset
    flush [--reset-after MICROSECONDS]

upload streams through a 64 KiB buffer, verifies disk readback, then returns to DFU.
Use a signed --bundle with its trusted --key, or a development --helper directory. Existing destination
files require --overwrite; replacement happens after temporary-file verification.
Files may have any byte length up to 2 GiB minus 1 byte; missing parent directories are created.

decrypt reads the Classic IMG1 header and decrypts its entire encrypted body.
Output contains the plaintext body, including final AES block padding.
For raw ciphertext, --skip-bytes N skips N input bytes and decrypts the rest;
use --skip-bytes 0 for a file containing only ciphertext.
--offset starts inside the body; --size limits the bytes decrypted.
Offset and size must be multiples of 16. Numbers accept decimal or 0x.
CBC uses the preceding ciphertext block for interior offsets, or --iv
(32 hex digits, default zero) at body offset zero.
--chunk defaults to 512 (16..512, aligned to 16).
--reference compares against a plaintext body file at the same --offset.
Output paths must be new.

dfu-state opens the USB transport and reads GETSTATE without running a payload.

check uses 512-byte BootROM replies after a small ROM/transport preflight,
verifies the full 64 KiB fingerprint, then performs bounded
NOR SysCfg reads. Connect in BootROM DFU (blank screen);
recoverable DFU states are cleared before checks. Reset only if recovery fails.
Device tooling: iPod Classic 6G, 6.5G and 7G, with a verified BootROM.
Installation requires a bundle matching the checked device.

debug talks to an iPod running RevolaOS with the USB Debug extension enabled
(normal mode, 05ac:1261). It uses vendor control requests only and claims no interface.
peek prints a hexdump; poke writes one aligned 32-bit word and reads it back.
scroll STEPS is positive for clockwise (down a list).
heap --watch prints a line every SECONDS until interrupted (JSON lines with --json).
screen describes the strings drawn in the next redraw: title, rows, values, selection.
select and menu press until the screen changes, up to three times.
wait exits 2 if --until change saw no change within --timeout (default 5 s).
goto returns to the main menu, then selects each row label of PATH, e.g. Settings/About.
perf samples the PC at --rate (default 100 Hz) for --seconds (default 5) and
attributes each sample to the nearest preceding symbol in --symbols.
--symbols takes the generated NAME = 0xADDRESS; linker script, e.g. osos-2.0.5.ld.
clock prints the device's microsecond counter; boot prints it at points of the boot.
heap-watch samples Apple's free heap every --period (default 50 ms) for --seconds
(default 10) and prints the lowest, highest and latest figures.
stack prints how deep the UI stack went; refill restarts the mark afterwards.
fault raises a fault on the UI task to test the panic screen and crash recovery.
reset warm-resets the iPod, waits for it to answer again and prints boot.
flush stores a new sequence number and flushes the settings. --reset-after resets
that long after the flush began (0: when it returns), waits for the iPod, and
reports which copy survived; it exits 2 when neither did.

--device uses decimal BUS:ADDRESS from `revola devices`, not USB VID:PID.
Omit it when only one DFU device is connected.

check-files accepts saved SysCfg or a NOR dump beginning with SysCfg.
nor-dump checks the device, then reads/cross-checks the full 1 MiB NOR using
512-byte data replies, shifted reads, and slow edge reads.
--json suppresses text progress and emits one final result to stdout.
Otherwise live progress goes to stderr.
Exit codes: 0 success, 1 command/access/input error, 2 checks failed.
"#;

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Bundle(bundle::Args),
    Firmware(firmware::Args),
    Devices,
    DfuState {
        selector: Option<DeviceSelector>,
    },
    Check {
        selector: Option<DeviceSelector>,
    },
    CheckFiles {
        bootrom: String,
        syscfg: String,
    },
    Syscfg {
        path: String,
    },
    Decrypt(decrypt::Args),
    Upload(upload::Args),
    Debug(debug::Args),
    NorDump {
        output: String,
        selector: Option<DeviceSelector>,
    },
}

fn parse_options<'a>(
    args: &'a [String],
    allowed: &[&str],
) -> Result<BTreeMap<&'a str, String>, String> {
    let mut options = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(option) = rest.next() {
        if !allowed.contains(&option.as_str()) || options.contains_key(option.as_str()) {
            return Err(format!("Unknown or duplicate argument: {option}"));
        }
        let value = rest
            .next()
            .filter(|value| !value.starts_with('-'))
            .ok_or_else(|| format!("{option} needs a value"))?;
        options.insert(option.as_str(), value.clone());
    }
    Ok(options)
}

fn parse(args: &[String]) -> Result<(Command, bool), String> {
    let mut args = args.to_vec();
    let json_count = args.iter().filter(|s| s.as_str() == "--json").count();
    if json_count > 1 {
        return Err("Duplicate --json".into());
    }
    args.retain(|s| s != "--json");
    let json = json_count == 1;
    if args.len() == 2
        && matches!(
            args[0].as_str(),
            "devices"
                | "dfu-state"
                | "check"
                | "nor-dump"
                | "decrypt"
                | "check-files"
                | "syscfg"
                | "upload"
                | "bundle"
                | "firmware"
                | "debug"
        )
        && matches!(args[1].as_str(), "--help" | "-h")
    {
        return Ok((Command::Help, json));
    }
    let command = match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") if args.len() <= 1 => Command::Help,
        Some("devices") if args.len() == 1 => Command::Devices,
        Some("syscfg") if args.len() == 2 && !args[1].starts_with('-') => Command::Syscfg {
            path: args[1].clone(),
        },
        Some("check" | "nor-dump" | "dfu-state") => {
            let nor_dump = args[0] == "nor-dump";
            let allowed: &[&str] = if nor_dump {
                &["--device", "--output"]
            } else {
                &["--device"]
            };
            let mut options = parse_options(&args[1..], allowed)?;
            let selector = options
                .get("--device")
                .map(|value| parse_selector(value))
                .transpose()?;
            let output = options.remove("--output");
            if nor_dump {
                Command::NorDump {
                    output: output.ok_or("Missing --output NEW_FILE")?,
                    selector,
                }
            } else if args[0] == "dfu-state" {
                Command::DfuState { selector }
            } else {
                Command::Check { selector }
            }
        }
        Some("firmware") => Command::Firmware(firmware::parse(&args[1..])?),
        Some("bundle") => Command::Bundle(bundle::parse(&args[1..])?),
        Some("upload") => Command::Upload(upload::parse(&args[1..])?),
        Some("debug") => Command::Debug(debug::parse(&args[1..])?),
        Some("decrypt") => Command::Decrypt(decrypt::parse(&args[1..])?),
        Some("check-files") => {
            let mut options = parse_options(&args[1..], &["--bootrom", "--syscfg"])?;
            Command::CheckFiles {
                bootrom: options
                    .remove("--bootrom")
                    .ok_or("Missing --bootrom FILE")?,
                syscfg: options.remove("--syscfg").ok_or("Missing --syscfg FILE")?,
            }
        }
        _ => return Err("Invalid command; use revola --help".into()),
    };
    Ok((command, json))
}

type CliResult<T> = Result<T, Box<dyn std::error::Error>>;

fn open_session(selector: Option<DeviceSelector>) -> CliResult<Session> {
    Session::open(selector).map_err(|error| match error {
        revola_device::Error::DeviceCount(_) => {
            format!("{error}; use `revola devices` and --device BUS:ADDRESS to select an iPod")
                .into()
        }
        error => error.into(),
    })
}

fn parse_selector(value: &str) -> Result<DeviceSelector, String> {
    let message = "--device needs decimal BUS:ADDRESS from `revola devices`, not USB VID:PID; omit it for a single device";
    let (bus, address) = value.split_once(':').ok_or(message)?;
    if [bus, address]
        .iter()
        .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(message.into());
    }
    Ok(DeviceSelector {
        bus: bus.parse().map_err(|_| message)?,
        address: address.parse().map_err(|_| message)?,
    })
}

fn ensure_new_output(path: &str) -> CliResult<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err("Output already exists; choose a new path".into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn write_new_output(path: &str, bytes: &[u8]) -> CliResult<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn read_bounded(path: impl AsRef<Path>, max: usize) -> CliResult<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(format!("Input exceeds {max} bytes").into());
    }
    Ok(bytes)
}

fn print_json(value: &impl Serialize) -> CliResult<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}

fn print_report(report: &CheckReport, json: bool) -> CliResult<u8> {
    write_report(report, json, &mut io::stdout().lock())
}

fn write_report(report: &CheckReport, json: bool, out: &mut impl Write) -> CliResult<u8> {
    if json {
        serde_json::to_writer_pretty(&mut *out, report)?;
        writeln!(out)?;
    } else {
        for check in &report.checks {
            writeln!(out, "{:?}: {:?} — {}", check.id, check.status, check.detail)?;
        }
        if let Some(identity) = &report.identity {
            writeln!(out, "Serial: {}", identity.serial)?;
        }
        writeln!(out, "DFU cleanup: {:?}", report.cleanup)?;
        if let Cleanup::Failed(_) = report.cleanup {
            writeln!(out, "Re-enter fresh DFU before another operation.")?;
        }
        writeln!(out, "Target compatible: {}", report.compatible)?;
    }
    Ok(if report.compatible { 0 } else { 2 })
}

fn write_check_event(event: &Event, json: bool, out: &mut impl Write) -> io::Result<()> {
    if json {
        return Ok(());
    }
    match event {
        Event::Check(check) if check.status == revola_device::CheckStatus::Running => {
            let label = match check.id {
                revola_device::CheckId::Rom => "Reading BootROM",
                revola_device::CheckId::Model => "Reading device information",
                _ => "Checking DFU",
            };
            writeln!(out, "{label}…")
        }
        _ => Ok(()),
    }
}

fn run(command: Command, json: bool) -> CliResult<u8> {
    match command {
        Command::Help => {
            io::stdout().lock().write_all(HELP.as_bytes())?;
        }
        Command::Devices => {
            let devices = discover()?;
            if json {
                print_json(&devices)?;
            } else {
                let mut out = io::stdout().lock();
                if devices.is_empty() {
                    writeln!(out, "No relevant Apple USB devices found")?;
                }
                for device in devices {
                    writeln!(
                        out,
                        "{}:{}  {:04x}:{:04x}  {}",
                        device.selector.bus,
                        device.selector.address,
                        device.vendor_id,
                        device.product_id,
                        device.mode
                    )?;
                }
            }
        }
        Command::DfuState { selector } => {
            let mut session = Session::open(selector)?;
            let state = session.dfu_state()?;
            if json {
                print_json(
                    &serde_json::json!({"backend": session.usb_backend(), "dfu_state": state, "device": session.device()}),
                )?;
            } else {
                writeln!(
                    io::stdout().lock(),
                    "USB backend: {}\nDFU state: {state}",
                    session.usb_backend()
                )?;
            }
        }
        Command::Check { selector } => {
            let mut session = open_session(selector)?;
            let report = session.check(|event| {
                // Ignore logging failures so DFU cleanup still runs.
                let _ = write_check_event(&event, json, &mut io::stderr().lock());
            });
            return print_report(&report, json);
        }
        Command::CheckFiles { bootrom, syscfg } => {
            let rom = read_bounded(bootrom, revola_device::BOOTROM_SIZE)?;
            let cfg = read_bounded(syscfg, 0x100000)?;
            return print_report(&check_saved(&rom, &cfg), json);
        }
        Command::Syscfg { path } => {
            let bytes = read_bounded(path, 0x100000)?;
            let config = SysCfg::parse(&bytes)?;
            let identity = config.identity()?;
            if json {
                print_json(&identity)?;
            } else {
                let mut out = io::stdout().lock();
                writeln!(
                    out,
                    "Model: {}\nSerial: {}\nHardware: {:#010x}\nRecorded firmware: {}",
                    identity.model,
                    identity.serial,
                    identity.hardware_version,
                    identity.recorded_firmware
                )?;
            }
        }
        Command::Firmware(args) => return firmware::run(args, json),
        Command::Bundle(args) => return bundle::run(args, json),
        Command::Upload(args) => return upload::run(args, json),
        Command::Debug(args) => return debug::run(args, json),
        Command::Decrypt(args) => return decrypt::run(args, json),
        Command::NorDump { output, selector } => {
            ensure_new_output(&output)?;
            let mut session = open_session(selector)?;
            let checks = session.check(|event| {
                let _ = write_check_event(&event, json, &mut io::stderr().lock());
            });
            if !checks.compatible {
                return print_report(&checks, json);
            }
            let mut progress = Progress::default();
            let dump = session.dump_nor(|p| {
                let _ = progress.write(
                    p.stage,
                    p.completed as u64,
                    p.total as u64,
                    json,
                    &mut io::stderr().lock(),
                );
                Ok(())
            })?;
            write_new_output(&output, &dump.bytes)?;
            if json {
                print_json(&serde_json::json!({
                    "checks": checks,
                    "nor_dump": dump.report,
                    "output": output,
                }))?;
            } else {
                let mut out = io::stdout().lock();
                writeln!(
                    out,
                    "Read and cross-checked {} NOR bytes in {:.3}s",
                    dump.report.bytes, dump.report.seconds
                )?;
                writeln!(
                    out,
                    "SHA-256: {}\nDFU cleanup: {:?}\nOutput: {}",
                    dump.report.sha256, dump.report.cleanup, output
                )?;
            }
        }
    }
    Ok(0)
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    let json = args.iter().any(|s| s == "--json");
    let result = parse(&args)
        .map_err(Into::into)
        .and_then(|(command, json)| run(command, json));
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if json {
                let _ = print_json(&serde_json::json!({
                    "schema_version": 1,
                    "error": error.to_string(),
                }));
            } else {
                let _ = writeln!(io::stderr().lock(), "Error: {error}");
            }
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn arguments_reject_missing_values_and_ambiguous_device_selection() {
        for line in [
            "check --device 1:2:3",
            "check --device 999:1",
            "check --device 1:2 --device 1:3",
            "nor-dump --output --json",
            "check-files --bootrom a --bootrom b --syscfg c",
        ] {
            assert!(parse(&args(line)).is_err(), "{line}");
        }
    }

    #[test]
    fn exclusive_output_preserves_existing_files() {
        let path = env::temp_dir().join(format!("revola-output-test-{}", std::process::id()));
        let path = path.to_str().unwrap();
        write_new_output(path, b"original").unwrap();
        assert!(ensure_new_output(path).is_err());
        assert!(write_new_output(path, b"replacement").is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"original");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn json_check_output_is_one_report_without_text_progress() {
        let report = check_saved(&[0; 64], &[]);
        let mut stderr = Vec::new();
        let events = [
            Event::Check(report.checks[0].clone()),
            Event::Progress {
                stage: revola_device::CheckId::Rom,
                completed: 1024,
                total: 65536,
            },
        ];
        for event in &events {
            write_check_event(event, true, &mut stderr).unwrap();
        }
        for stage in ["echo", "decrypt", "nor-read", "nor-cross-check"] {
            Progress::default()
                .write(stage, 512, 1024, true, &mut stderr)
                .unwrap();
        }
        assert!(stderr.is_empty());
        let mut stdout = Vec::new();
        assert_eq!(write_report(&report, true, &mut stdout).unwrap(), 2);
        let parsed: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["compatible"], false);
    }
}
