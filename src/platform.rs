#[cfg(not(target_os = "windows"))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(target_os = "windows"))]
pub use unsupported::connection_status;
#[cfg(target_os = "windows")]
pub use windows::connection_status;
