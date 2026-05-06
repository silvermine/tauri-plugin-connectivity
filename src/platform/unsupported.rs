use crate::error::{Error, Result};
use crate::types::ConnectionStatus;
use tracing::warn;

/// Returns [`Error::Unsupported`] until a platform-specific implementation is added.
pub fn connection_status() -> Result<ConnectionStatus> {
   warn!("connection status detection is not supported on this platform");
   Err(Error::Unsupported)
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn returns_unsupported() {
      assert!(matches!(connection_status(), Err(Error::Unsupported)));
   }
}
