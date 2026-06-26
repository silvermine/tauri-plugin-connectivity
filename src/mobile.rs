#[cfg(target_os = "android")]
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tauri::plugin::{PluginApi, PluginHandle};
use tauri::{AppHandle, Runtime};

use crate::types::{ConnectionStatus, ConnectionType};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "org.silvermine.plugin.connectivity";
#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_connectivity);

const COMMAND_CONNECTION_STATUS: &str = "connectionStatus";
#[cfg(target_os = "android")]
const COMMAND_SUPPORTED_CONNECTION_TYPES: &str = "supportedConnectionTypes";

#[cfg(target_os = "android")]
#[derive(Debug, Deserialize)]
struct MobileSupportedConnectionTypes {
   value: Vec<ConnectionType>,
}

/// Initializes the Rust-side bridge to the native mobile plugin.
pub fn init<R: Runtime, C: DeserializeOwned>(
   _app: &AppHandle<R>,
   api: PluginApi<R, C>,
) -> crate::Result<Connectivity<R>> {
   #[cfg(target_os = "android")]
   let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "ConnectivityPlugin")?;
   #[cfg(target_os = "ios")]
   let handle = api.register_ios_plugin(init_plugin_connectivity)?;

   Ok(Connectivity(handle))
}

/// Access to the mobile connectivity APIs.
pub struct Connectivity<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Connectivity<R> {
   /// Returns the current network connection status.
   pub fn connection_status(&self) -> crate::Result<ConnectionStatus> {
      // Calls the native `connectionStatus` command, implemented by
      // `ConnectivityPlugin` on Android and the iOS plugin on iOS.
      self
         .0
         .run_mobile_plugin(COMMAND_CONNECTION_STATUS, ())
         .map_err(Into::into)
   }

   /// Returns the supported physical connection transport classes.
   pub fn supported_connection_types(&self) -> crate::Result<Vec<ConnectionType>> {
      #[cfg(target_os = "android")]
      {
         let result: MobileSupportedConnectionTypes = self
            .0
            .run_mobile_plugin(COMMAND_SUPPORTED_CONNECTION_TYPES, ())
            .map_err(crate::Error::from)?;
         Ok(result.value)
      }

      #[cfg(not(target_os = "android"))]
      {
         Err(crate::Error::Unsupported)
      }
   }
}
