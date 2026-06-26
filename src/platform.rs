//! Rust-native desktop connectivity backends.
//!
//! Mobile platforms use Tauri's native plugin bridge through `src/mobile.rs`
//! instead of this module.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(
   target_os = "macos",
   not(any(target_os = "linux", target_os = "macos", target_os = "windows"))
))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::connection_status;
#[cfg(target_os = "linux")]
pub use linux::supported_connection_types;
#[cfg(target_os = "macos")]
pub use macos::connection_status;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub use unsupported::connection_status;
#[cfg(target_os = "macos")]
pub use unsupported::supported_connection_types;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub use unsupported::supported_connection_types;
#[cfg(target_os = "windows")]
pub use windows::connection_status;
#[cfg(target_os = "windows")]
pub use windows::supported_connection_types;
