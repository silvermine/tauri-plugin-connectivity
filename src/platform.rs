#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::connection_status;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub use unsupported::connection_status;
#[cfg(target_os = "windows")]
pub use windows::connection_status;
