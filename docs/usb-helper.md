# USB upload helper

The helper is a Rockbox-based image sent over BootROM DFU. It enumerates as WinUSB device `f055:180d`, writes one file to the data partition and reads it back, or deletes `/Revola/Firmware/revola.bin`, `/Revola/Firmware/loader.bin`, `/Revola/` and unfinished uploads, then returns to DFU.

## Build

| Requirement | |
|---|---|
| GNU make | |
| `arm-elf-eabi-gcc` 9.5.0 | the version pinned in `firmware/apple.json` |
| Rockbox checkout | the commit pinned in `dependencies.lock`, with `rockbox/*.patch` applied |

```sh
python3 tools/setup.py            # clones Rockbox into vendor/ and applies rockbox/*.patch
python3 tools/setup.py --check    # verifies the pin
python3 usb-helper/build.py --toolchain /path/to/bin --out build/usb-helper
python3 -m unittest discover -s usb-helper/tests -p '*_test.py'
```

## Output

| File | |
|---|---|
| `build/usb-helper/upload.dfu` | the IMG1 image |
| `build/usb-helper/manifest.json` | schema 3: BootROM hash, size, SHA-256, offsets of the nonce, config and cold-init slots, and the `storage_inspection` and `file_removal` capabilities |

Both go into a bundle as the `usb_helper` component. `usb-helper/tests/fixtures/` holds one built image for the Rust and Python tests; it contains Rockbox and helper code only.
