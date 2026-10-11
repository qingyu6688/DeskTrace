//! 应用清单读取：HKCU/HKLM 卸载项（显式区分 32/64 位视图）与当前用户的 MSIX 包入口。
//! 只读取名称、发布者、版本、日期和主程序线索，不执行 `UninstallString`，不扫描其他用户。

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::ApplicationModel::Package;
use windows::Management::Deployment::PackageManager;
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_SAM_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::WindowsError;

const UNINSTALL_PATH: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall");
/// 1601 到 1970 的毫秒差，WinRT DateTime 以 1601 年为纪元、单位 100 纳秒。
const WINRT_EPOCH_OFFSET_MS: i64 = 11_644_473_600_000;
const WINRT_TICKS_PER_MS: i64 = 10_000;
/// 注册表键名最长 255 个字符。
const MAX_KEY_NAME: usize = 256;
/// 补丁、更新类卸载项不是独立应用。
const SKIPPED_RELEASE_TYPES: [&str; 4] = ["update", "hotfix", "security update", "service pack"];

/// 一个 Win32 卸载项。
#[derive(Clone, Debug)]
pub struct Win32App {
    /// `HKLM\64\{子键名}` 形式的清单身份。
    pub uninstall_key: String,
    /// 展示名称。
    pub display_name: String,
    /// 发布者。
    pub publisher: Option<String>,
    /// 版本。
    pub version: Option<String>,
    /// `InstallDate` 原始值。
    pub install_date_raw: Option<String>,
    /// `WindowsInstaller=1` 表示 MSI 安装。
    pub is_msi: bool,
    /// 从 `DisplayIcon` 确认存在的主程序路径，不是卸载程序。
    pub entry_executable: Option<String>,
}

/// 一个当前用户可启动的打包应用入口。
#[derive(Clone, Debug)]
pub struct PackageApp {
    /// 应用用户模型 ID。
    pub aumid: String,
    /// 入口展示名称。
    pub display_name: String,
    /// 发布者。
    pub publisher: Option<String>,
    /// 包版本。
    pub version: Option<String>,
    /// 包安装或更新时间（UTC 毫秒）。
    pub installed_at_ms: Option<i64>,
}

/// 一次清单读取结果。`complete` 为假表示有来源读取失败，不能据此判断应用已卸载。
#[derive(Clone, Debug, Default)]
pub struct CatalogScan {
    /// Win32 卸载项。
    pub win32: Vec<Win32App>,
    /// 打包应用入口。
    pub packages: Vec<PackageApp>,
    /// 所有来源都读取成功。
    pub complete: bool,
    /// 被取消。
    pub cancelled: bool,
}

/// 读取完整清单。在调用线程上初始化 MTA 以使用 WinRT 包管理接口。
pub fn read_catalog(cancel: &AtomicBool) -> CatalogScan {
    let mut scan = CatalogScan {
        complete: true,
        ..CatalogScan::default()
    };
    let sources: [(HKEY, &str, REG_SAM_FLAGS, &str); 3] = [
        (HKEY_LOCAL_MACHINE, "HKLM", KEY_WOW64_64KEY, "64"),
        (HKEY_LOCAL_MACHINE, "HKLM", KEY_WOW64_32KEY, "32"),
        (HKEY_CURRENT_USER, "HKCU", KEY_WOW64_64KEY, "64"),
    ];
    let mut seen = HashSet::new();
    for (root, root_name, view, view_name) in sources {
        if cancel.load(Ordering::Relaxed) {
            scan.cancelled = true;
            scan.complete = false;
            return scan;
        }
        match read_uninstall_view(root, view) {
            Ok(entries) => {
                for (subkey, app) in entries {
                    // 同一产品同时出现在两个视图时只保留先读到的 64 位视图。
                    if seen.insert((
                        root_name,
                        subkey.to_lowercase(),
                        app.display_name.to_lowercase(),
                    )) {
                        scan.win32.push(Win32App {
                            uninstall_key: format!("{root_name}\\{view_name}\\{subkey}"),
                            ..app
                        });
                    }
                }
            }
            Err(_) => scan.complete = false,
        }
    }
    if cancel.load(Ordering::Relaxed) {
        scan.cancelled = true;
        scan.complete = false;
        return scan;
    }
    match read_packages(cancel) {
        Ok(packages) => scan.packages = packages,
        Err(_) => scan.complete = false,
    }
    scan
}

