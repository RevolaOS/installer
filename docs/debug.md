# Debugging a device

`revola debug --help` lists the commands for a normal-mode iPod running the USB Debug extension. Enable it in Settings > Extensions. While enabled, USB stays in the running UI instead of disk mode; disable it and reconnect the cable to sync. Build the extension with the SDK matching the installed core. The wire protocol is in the extensions repository, [usb-debug/PROTOCOL.md](https://github.com/RevolaOS/extensions/blob/main/usb-debug/PROTOCOL.md).

## Limits

| Command | |
|---|---|
| `scroll` | injects wheel movements, not exact row counts; it synchronises against Apple's wheel state and refuses firmware other than the pinned image |
| screen parsing, `goto` | describe and navigate the current UI; a screen check and a later input are not atomic. Scrolling Now Playing changes volume |
| `time-site` | stops timing after success or failure; if the stop is not acknowledged the command fails and warns that timing may still be active |

## Firmware symbols

The wheel addresses and offsets in `crates/revola-device/src/debug/symbols.rs` are generated from the canonical firmware header. From the workspace root:

```sh
python3 installer/tools/debug_symbols.py --check revola/firmware/osos-2.0.5.h
```

Omit `--check` to regenerate after the firmware symbols have been verified and copied from reversing. The Rust constants are never edited by hand.
