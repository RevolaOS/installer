# Working rules

Follow STYLE.md for every file you touch.

- Never guess. Addresses, struct offsets, Apple behaviour and RAM figures come from the symbols file, Ghidra, the emulator or hardware. If you cannot find it, say so.
- Verify in the emulator before saying a change works. Use `ipodvm` from the emulator repository for disks, launching, input, screen reading and profiling. Quit only VMs you launched.
- Report measurements, progress and open problems in the conversation or in the workspace `notes/` folder. Never in committed files.
- Never commit Apple firmware bytes, decrypted images, disk images, Ghidra project files or anything derived from them except names, comments, types and addresses.
- Keep the change to what was asked. Put unrelated findings in the conversation.
- Commit and push only when asked. No generated-by trailers in commit messages.
