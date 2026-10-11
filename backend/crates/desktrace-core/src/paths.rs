//! Windows 本地目录路径的规范化与按组件的子树匹配。
//!
//! 这里只做字符串层面的规范化：盘符大写、统一分隔符、去掉 `.` 和多余分隔符。
//! 符号链接、短文件名和区分大小写目录需要原生层另行解析，不在此处猜测。

use thiserror::Error;

/// 路径校验错误，文案直接展示给用户。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathError {
    /// 空路径。
    #[error("路径不能为空")]
    Empty,
    /// 不是带盘符的绝对路径。
    #[error("只支持带盘符的本地绝对路径，例如 D:\\Projects")]
    NotLocalAbsolute,
    /// 网络路径首期不作为长期判断对象。
    #[error("暂不支持网络路径")]
    Network,
    /// 含 `..`，需要原生层解析后再传入。
    #[error("路径不能包含上级目录跳转")]
    ParentTraversal,
    /// 含 Windows 文件名不允许的字符。
    #[error("路径包含不允许的字符")]
    InvalidCharacter,
}

/// 规范化后的目录路径。`display` 保留原有大小写，`key` 用于比较。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedPath {
    /// 展示用路径，如 `D:\Projects\旧网站`。
    pub display: String,
    /// 比较键：大写、反斜杠分隔、无结尾分隔符（盘符根保留 `D:\`）。
    pub key: String,
}

const LONG_PREFIX: &str = r"\\?\";
const INVALID_CHARS: [char; 6] = ['<', '>', '"', '|', '?', '*'];

/// 规范化本地绝对目录路径。
pub fn normalize_local_path(input: &str) -> Result<NormalizedPath, PathError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(PathError::Empty);
    }
    let replaced = trimmed.replace('/', "\\");
    let without_prefix = match replaced.strip_prefix(LONG_PREFIX) {
        Some(rest) if rest.to_ascii_uppercase().starts_with(r"UNC\") => {
            return Err(PathError::Network);
        }
        Some(rest) => rest.to_string(),
        None if replaced.starts_with(r"\\") => return Err(PathError::Network),
        None => replaced,
    };
    let mut chars = without_prefix.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic);
    let colon = chars.next();
    let separator = chars.next();
    let (Some(drive), Some(':')) = (drive, colon) else {
        return Err(PathError::NotLocalAbsolute);
    };
    if separator.is_some_and(|value| value != '\\') {
        return Err(PathError::NotLocalAbsolute);
    }
    if separator.is_none() {
        // `D:` 表示该盘当前目录，不是根目录。
        return Err(PathError::NotLocalAbsolute);
    }
    let rest = without_prefix.get(3..).unwrap_or_default();
    let mut components = Vec::new();
    for component in rest.split('\\') {
        match component {
            "" | "." => {}
            ".." => return Err(PathError::ParentTraversal),
            value => {
                if value
                    .chars()
                    .any(|ch| INVALID_CHARS.contains(&ch) || ch == ':' || ch.is_control())
                {
                    return Err(PathError::InvalidCharacter);
                }
                // Windows 会去掉文件名结尾的空格和点，保留会造成同一目录两个键。
                let cleaned = value.trim_end_matches([' ', '.']);
                if cleaned.is_empty() {
                    return Err(PathError::InvalidCharacter);
                }
                components.push(cleaned.to_string());
            }
        }
    }
    let root = format!("{}:\\", drive.to_ascii_uppercase());
    let display = format!("{root}{}", components.join("\\"));
    Ok(NormalizedPath {
        key: path_key(&display),
        display,
    })
}

/// 生成比较键。Windows 默认目录不区分大小写，按 Unicode 大写比较更接近系统规则。
fn path_key(display: &str) -> String {
    display.chars().flat_map(char::to_uppercase).collect()
}

