mod commands;
mod error;
mod platform;
mod types;

pub use error::{Error, Result};
pub use types::{ConnectionStatus, ConnectionType};

use tauri::{Manager, Runtime, plugin::TauriPlugin};

/// Provides connectivity detection for the current platform.
///
/// This is the Rust-side API for querying connection status. Platform-specific
/// implementations will be added behind this interface.
pub struct Connectivity;

impl Connectivity {
   /// Returns the current network connection status.
   pub fn connection_status(&self) -> Result<ConnectionStatus> {
      platform::connection_status()
   }
}

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to
/// access the connectivity APIs.
pub trait ConnectivityExt<R: Runtime> {
   fn connectivity(&self) -> &Connectivity;
}

impl<R: Runtime, T: Manager<R>> ConnectivityExt<R> for T {
   fn connectivity(&self) -> &Connectivity {
      self.state::<Connectivity>().inner()
   }
}

/// Initializes the connectivity plugin.
///
/// # Examples
///
/// ```no_run
/// tauri::Builder::default()
///    .plugin(tauri_plugin_connectivity::init())
///    .run(tauri::generate_context!())
///    .expect("error while running tauri application");
/// ```
pub fn init<R: Runtime>() -> TauriPlugin<R> {
   tauri::plugin::Builder::new("connectivity")
      .invoke_handler(tauri::generate_handler![commands::connection_status])
      .setup(|app, _api| {
         app.manage(Connectivity);
         Ok(())
      })
      .build()
}
