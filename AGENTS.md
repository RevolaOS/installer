# Working rules

Follow STYLE.md for every file you touch.

- Never guess. Addresses, struct offsets, Apple behaviour and RAM figures come from the symbols file, Ghidra, the emulator or hardware. If you cannot find it, say so.
- New Apple functions, data, sites, offsets or constants start in the reversing repository: reverse them in Ghidra and name them there, add them to `symbols/osos-2.0.5/`, regenerate, and copy `osos-2.0.5.h` and `.ld` unchanged into `revola/firmware/`. Code uses only the generated names (`ADDR_`, `SITE_`, struct offsets, `RES_`). Never write a raw address or offset; instruction words appear only as a site's expected word in `sites.def` or as an `RV_HOOK_WORD` replacement with a comment saying what it does. Never edit the copied header.
- Before naming anything Apple, search Ghidra and the symbols file. Keep a name that already exists instead of adding a second one, and name new things in line with their neighbours (same prefix and wording).
- Verify in the emulator before saying a change works. Use `ipodvm` from the emulator repository for disks, launching, input, screen reading and profiling. Quit only VMs you launched.
- Report measurements, progress and open problems in the conversation or in the workspace `notes/` folder. Never in committed files.
- Never commit Apple firmware bytes, decrypted images, disk images, Ghidra project files or anything derived from them except names, comments, types and addresses.
- Keep the change to what was asked. Put unrelated findings in the conversation.
- Commit and push only when asked. No generated-by trailers in commit messages.
