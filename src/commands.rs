use tauri::{AppHandle, Runtime, command};

use crate::error::Result;
use crate::types::ConnectionStatus;
use crate::{Error, platform};

/// Returns the current network connection status.
///
/// On platforms without an implementation, this returns [`Error::Unsupported`].
#[command]
pub(crate) async fn connection_status<R: Runtime>(_app: AppHandle<R>) -> Result<ConnectionStatus> {
   debug!("received frontend request for connection_status");

   let result = tauri::async_runtime::spawn_blocking(platform::connection_status)
      .await
      .map_err(|error| Error::DetectionFailed {
         message: format!("connection status worker failed: {error}"),
         code: None,
      })?;

   match result {
      Ok(status) => {
         debug!(?status, "returning connection status to frontend");
         Ok(status)
      }
      Err(error) => {
         warn!(%error, "failed to resolve connection status");
         Err(error)
      }
   }
}
