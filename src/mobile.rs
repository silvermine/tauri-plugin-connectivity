use serde::de::DeserializeOwned;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::marker::PhantomData;
use tauri::plugin::PluginApi;
#[cfg(any(target_os = "android", target_os = "ios"))]
use tauri::plugin::PluginHandle;
use tauri::{AppHandle, Runtime};

use crate::types::ConnectionStatus;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "org.silvermine.plugin.connectivity";
#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_connectivity);

const COMMAND_CONNECTION_STATUS: &str = "connectionStatus";

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

   #[cfg(target_os = "ios")]
   {
      let handle = api
         .register_ios_plugin(init_plugin_connectivity)
         .map_err(|error| crate::Error::DetectionFailed {
            message: error.to_string(),
            code: None,
         })?;

      Ok(Connectivity::Native(handle))
   }

   #[cfg(not(any(target_os = "android", target_os = "ios")))]
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

   /// A registered native iOS plugin handle.
   #[cfg(target_os = "ios")]
   Native(PluginHandle<R>),

   /// A mobile platform supported by Tauri but not implemented by this plugin.
   #[cfg(not(any(target_os = "android", target_os = "ios")))]
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

         #[cfg(target_os = "ios")]
         Self::Native(handle) => handle
            .run_mobile_plugin(COMMAND_CONNECTION_STATUS, ())
            .map_err(|error| crate::Error::DetectionFailed {
               message: error.to_string(),
               code: None,
            }),

         #[cfg(not(any(target_os = "android", target_os = "ios")))]
         Self::Unsupported(_) => Err(crate::Error::Unsupported),
      }
   }
}