/// 按路径组件判断 `child` 是否等于 `parent` 或位于其子树内。
/// `C:\WORK` 不会匹配 `C:\WORKSHOP`。
pub fn is_same_or_descendant(child_key: &str, parent_key: &str) -> bool {
    if child_key == parent_key {
        return true;
    }
    let Some(rest) = child_key.strip_prefix(parent_key) else {
        return false;
    };
    // 盘符根的键以反斜杠结尾，其余键不以分隔符结尾。
    parent_key.ends_with('\\') || rest.starts_with('\\')
}

/// 判断 `child` 是否是 `parent` 的直接或间接子目录（不含相等）。
pub fn is_descendant(child_key: &str, parent_key: &str) -> bool {
    child_key != parent_key && is_same_or_descendant(child_key, parent_key)
}

/// 返回路径最后一个组件作为展示名称；盘符根返回 `D:\`。
pub fn display_name(display: &str) -> String {
    match display.trim_end_matches('\\').rsplit('\\').next() {
        Some(name) if !name.ends_with(':') && !name.is_empty() => name.to_string(),
        _ => display.to_string(),
    }
}

/// 参与归属判断的一个观察范围。
#[derive(Clone, Copy, Debug)]
pub struct ScopeRule<'a> {
    /// 范围根目录。
    pub root: &'a NormalizedPath,
    /// 是否让任意深度的子项各自成为对象。
    pub include_descendants: bool,
}

/// 路径在观察范围中的归属。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeMatch {
    /// 记录到哪个对象的路径。
    pub target: NormalizedPath,
    /// 目标就是输入路径本身；为假时表示记到了它所在的范围直接子项上。
    pub direct: bool,
}

/// 解析路径归属。等于范围根时归到根；范围包含子目录时归到路径本身；
/// 不包含子目录时归到范围根下的直接子项，例如桌面上某个文件夹里的内容记到这个文件夹。
/// 多个范围同时命中时取最具体（根路径最长）的一个。
pub fn resolve_in_scopes(path: &NormalizedPath, scopes: &[ScopeRule<'_>]) -> Option<ScopeMatch> {
    let scope = scopes
        .iter()
        .filter(|scope| is_same_or_descendant(&path.key, &scope.root.key))
        .max_by_key(|scope| scope.root.key.len())?;
    if path.key == scope.root.key || scope.include_descendants {
        return Some(ScopeMatch {
            target: path.clone(),
            direct: true,
        });
    }
    let target = top_level_child(path, scope.root)?;
    let direct = target.key == path.key;
    Some(ScopeMatch { target, direct })
}

fn components(display: &str) -> Vec<&str> {
    display.trim_end_matches('\\').split('\\').collect()
}

/// `path` 位于 `root` 之下时，返回它所在的 `root` 直接子项。
pub fn top_level_child(path: &NormalizedPath, root: &NormalizedPath) -> Option<NormalizedPath> {
    if !is_descendant(&path.key, &root.key) {
        return None;
    }
    let root_parts = components(&root.display).len();
    let parts = components(&path.display);
    let child = parts.get(..=root_parts)?.join("\\");
    normalize_local_path(&child).ok()
}

/// 打开后不会登记到“最近使用的项目”的文件类型：程序、脚本、安装包和快捷方式。
/// 这些文件仍会列出，但状态为“无法判断”，不会被误判为长期未用。
const UNOBSERVABLE_EXTENSIONS: [&str; 15] = [
    "exe",
    "com",
    "scr",
    "bat",
    "cmd",
    "ps1",
    "vbs",
    "js",
    "msi",
    "msix",
    "appx",
    "appinstaller",
    "lnk",
    "url",
    "appref-ms",
];

/// 文件被打开时能否通过“最近使用的项目”观察到。
pub fn file_open_observable(file_name: &str) -> bool {
    match file_name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => {
            let extension = extension.to_ascii_lowercase();
            !UNOBSERVABLE_EXTENSIONS.contains(&extension.as_str())
        }
        _ => true,
    }
}