/// 打开的注册表键，离开作用域时关闭。
struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY：句柄由 RegOpenKeyExW 成功返回，只在这里关闭一次。
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn check(status: WIN32_ERROR) -> Result<(), WindowsError> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(WindowsError::Api {
            code: status.0 as i32,
        })
    }
}

fn read_uninstall_view(
    root: HKEY,
    view: REG_SAM_FLAGS,
) -> Result<Vec<(String, Win32App)>, WindowsError> {
    let mut raw = HKEY::default();
    // SAFETY：只读打开固定路径；成功后句柄交给 RegistryKey 管理。
    let status = unsafe { RegOpenKeyExW(root, UNINSTALL_PATH, None, KEY_READ | view, &mut raw) };
    if status == ERROR_FILE_NOT_FOUND {
        // 当前用户没有任何按用户安装的程序时该键可能不存在，属于正常的空结果。
        return Ok(Vec::new());
    }
    check(status)?;
    let key = RegistryKey(raw);
    let mut entries = Vec::new();
    for index in 0.. {
        let mut name = [0u16; MAX_KEY_NAME];
        let mut length = MAX_KEY_NAME as u32;
        // SAFETY：缓冲区长度按字符数传入，系统写入不会越界。
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut length,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        check(status)?;
        let subkey = String::from_utf16_lossy(&name[..length as usize]);
        if let Some(app) = read_entry(&key, &subkey) {
            entries.push((subkey, app));
        }
    }
    Ok(entries)
}

fn read_entry(key: &RegistryKey, subkey: &str) -> Option<Win32App> {
    let sub = HSTRING::from(subkey);
    let display_name = read_string(key, &sub, w!("DisplayName"))?;
    if read_dword(key, &sub, w!("SystemComponent")) == Some(1)
        || read_string(key, &sub, w!("ParentKeyName")).is_some()
    {
        return None;
    }
    if read_string(key, &sub, w!("ReleaseType"))
        .is_some_and(|value| SKIPPED_RELEASE_TYPES.contains(&value.to_lowercase().as_str()))
    {
        return None;
    }
    Some(Win32App {
        uninstall_key: String::new(),
        display_name,
        publisher: read_string(key, &sub, w!("Publisher")),
        version: read_string(key, &sub, w!("DisplayVersion")),
        install_date_raw: read_string(key, &sub, w!("InstallDate")),
        is_msi: read_dword(key, &sub, w!("WindowsInstaller")) == Some(1),
        entry_executable: read_string(key, &sub, w!("DisplayIcon"))
            .and_then(|icon| entry_from_icon(&icon)),
    })
}

