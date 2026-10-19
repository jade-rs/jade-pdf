//! 変換群のメモリ上限。対応外環境では指定を無視せず起動を拒否する。
/// 親から監督へ渡す専用群の絶対パス。通常変換の起動時には継承値を除去する。
pub(crate) const PARENT_GROUP_ENV: &str = "JADE_PDF2DOCX_INTERNAL_MEMORY_GROUP";

#[cfg(target_os = "linux")]
#[path = "memory_group_linux.rs"]
mod platform;

#[cfg(not(target_os = "linux"))]
mod platform {
    use std::{io, path::Path, process::Command};

    /// 対応外OSでは生成できないメモリ制限guard。
    pub(crate) struct MemoryGroup;

    /// 対応外OSで内部メモリ群の指定を無視しない停止guard。
    pub(crate) struct ParentDeath;

    impl ParentDeath {
        /// 未指定の場合だけ、通常のprocess group停止へ進める。
        pub(crate) fn from_env() -> io::Result<Option<Self>> {
            if std::env::var_os(super::PARENT_GROUP_ENV).is_some() {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "memory group requires Linux",
                ))
            } else {
                Ok(None)
            }
        }
        /// 対応外OSでは生成に失敗するため到達しない。
        pub(crate) fn terminate(&self) -> io::Result<()> {
            unreachable!()
        }
    }

    /// 対応外filesystemを通常directoryの削除で代用しない。
    pub(crate) fn cleanup(_: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "memory cgroup cleanup requires Linux cgroup v2",
        ))
    }

    impl MemoryGroup {
        /// Linux以外で制限の指定を黙って無視しない。
        pub(crate) fn new(_: &Path, _: u64) -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "memory limit requires Linux cgroup v2 with a delegated memory controller",
            ))
        }
        /// 対応外OSでは生成に失敗するため到達しない。
        pub(crate) fn configure(&self, _: &mut Command) -> io::Result<()> {
            unreachable!()
        }
        /// 対応外OSでは生成に失敗するため到達しない。
        pub(crate) fn exceeded(&self) -> io::Result<bool> {
            unreachable!()
        }
        /// 対応外OSでは生成に失敗するため到達しない。
        pub(crate) fn kill(&self) -> io::Result<()> {
            unreachable!()
        }
    }
}

pub(crate) use platform::{MemoryGroup, ParentDeath, cleanup};
