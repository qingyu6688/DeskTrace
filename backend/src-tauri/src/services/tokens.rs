//! 一次性凭据：目录选择凭据把原生对话框的结果交给后端，前端无法传入任意路径；
//! 清除凭据绑定数据代次，用户看到影响说明后才能确认清除。两者都 60 秒过期、只能使用一次。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use desktrace_core::NormalizedPath;
use uuid::Uuid;

/// 凭据有效期。
pub(crate) const TOKEN_TTL: Duration = Duration::from_secs(60);

/// 目录选择结果。
#[derive(Clone, Debug)]
pub(crate) struct FolderSelection {
    pub(crate) path: NormalizedPath,
    pub(crate) file_identity: Option<String>,
}

#[derive(Default)]
struct Tables {
    selections: HashMap<String, (FolderSelection, Instant)>,
    clears: HashMap<String, (i64, Instant)>,
}

/// 凭据存储，只存在于当前进程内存中。
#[derive(Default)]
pub(crate) struct TokenStore {
    tables: Mutex<Tables>,
}

impl TokenStore {
    pub(crate) fn issue_selection(&self, selection: FolderSelection) -> String {
        let token = Uuid::new_v4().to_string();
        if let Ok(mut tables) = self.tables.lock() {
            prune(&mut tables);
            tables
                .selections
                .insert(token.clone(), (selection, Instant::now() + TOKEN_TTL));
        }
        token
    }

    /// 取出并作废目录选择凭据；过期或不存在时返回 `None`。
    pub(crate) fn take_selection(&self, token: &str) -> Option<FolderSelection> {
        let mut tables = self.tables.lock().ok()?;
        let (selection, expires) = tables.selections.remove(token)?;
        (Instant::now() <= expires).then_some(selection)
    }

    pub(crate) fn issue_clear(&self, revision: i64) -> String {
        let token = Uuid::new_v4().to_string();
        if let Ok(mut tables) = self.tables.lock() {
            prune(&mut tables);
            tables
                .clears
                .insert(token.clone(), (revision, Instant::now() + TOKEN_TTL));
        }
        token
    }

    /// 取出并作废清除凭据，返回签发时的数据代次。
    pub(crate) fn take_clear(&self, token: &str) -> Option<i64> {
        let mut tables = self.tables.lock().ok()?;
        let (revision, expires) = tables.clears.remove(token)?;
        (Instant::now() <= expires).then_some(revision)
    }
}

fn prune(tables: &mut Tables) {
    let now = Instant::now();
    tables.selections.retain(|_, (_, expires)| *expires >= now);
    tables.clears.retain(|_, (_, expires)| *expires >= now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_single_use() -> Result<(), desktrace_core::PathError> {
        let store = TokenStore::default();
        let token = store.issue_selection(FolderSelection {
            path: desktrace_core::normalize_local_path(r"D:\Projects")?,
            file_identity: None,
        });
        assert!(store.take_selection(&token).is_some());
        assert!(store.take_selection(&token).is_none());
        let clear = store.issue_clear(7);
        assert_eq!(store.take_clear(&clear), Some(7));
        assert_eq!(store.take_clear(&clear), None);
        assert_eq!(store.take_clear("forged"), None);
        Ok(())
    }
}
