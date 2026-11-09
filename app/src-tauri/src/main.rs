// SPDX-License-Identifier: GPL-3.0-only
// Hide the console window in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod linux_graphics;

fn main() {
    linux_graphics::configure();
    revola_installer_lib::run()
}
