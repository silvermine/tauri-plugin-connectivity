use tauri::{AppHandle, Runtime, command};

use crate::ConnectivityExt;
use crate::error::Result;
use crate::types::ConnectionStatus;

/// Returns the current network connection status.
///
/// On platforms without an implementation, this returns [`Error::Unsupported`].
#[command]
pub(crate) async fn connection_status<R: Runtime>(app: AppHandle<R>) -> Result<ConnectionStatus> {
   app.connectivity().connection_status()
}