/// 读取字符串值，`REG_EXPAND_SZ` 会被系统展开。空字符串按缺失处理。
fn read_string(key: &RegistryKey, subkey: &HSTRING, name: PCWSTR) -> Option<String> {
    let mut size = 0u32;
    // SAFETY：第一次只查询字节数。
    let status = unsafe {
        RegGetValueW(
            key.0,
            subkey,
            name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY：缓冲区按查询到的字节数分配。
    let status = unsafe {
        RegGetValueW(
            key.0,
            subkey,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..end]).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn read_dword(key: &RegistryKey, subkey: &HSTRING, name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY：输出缓冲区是本地 u32，大小匹配 REG_DWORD。
    let status = unsafe {
        RegGetValueW(
            key.0,
            subkey,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(value)
}

/// 从 `DisplayIcon` 提取主程序：去掉引号和图标序号，只接受真实存在的 `.exe`，
/// 并排除卸载、安装、更新程序，避免把卸载器当成应用入口。
fn entry_from_icon(icon: &str) -> Option<String> {
    let trimmed = icon.trim();
    let path = if let Some(rest) = trimmed.strip_prefix('"') {
        rest.split('"').next()?.to_string()
    } else {
        match trimmed.rsplit_once(',') {
            Some((path, index)) if index.trim().parse::<i32>().is_ok() => path.trim().to_string(),
            _ => trimmed.to_string(),
        }
    };
    let file = Path::new(&path);
    let extension = file.extension()?.to_string_lossy().to_lowercase();
    let stem = file.file_stem()?.to_string_lossy().to_lowercase();
    let helper = stem.starts_with("unins")
        || stem.contains("uninstall")
        || stem.starts_with("setup")
        || stem.contains("installer")
        || matches!(stem.as_str(), "msiexec" | "update" | "updater");
    (extension == "exe" && !helper && file.is_file()).then_some(path)
}

/// 当前线程的 MTA 初始化凭据。
struct MultiThreadedApartment;

impl MultiThreadedApartment {
    fn enter() -> Result<Self, WindowsError> {
        // SAFETY：只初始化当前线程，Drop 中在同一线程配对反初始化。
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for MultiThreadedApartment {
    fn drop(&mut self) {
        // SAFETY：与 enter 中的初始化在同一线程配对。
        unsafe { CoUninitialize() };
    }
}

fn read_packages(cancel: &AtomicBool) -> Result<Vec<PackageApp>, WindowsError> {
    let _apartment = MultiThreadedApartment::enter()?;
    let manager = PackageManager::new()?;
    // 空的安全标识符表示当前用户，普通权限即可读取。
    let packages = manager.FindPackagesByUserSecurityId(&HSTRING::new())?;
    let iterator = packages.First()?;
    let mut apps = Vec::new();
    while iterator.HasCurrent()? {
        if cancel.load(Ordering::Relaxed) {
            return Err(WindowsError::ThreadFailed);
        }
        let package = iterator.Current()?;
        // 单个包读不出入口不影响其余包；框架包、资源包和占位包没有用户入口。
        if let Ok(mut entries) = package_entries(&package) {
            apps.append(&mut entries);
        }
        iterator.MoveNext()?;
    }
    Ok(apps)
}

fn package_entries(package: &Package) -> Result<Vec<PackageApp>, WindowsError> {
    if package.IsFramework()?
        || package.IsResourcePackage()?
        || package.IsBundle()?
        || package.IsStub().unwrap_or(false)
    {
        return Ok(Vec::new());
    }
    let version = package.Id()?.Version()?;
    let version = format!(
        "{}.{}.{}.{}",
        version.Major, version.Minor, version.Build, version.Revision
    );
    let publisher = package
        .PublisherDisplayName()
        .ok()
        .map(|value| value.to_string())
        .filter(|value| !value.is_empty());
    let installed_at_ms = package
        .InstalledDate()
        .ok()
        .map(|date| date.UniversalTime / WINRT_TICKS_PER_MS - WINRT_EPOCH_OFFSET_MS)
        .filter(|value| *value > 0);
    let entries = package.GetAppListEntries()?;
    let mut apps = Vec::new();
    for index in 0..entries.Size()? {
        let entry = entries.GetAt(index)?;
        let aumid = entry.AppUserModelId()?.to_string();
        if aumid.is_empty() {
            continue;
        }
        let display_name = entry
            .DisplayInfo()
            .and_then(|info| info.DisplayName())
            .map(|name| name.to_string())
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| aumid.clone());
        apps.push(PackageApp {
            aumid,
            display_name,
            publisher: publisher.clone(),
            version: Some(version.clone()),
            installed_at_ms,
        });
    }
    Ok(apps)
}
