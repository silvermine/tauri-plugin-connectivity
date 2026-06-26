use serde::{Deserialize, de::DeserializeOwned};
#[cfg(not(target_os = "android"))]
use std::marker::PhantomData;
use tauri::plugin::PluginApi;
#[cfg(target_os = "android")]
use tauri::plugin::PluginHandle;
use tauri::{AppHandle, Runtime};

use crate::types::{ConnectionStatus, ConnectionType};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "org.silvermine.plugin.connectivity";
#[cfg(target_os = "android")]
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
   {
      let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "ConnectivityPlugin")?;
      Ok(Connectivity::Native(handle))
   }

   #[cfg(not(target_os = "android"))]
   {
      let _ = api;

      Ok(Connectivity::Unsupported(PhantomData))
   }
}

/// Access to the mobile connectivity APIs.
pub enum Connectivity<R: Runtime> {
   /// A registered native Android plugin handle.
   #[cfg(target_os = "android")]
   Native(PluginHandle<R>),

   /// A mobile platform supported by Tauri but not implemented by this plugin.
   #[cfg(not(target_os = "android"))]
   Unsupported(PhantomData<R>),
}

impl<R: Runtime> Connectivity<R> {
   /// Returns the current network connection status.
   pub fn connection_status(&self) -> crate::Result<ConnectionStatus> {
      match self {
         #[cfg(target_os = "android")]
         Self::Native(handle) => {
            // Calls the native `connectionStatus` command, implemented by
            // `ConnectivityPlugin` on Android.
            handle
               .run_mobile_plugin(COMMAND_CONNECTION_STATUS, ())
               .map_err(Into::into)
         }

         #[cfg(not(target_os = "android"))]
         Self::Unsupported(_) => Err(crate::Error::Unsupported),
      }
   }

   /// Returns the supported physical connection transport classes.
   pub fn supported_connection_types(&self) -> crate::Result<Vec<ConnectionType>> {
      match self {
         #[cfg(target_os = "android")]
         Self::Native(handle) => {
            let result: MobileSupportedConnectionTypes = handle
               .run_mobile_plugin(COMMAND_SUPPORTED_CONNECTION_TYPES, ())
               .map_err(crate::Error::from)?;

            Ok(result.value)
         }

         #[cfg(not(target_os = "android"))]
         Self::Unsupported(_) => Err(crate::Error::Unsupported),
      }
   }
}
