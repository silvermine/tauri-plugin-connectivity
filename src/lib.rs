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

/// Provides connectivity detection for the current platform.
///
/// This is the Rust-side API for querying connection status. Platform-specific
/// implementations will be added behind this interface.
#[cfg(desktop)]
pub struct Connectivity;

#[cfg(mobile)]
pub struct Connectivity<R: Runtime>(mobile::Connectivity<R>);

#[cfg(desktop)]
impl Connectivity {
   /// Returns the current network connection status.
   pub fn connection_status(&self) -> Result<ConnectionStatus> {
      debug!("querying connectivity status from plugin state");
      platform::connection_status()
   }

   /// Returns the supported physical connection transport classes.
   pub fn supported_connection_types(&self) -> Result<Vec<ConnectionType>> {
      debug!("querying supported connection types from plugin state");
      platform::supported_connection_types()
   }
}

#[cfg(mobile)]
impl<R: Runtime> Connectivity<R> {
   /// Returns the current network connection status.
   pub fn connection_status(&self) -> Result<ConnectionStatus> {
      debug!("querying mobile connectivity status from plugin state");
      self.0.connection_status()
   }

   /// Returns the supported physical connection transport classes.
   pub fn supported_connection_types(&self) -> Result<Vec<ConnectionType>> {
      debug!("querying mobile supported connection types from plugin state");
      self.0.supported_connection_types()
   }
}

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to
/// access the connectivity APIs.
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
      .invoke_handler(tauri::generate_handler![
         commands::connection_status,
         commands::supported_connection_types
      ])
      .setup(|app, _api| {
         debug!("registering connectivity plugin state");

         #[cfg(desktop)]
         app.manage(Connectivity);

         #[cfg(target_os = "macos")]
         {
            // Start the path monitor early. Its first update arrives
            // asynchronously, so this does not guarantee a populated cache on
            // return — it ensures the update has normally landed long before
            // the webview loads and the frontend makes its first call. Until
            // then, reads report disconnected (see `platform::macos`).
            let _ = platform::connection_status();
         }

         #[cfg(mobile)]
         {
            let connectivity = mobile::init(app, _api)?;
            app.manage(Connectivity(connectivity));
         }
         Ok(())
      })
      .build()
}
