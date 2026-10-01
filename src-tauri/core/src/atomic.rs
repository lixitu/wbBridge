//! `core/src/atomic.js` 的 Rust 等价实现：替换被占用路径时的短暂退避重试。
//!
//! 原实现注释说明得很清楚——POSIX 的 rename 是原子的且无条件覆盖，而 Windows 在目标被打开
//! （未共享删除权限：杀软实时扫描、同步客户端、编辑器，或正在被读取的配置文件）或映射为
//! 运行中映像时会以 EPERM/EACCES/EBUSY 拒绝替换。这些冲突是短暂的，所以短暂重试而不是直接
//! 让写入失败。
//!
//! 为了可测，`rename` 与 `sleep` 都可注入；同时保留 [`node_code_for_io`] 把 `std::io::Error`
//! 的 errno 映射回 Node 的 errno 字符串，保证「暂态」判定与原实现同源。

use std::fmt;
use std::io;
use std::path::Path;

/// 会被重试的 errno 集合（与 `core/src/atomic.js` 的 `TRANSIENT` 一致）。
pub const TRANSIENT_CODES: [&str; 3] = ["EPERM", "EACCES", "EBUSY"];

/// 退避间隔（毫秒）：一次原始尝试 + 五次重试。
pub const DELAYS: [u64; 5] = [50, 100, 200, 400, 800];

/// 替换失败的错误，保留 Node 风格的 `code` 以便复用同一套暂态判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceError {
    /// Node 风格 errno 字符串（`EPERM`/`ENOENT`/...），未知时为 `None`。
    pub code: Option<String>,
    /// 面向日志的消息。
    pub message: String,
}

impl ReplaceError {
    /// 构造一个带 errno 的错误。
    pub fn new(code: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            code: code.map(|value| value.to_string()),
            message: message.into(),
        }
    }

    /// 是否为「短暂冲突」，即原实现里应当重试的一类失败。
    pub fn is_transient(&self) -> bool {
        is_transient(self.code.as_deref())
    }
}

impl fmt::Display for ReplaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.code {
            Some(code) => write!(formatter, "{code}: {}", self.message),
            None => write!(formatter, "{}", self.message),
        }
    }
}

impl std::error::Error for ReplaceError {}

impl From<io::Error> for ReplaceError {
    fn from(error: io::Error) -> Self {
        match node_code_for_io(&error) {
            Some(code) => ReplaceError::new(Some(code), error.to_string()),
            None => ReplaceError::new(None, error.to_string()),
        }
    }
}

/// Node 风格的暂态 errno 判定。
pub fn is_transient(code: Option<&str>) -> bool {
    matches!(code, Some(value) if TRANSIENT_CODES.contains(&value))
}

/// 把 `std::io::Error` 的原始 errno 映射回 Node 的 `error.code`。
///
/// 覆盖 Unix 的 EPERM(1)/EACCES(13)/EBUSY(16) 与 Windows 的
/// ERROR_ACCESS_DENIED(5)/ERROR_SHARING_VIOLATION(32)/ERROR_LOCK_VIOLATION(33)。
pub fn node_code_for_io(error: &io::Error) -> Option<&'static str> {
    match error.raw_os_error() {
        Some(1) => Some("EPERM"),
        Some(13) => Some("EACCES"),
        Some(16) => Some("EBUSY"),
        Some(2) => Some("ENOENT"),
        Some(5) => Some("EPERM"),
        Some(32) | Some(33) => Some("EBUSY"),
        _ => match error.kind() {
            io::ErrorKind::NotFound => Some("ENOENT"),
            io::ErrorKind::PermissionDenied => Some("EACCES"),
            _ => None,
        },
    }
}

/// `replaceWithRetry(temp, target, { rename, sleep, delays })`。
///
/// 语义与原实现逐条对齐：
/// - 一次原始尝试后，最多再重试 `delays.len()` 次；
/// - 只有暂态 errno 才重试，其余立即抛出；
/// - 暂态但已用尽重试次数时，抛出**最后一次**的错误。
pub fn replace_with_retry_with<R, S>(
    temp: &Path,
    target: &Path,
    mut rename: R,
    mut sleep: S,
    delays: &[u64],
) -> Result<(), ReplaceError>
where
    R: FnMut(&Path, &Path) -> Result<(), ReplaceError>,
    S: FnMut(u64),
{
    let mut attempt = 0usize;
    loop {
        match rename(temp, target) {
            Ok(()) => return Ok(()),
            Err(error) => {
                if !error.is_transient() || attempt >= delays.len() {
                    return Err(error);
                }
                sleep(delays[attempt]);
                attempt += 1;
            }
        }
    }
}

