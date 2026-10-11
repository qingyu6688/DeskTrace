//! 依赖本机系统状态的冒烟检查，默认忽略，需要时在 Windows 上手动运行：
//! `cargo test -p desktrace-windows -- --ignored`
//! 断言只检查数量和格式，不输出应用名称或路径。
#![cfg(windows)]

use std::sync::atomic::AtomicBool;

use desktrace_windows::{
    folder_file_identity, idle_milliseconds, local_offset_minutes, read_catalog,
};

#[test]
#[ignore = "依赖本机时区"]
fn offset_is_within_valid_range() {
    let offset = local_offset_minutes(1_791_684_000_000);
    assert!((-720..=840).contains(&offset));
    assert_eq!(offset % 15, 0);
}

#[test]
#[ignore = "依赖本机文件系统"]
fn folder_identity_is_stable_for_same_directory() {
    let directory = std::env::temp_dir();
    let path = directory.to_string_lossy();
    let first = folder_file_identity(&path);
    assert!(first.as_deref().is_some_and(|value| value.contains(':')));
    assert_eq!(first, folder_file_identity(&path));
    assert!(folder_file_identity(r"Z:\desktrace-does-not-exist").is_none());
}

#[test]
#[ignore = "读取本机应用清单"]
fn catalog_reads_without_failure() {
    let scan = read_catalog(&AtomicBool::new(false));
    assert!(scan.complete, "注册表或包管理接口读取失败");
    assert!(!scan.win32.is_empty() || !scan.packages.is_empty());
    assert!(
        scan.win32
            .iter()
            .all(|app| app.uninstall_key.starts_with("HK"))
    );
    assert!(scan.packages.iter().all(|app| app.aumid.contains('!')));
}

#[test]
#[ignore = "依赖交互会话"]
fn idle_time_is_readable() {
    assert!(idle_milliseconds().is_some());
}

#[test]
#[ignore = "依赖当前用户的桌面"]
fn desktop_folder_exists_and_lists() {
    let desktop = desktrace_windows::desktop_folder().expect("应能读到系统桌面");
    assert!(std::path::Path::new(&desktop).is_dir());
    let items = desktrace_windows::list_directory(&desktop).expect("应能列出桌面");
    assert!(
        items
            .iter()
            .all(|item| !item.path.to_lowercase().ends_with("desktop.ini"))
    );
}

#[test]
#[ignore = "依赖当前用户的最近使用的项目目录"]
fn recent_watcher_starts_and_stops() {
    let (sender, _receiver) = std::sync::mpsc::sync_channel(8);
    let watcher = desktrace_windows::RecentWatcher::start(sender).expect("应能开始监听");
    drop(watcher);
}
