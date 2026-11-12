# installer

Desktop installer for RevolaOS and the Rust crates it is built on: device access over DFU, firmware input preparation, recipe assembly and bundle verification, plus the `revola` command line tool and the on-device USB helper.

## Layout

| Path | Contents |
|---|---|
| `crates/revola-device` | BootROM DFU checks, NOR backup, AES decryption through the device, verified file uploads and removal, bootloader installation |
| `crates/revola-bundle` | Signed release bundles, IPSW and AUPD preparation, recipe assembly |
| `crates/revola-cli` | The `revola` command line tool over both crates |
| `app/` | The Tauri 2 and React installer |
| `usb-helper/` | On-device upload helper, built from the pinned Rockbox checkout |
| `rockbox/` | Patch applied to that checkout |
| `firmware/apple.json` | Sizes and hashes of the pinned Apple 2.0.5 inputs; no Apple bytes |
| `tools/` | `setup.py` vendors Rockbox, `firmware.py` reads the profile, `bundle.py` exports and packs bundles, `debug_symbols.py` generates the debug constants |
| `docs/` | Documentation |

## Build

| Part | Requires |
|---|---|
| Crates and CLI | Rust, libusb, pkg-config |
| USB helper | GNU make, `arm-elf-eabi-gcc` 9.5.0, the Rockbox checkout from `tools/setup.py` |
| Desktop app | Node.js, pnpm, the Tauri platform prerequisites |

```sh
cargo build --release -p revola-cli
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cd app && pnpm install && pnpm tauri dev
```

## Documentation

| Page | Contents |
|---|---|
| [docs/](docs/README.md) | Command line, debugging, bundles, USB helper, desktop app |
| [User guide](https://github.com/RevolaOS/revola/tree/main/docs/guide) | Installing and uninstalling as a user |
| [STYLE.md](STYLE.md) | Principles and rules for every file |

## Credits

RevolaOS started from the groundwork of [RepriseOS](https://github.com/lgreitis/osos-lab) by lgreitis: the boot chain, the installer's device and bundle code, and the reverse engineering of Apple's 2.0.5 firmware. You can support lgreitis on [Ko-fi](https://ko-fi.com/lgreitis). DFU code is adapted from wInd3x; the PIPE0 protocol follows Rockbox's `ipoddfu.c`. See NOTICE.

## License

GPL-3.0-only. See COPYING and NOTICE.
