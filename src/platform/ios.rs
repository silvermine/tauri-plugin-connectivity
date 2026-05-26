use serde::de::DeserializeOwned;
use tauri::{
   Runtime,
   plugin::{PluginApi, PluginHandle},
};

use crate::error::{Error, Result};
use crate::types::ConnectionStatus;

tauri::ios_plugin_binding!(init_plugin_connectivity);

pub(crate) struct IosConnectivity<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Clone for IosConnectivity<R> {
   fn clone(&self) -> Self {
      Self(self.0.clone())
   }
}

impl<R: Runtime> IosConnectivity<R> {
   pub(crate) fn new<C: DeserializeOwned>(api: PluginApi<R, C>) -> Result<Self> {
      let handle = api
         .register_ios_plugin(init_plugin_connectivity)
         .map_err(|error| Error::DetectionFailed {
            message: error.to_string(),
            code: None,
         })?;

      Ok(Self(handle))
   }

   pub(crate) fn connection_status(&self) -> Result<ConnectionStatus> {
      self
         .0
         .run_mobile_plugin("connectionStatus", ())
         .map_err(|error| Error::DetectionFailed {
            message: error.to_string(),
            code: None,
         })
   }
}
