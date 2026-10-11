#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    desktrace_desktop_lib::run().context("DeskTrace 启动失败")
}
