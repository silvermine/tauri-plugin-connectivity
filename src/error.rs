use std::fmt;

use serde::{Serialize, ser::Serializer};

/// Errors that can occur when detecting connection status.
#[derive(Debug)]
pub enum Error {
   /// The current platform does not support connection status detection.
   Unsupported,

   /// The platform-specific backend failed while detecting connection status.
   DetectionFailed { message: String, code: Option<i32> },
}

impl fmt::Display for Error {
   fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
      match self {
         Self::Unsupported => {
            formatter.write_str("connection status detection is not supported on this platform")
         }
         Self::DetectionFailed {
            message,
            code: Some(code),
         } => {
            write!(
               formatter,
               "connection status detection failed with native error code {code}: {message}"
            )
         }
         Self::DetectionFailed {
            message,
            code: None,
         } => {
            write!(formatter, "connection status detection failed: {message}")
         }
      }
   }
}

impl std::error::Error for Error {}

impl Serialize for Error {
   fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
   where
      S: Serializer,
   {
      serializer.serialize_str(self.to_string().as_ref())
   }
}

/// A specialized [`Result`] type for connectivity operations.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(target_os = "windows")]
impl From<windows::core::Error> for Error {
   fn from(value: windows::core::Error) -> Self {
      Self::DetectionFailed {
         message: value.to_string(),
         code: Some(value.code().0),
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn unsupported_error_displays_message() {
      let err = Error::Unsupported;

      assert_eq!(
         err.to_string(),
         "connection status detection is not supported on this platform"
      );
   }

   #[test]
   fn error_serializes_to_string() {
      let err = Error::Unsupported;
      let json = serde_json::to_value(&err).unwrap();

      assert_eq!(
         json,
         "connection status detection is not supported on this platform"
      );
   }

   #[test]
   fn detection_failed_error_displays_message() {
      let err = Error::DetectionFailed {
         message: String::from("backend unavailable"),
         code: None,
      };

      assert_eq!(
         err.to_string(),
         "connection status detection failed: backend unavailable"
      );
   }

   #[test]
   fn detection_failed_error_serializes_to_string() {
      let err = Error::DetectionFailed {
         message: String::from("backend unavailable"),
         code: Some(-1),
      };
      let json = serde_json::to_value(&err).unwrap();

      assert_eq!(
         json,
         "connection status detection failed with native error code -1: backend unavailable"
      );
   }

   #[cfg(target_os = "windows")]
   #[test]
   fn windows_error_preserves_hresult_code() {
      let err = Error::from(windows::core::Error::from_hresult(windows::core::HRESULT(
         -1,
      )));

      assert!(matches!(err, Error::DetectionFailed { code: Some(-1), .. }));
   }
}
