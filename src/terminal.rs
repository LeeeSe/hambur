use anyhow::Result;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

/// 使用RAII模式管理终端的原始模式
/// 在创建时启用原始模式，在作用域结束时自动禁用原始模式
pub struct RawModeGuard;

impl RawModeGuard {
    /// 进入原始模式并返回一个守卫实例
    /// 当守卫实例被丢弃时，原始模式会自动被禁用
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // 忽略可能的错误，因为在drop实现中无法返回错误
        let _ = disable_raw_mode();
    }
}