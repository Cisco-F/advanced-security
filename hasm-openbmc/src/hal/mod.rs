//! Hardware abstraction layer entry points.
//!
//! The current HAL surface is intentionally small: `init` owns clock-tree and
//! peripheral-token setup before the board-specific drivers are constructed.

pub mod init;
