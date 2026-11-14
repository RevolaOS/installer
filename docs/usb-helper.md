# USB upload helper

The helper is a Rockbox-based image sent over BootROM DFU. It enumerates as WinUSB device `f055:180d` and does one of three jobs before returning to DFU: it writes files to the data partition; it inspects the data partition; or it deletes the RevolaOS files.

## Upload

The image config names the first file. Each file is written under a staging name, read back, renamed into place and read back once more under its final name, the one the bootloader opens. The volume stays mounted afterwards, and the host either queues another file or ends the session. All requests are class requests on interface 0; the OUT requests carry the session nonce first and are refused without it.

| Request | | Payload | Accepted in state |
|---|---|---|---|
| `0x52` STATUS | IN | 320-byte result: state, rc, counters, both hashes, path | any |
| `0x53` START | OUT | nonce | 1 READY |
| `0x59` NEXT | OUT | nonce, then the file config: size, mode (0 create, 1 replace), SHA-256, path | 4 COMPLETE with rc 0 |
| `0x54` ACKNOWLEDGE | OUT | nonce | 4 COMPLETE; unmounts and returns to DFU |
| `0x55` CANCEL | OUT | nonce | any until ACKNOWLEDGE; fails the session and returns to DFU |

States: 1 ready, 2 receiving, 3 verifying, 4 complete. NEXT resets the counters and hashes, checks the path, free space and (for mode 0) that the destination is absent, then reports READY; a failed check leaves state 4 with the error code and no further file is accepted. The host reads STATUS at COMPLETE for every file; the result of the last file is also read over DFU after the return. The helper waits 120 s between files and 30 s for progress within one.

## Removal

Removal is best effort: the helper deletes everything under `/Revola/` except the two firmware files, then `/Revola/Firmware/loader.bin` and `/Revola/Firmware/revola.bin`, then the `/.revola-*.part` remains of unfinished uploads. An entry that refuses to go is skipped and the rest continues, so the host always gets to restore Apple's bootloader. The firmware files go last because the RevolaOS bootloader still boots while they exist.

The result word `existing_files` reports the outcome; an entry that did not exist sets neither bit.

| Bit | Deleted | Bit | Remains |
|---|---|---|---|
| 0 | `loader.bin` | 4 | `loader.bin` |
| 1 | `revola.bin` | 5 | `revola.bin` |
| 2 | `/Revola/` | 6 | something under `/Revola/` |
| 3 | unfinished uploads | 7 | an unfinished upload |

Inspection sets bits 0 and 1 for the firmware files that exist. The `file_removal` capability in the manifest says the helper has the removal mode; a release bundle requires it.

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
| `build/usb-helper/manifest.json` | schema 3: BootROM hash, size, SHA-256, offsets of the nonce, config and cold-init slots, and the `storage_inspection`, `file_removal` and `batch_upload` capabilities |

Both go into a bundle as the `usb_helper` component. `usb-helper/tests/fixtures/` holds one built image for the Rust and Python tests; it contains Rockbox and helper code only.
