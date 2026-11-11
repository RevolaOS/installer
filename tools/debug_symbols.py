# SPDX-License-Identifier: GPL-3.0-only
"""Export the debugger's wheel and display symbols from the generated firmware header."""

import argparse
import re
from pathlib import Path

NAMES = {
    "ADDR_WheelState": "ADDR_WHEEL_STATE",
    "WheelState_notify_pending": "WHEEL_STATE_NOTIFY_PENDING",
    "WheelState_position": "WHEEL_STATE_POSITION",
    "WheelState_reported_position": "WHEEL_STATE_REPORTED_POSITION",
    "DisplayPipe_control": "CONTROL",
    "DisplayPipe_order": "ORDER",
    "DisplayPipe_slot0": "SLOT0",
    "DisplayPipe_slot_step": "SLOT_STEP",
    "DisplaySlot_stride": "SLOT_STRIDE",
    "DisplaySlot_format": "SLOT_FORMAT",
    "DisplaySlot_source": "SLOT_SOURCE",
    "DisplaySlot_size": "SLOT_SIZE",
    "DisplaySlot_position": "SLOT_POSITION",
    "DisplayVideoMode_YUV420": "VIDEO_MODE_YUV420",
    "DisplayLayerEnable_Layer0": "LAYER0_ENABLE",
    "DisplayLayerEnable_Video": "VIDEO_ENABLE",
    "DisplayPipe_video_mode": "VIDEO_MODE",
    "DisplayPipe_video_strides": "VIDEO_STRIDES",
    "DisplayPipe_video_source_start": "VIDEO_SOURCE_START",
    "DisplayPipe_video_source_end": "VIDEO_SOURCE_END",
    "DisplayPipe_video_plane0": "VIDEO_PLANE0",
    "DisplayPipe_video_plane1": "VIDEO_PLANE1",
    "DisplayPipe_video_plane3": "VIDEO_PLANE3",
    "DisplayPipe_video_scale": "VIDEO_SCALE",
    "DisplayPipe_video_position": "VIDEO_POSITION",
    "DisplayPipe_video_size": "VIDEO_SIZE",
}


def generate(header):
    defines = dict(re.findall(r"^#define (\w+) (0x[0-9a-fA-F]+)$", header, re.M))
    lines = [
        "// SPDX-License-Identifier: GPL-3.0-only",
        "// Generated from osos-2.0.5.h by tools/debug_symbols.py.",
        "",
    ]
    for source, name in NAMES.items():
        kind = "u32" if source.startswith("ADDR_") else "usize"
        lines.append(f"pub(super) const {name}: {kind} = {defines[source]};")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("header", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    output = Path(__file__).resolve().parents[1] / "crates/revola-device/src/debug/symbols.rs"
    generated = generate(args.header.read_text())
    if args.check:
        if output.read_text() != generated:
            raise SystemExit("Debugger symbols differ from the firmware header")
    else:
        output.write_text(generated)