/// 使用默认退避表与真实 `std::fs::rename` / 真实睡眠的版本。
pub fn replace_with_retry(temp: &Path, target: &Path) -> Result<(), ReplaceError> {
    replace_with_retry_with(
        temp,
        target,
        |from, to| std::fs::rename(from, to).map_err(ReplaceError::from),
        |millis| std::thread::sleep(std::time::Duration::from_millis(millis)),
        &DELAYS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    type RenameOutcome = Result<(), ReplaceError>;

    fn failing(codes: Vec<Option<&'static str>>) -> (Rc<RefCell<usize>>, impl FnMut(&Path, &Path) -> RenameOutcome) {
        let calls = Rc::new(RefCell::new(0usize));
        let counter = Rc::clone(&calls);
        let mut remaining = codes;
        remaining.reverse();
        let rename = move |_: &Path, _: &Path| -> Result<(), ReplaceError> {
            *counter.borrow_mut() += 1;
            match remaining.pop() {
                Some(Some(code)) => Err(ReplaceError::new(Some(code), code)),
                Some(None) => Ok(()),
                None => Ok(()),
            }
        };
        (calls, rename)
    }

    #[test]
    fn transient_conflict_is_retried_before_succeeding() {
        let (calls, rename) = failing(vec![Some("EPERM"), Some("EPERM"), None]);
        let waits = RefCell::new(Vec::<u64>::new());
        let result = replace_with_retry_with(
            Path::new("temp"),
            Path::new("target"),
            rename,
            |millis| waits.borrow_mut().push(millis),
            &DELAYS,
        );
        assert!(result.is_ok());
        assert_eq!(*calls.borrow(), 3);
        assert_eq!(*waits.borrow(), vec![50, 100]);
    }

    #[test]
    fn permanent_failures_are_not_retried() {
        let (calls, rename) = failing(vec![Some("ENOENT")]);
        let result = replace_with_retry_with(
            Path::new("temp"),
            Path::new("target"),
            rename,
            |_| panic!("不应重试非暂态错误"),
            &DELAYS,
        );
        assert_eq!(result.unwrap_err().code.as_deref(), Some("ENOENT"));
        assert_eq!(*calls.borrow(), 1, "缺失的目录或文件必须立即报错");
    }

    #[test]
    fn a_persistent_conflict_gives_up_after_five_retries() {
        let codes = vec![Some("EPERM"); 6];
        let (calls, rename) = failing(codes);
        let waits = RefCell::new(Vec::<u64>::new());
        let result = replace_with_retry_with(
            Path::new("temp"),
            Path::new("target"),
            rename,
            |millis| waits.borrow_mut().push(millis),
            &DELAYS,
        );
        assert_eq!(result.unwrap_err().code.as_deref(), Some("EPERM"));
        assert_eq!(*calls.borrow(), 6, "一次尝试加五次重试");
        assert_eq!(*waits.borrow(), DELAYS.to_vec());
    }

    #[test]
    fn eacces_and_ebusy_are_transient_too() {
        assert!(is_transient(Some("EACCES")));
        assert!(is_transient(Some("EBUSY")));
        assert!(!is_transient(Some("ENOENT")));
        assert!(!is_transient(None));
    }

    #[test]
    fn real_rename_replaces_the_target_and_recovers_from_missing_source() {
        let root = std::env::temp_dir().join(format!("wbbridge-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let temp = root.join("temp");
        let target = root.join("target");
        std::fs::write(&temp, "new").unwrap();
        std::fs::write(&target, "old").unwrap();

        replace_with_retry(&temp, &target).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");

        // 源文件缺失 → ENOENT，不重试也不静默成功。
        let missing = root.join("missing");
        let error = replace_with_retry(&missing, &target).unwrap_err();
        assert!(!error.is_transient());

        std::fs::remove_dir_all(&root).unwrap();
    }
}
