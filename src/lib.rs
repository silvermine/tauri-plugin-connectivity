mod commands;
mod error;
#[cfg(mobile)]
mod mobile;
#[cfg(desktop)]
mod platform;
mod types;

pub use error::{Error, Result};
pub use types::{ConnectionStatus, ConnectionType};

use tauri::{Manager, Runtime, plugin::TauriPlugin};
use tracing::debug;

#[cfg(mobile)]
use mobile::Connectivity;

/// Provides connectivity detection for the current platform.
///
/// This is the Rust-side API for querying connection status. On mobile this is
/// [`mobile::Connectivity`], which bridges to the native plugin via a
/// `PluginHandle` and therefore carries the `R: Runtime` generic required by
/// Tauri's mobile plugin bridge.
#[cfg(desktop)]
#[derive(Clone, Copy)]
pub struct Connectivity;

#[cfg(desktop)]
impl Connectivity {
   /// Returns the current network connection status.
   pub fn connection_status(&self) -> Result<ConnectionStatus> {
      debug!("querying connectivity status from plugin state");
      platform::connection_status()
   }
}

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to
/// access the connectivity APIs.
///
/// The trait is split by platform because the return type differs: desktop uses the
/// Tauri-agnostic `Connectivity`, while mobile delegates to the native plugin and so
/// returns `Connectivity<R>`.
#[cfg(desktop)]
pub trait ConnectivityExt<R: Runtime> {
   fn connectivity(&self) -> &Connectivity;
}

#[cfg(mobile)]
pub trait ConnectivityExt<R: Runtime> {
   fn connectivity(&self) -> &Connectivity<R>;
}

#[cfg(desktop)]
impl<R: Runtime, T: Manager<R>> ConnectivityExt<R> for T {
   fn connectivity(&self) -> &Connectivity {
      self.state::<Connectivity>().inner()
   }
}

#[cfg(mobile)]
impl<R: Runtime, T: Manager<R>> ConnectivityExt<R> for T {
   fn connectivity(&self) -> &Connectivity<R> {
      self.state::<Connectivity<R>>().inner()
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
         debug!("registering connectivity plugin state");

         #[cfg(desktop)]
         app.manage(Connectivity);

         #[cfg(mobile)]
         {
            let connectivity = mobile::init(app, _api)?;
            app.manage(connectivity);
         }

         Ok(())
      })
      .build()
}
