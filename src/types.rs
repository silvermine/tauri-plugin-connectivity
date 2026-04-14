use serde::{Deserialize, Serialize};

/// Describes the physical or logical transport used to connect to the network.
///
/// When multiple interfaces are active simultaneously (e.g. WiFi + Cellular),
/// this represents the preferred/primary transport as determined by the OS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionType {
   /// Connected via Wi-Fi.
   Wifi,

   /// Connected via Ethernet (wired).
   Ethernet,

   /// Connected via a cellular network (e.g. LTE, 5G).
   Cellular,

   /// The connection type could not be determined.
   Unknown,
}

/// Information about the current network connection.
///
/// Combines reachability, cost/constraint flags, and the physical [`ConnectionType`]
/// to give callers enough context to make network policy decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
   /// Whether the device has an active internet connection.
   pub connected: bool,

   /// Whether data usage is billed or limited (e.g. mobile data plans, capped
   /// hotspots).
   ///
   /// Platform mapping:
   /// - **Windows:** `NetworkCostType` is `Unknown`, `Fixed`, or `Variable`
   /// - **iOS:** `NWPath.isExpensive`
   /// - **Android:** absence of `NET_CAPABILITY_NOT_METERED`
   pub metered: bool,

   /// Whether the connection is constrained -- approaching or over its data limit,
   /// roaming, or background data usage is restricted.
   ///
   /// Platform mapping:
<<<<<<< Updated upstream
   /// - **Windows:** `ApproachingDataLimit`, `OverDataLimit`, or `Roaming`
=======
   /// - **Windows:** `ConstrainedInternetAccess`, `ApproachingDataLimit`,
   ///   `OverDataLimit`, `Roaming`, or `BackgroundDataUsageRestricted`
>>>>>>> Stashed changes
   /// - **iOS:** `NWPath.isConstrained` (Low Data Mode)
   /// - **Android:** Data Saver / `RESTRICT_BACKGROUND_STATUS`
   pub constrained: bool,

   /// The physical or logical transport used to connect to the network.
   pub connection_type: ConnectionType,
}

impl ConnectionStatus {
   /// Returns a [`ConnectionStatus`] representing a disconnected state.
   pub fn disconnected() -> Self {
      Self {
         connected: false,
         metered: false,
         constrained: false,
         connection_type: ConnectionType::Unknown,
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn serializes_connection_status() {
      let status = ConnectionStatus {
         connected: true,
         metered: true,
         constrained: false,
         connection_type: ConnectionType::Cellular,
      };
      let json = serde_json::to_value(&status).unwrap();

      assert_eq!(json["connected"], true);
      assert_eq!(json["metered"], true);
      assert_eq!(json["constrained"], false);
      assert_eq!(json["connectionType"], "cellular");
   }

   #[test]
   fn serializes_all_connection_types() {
      let cases = [
         (ConnectionType::Wifi, "wifi"),
         (ConnectionType::Ethernet, "ethernet"),
         (ConnectionType::Cellular, "cellular"),
         (ConnectionType::Unknown, "unknown"),
      ];

      for (variant, expected) in cases {
         let json = serde_json::to_value(&variant).unwrap();
         assert_eq!(json, expected);
      }
   }

   #[test]
   fn deserializes_connection_status() {
      let json =
         r#"{"connected":true,"metered":false,"constrained":false,"connectionType":"wifi"}"#;
      let status: ConnectionStatus = serde_json::from_str(json).unwrap();

      assert!(status.connected);
      assert!(!status.metered);
      assert!(!status.constrained);
      assert_eq!(status.connection_type, ConnectionType::Wifi);
   }

   #[test]
   fn deserializes_all_connection_types() {
      let cases = [
         ("\"wifi\"", ConnectionType::Wifi),
         ("\"ethernet\"", ConnectionType::Ethernet),
         ("\"cellular\"", ConnectionType::Cellular),
         ("\"unknown\"", ConnectionType::Unknown),
      ];

      for (json, expected) in cases {
         let result: ConnectionType = serde_json::from_str(json).unwrap();
         assert_eq!(result, expected);
      }
   }

   #[test]
   fn disconnected_returns_expected_defaults() {
      let status = ConnectionStatus::disconnected();

      assert!(!status.connected);
      assert!(!status.metered);
      assert!(!status.constrained);
      assert_eq!(status.connection_type, ConnectionType::Unknown);
   }
}
