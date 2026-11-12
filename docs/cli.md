# Command line

`revola` is the command line tool over the `revola-device` and `revola-bundle` crates.

```sh
cargo build --release -p revola-cli
target/release/revola --help
```

| Command | Does |
|---|---|
| `devices`, `dfu-state` | lists iPods in BootROM DFU, or the state of one |
| `check` | runs the device checks the installer runs |
| `nor-dump --output FILE` | backs up the NOR flash |
| `decrypt` | decrypts a file through the device's AES engine |
| `check-files`, `syscfg` | check a BootROM dump and SysCfg, or print a SysCfg |
| `firmware inspect --ipsw FILE` | reads the firmware profile of an IPSW |
| `firmware extract`, `unpack`, `prepare` | pull the IMG1 out of an IPSW, unpack it, and prepare the decrypted inputs the recipe needs |
| `firmware decrypt-nor --input BACKUP --output FILE` | decrypts a NOR backup through the device |
| `firmware extract-efi` | extracts the EFI volume of a plaintext loader for the reversing tooling |
| `bundle apply-recipe --recipe FILE --data FILE --inputs DIR --out FILE` | applies a recipe to decrypted Apple inputs |
| `bundle export --spec FILE --base DIR --out DIR` | builds a bundle from a specification |
| `bundle sign`, `pack`, `inspect`, `fetch`, `assemble` | operate on a bundle |
| `upload --input FILE --destination /PATH --helper DIR` | sends one file to the data partition through the USB helper and verifies it; `--bundle DIR --key FILE` takes the helper from a bundle instead |
| `debug COMMAND` | see [Debugging a device](debug.md) |

`revola --help` lists every option. Installation and BootROM inspection commands use BootROM DFU. Select an iPod with `--device BUS:ADDRESS` and add `--json` for machine-readable output.

`apply-recipe` applies a schema 2 recipe (`classic7g-file-v1`) to decrypted Apple inputs on the user's machine; the recipe builder in the revola repository produces the recipe and data files.

## Tests

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Offline replay tests are ignored unless these variables point at preserved 2.0.5 material outside the repository:

`REVOLA_ASSEMBLY_ROOT`, `REVOLA_ASSEMBLY_BUILD`, `REVOLA_TEST_IPSW`, `REVOLA_TEST_PACKAGE`, `REVOLA_TEST_BOOTROM`, optionally `REVOLA_TEST_NOR` and `REVOLA_TEST_PREVIOUS_BOOTLOADER`.
