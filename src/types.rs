use serde::{Deserialize, Serialize};

/// Describes the physical or logical transport used to connect to the network.
///
/// When multiple interfaces are active simultaneously (e.g. WiFi + Cellular),
/// this represents the preferred/primary transport as determined by the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Deduplicates known connection transport classes in stable API order.
#[derive(Debug, Default)]
pub(crate) struct ConnectionTypes {
   wifi: bool,
   ethernet: bool,
   cellular: bool,
}

impl ConnectionTypes {
   pub(crate) fn new() -> Self {
      Self::default()
   }

   pub(crate) fn insert(&mut self, connection_type: ConnectionType) {
      match connection_type {
         ConnectionType::Wifi => self.wifi = true,
         ConnectionType::Ethernet => self.ethernet = true,
         ConnectionType::Cellular => self.cellular = true,
         ConnectionType::Unknown => {}
      }
   }

   pub(crate) fn into_vec(self) -> Vec<ConnectionType> {
      let mut connection_types = Vec::with_capacity(3);

      if self.wifi {
         connection_types.push(ConnectionType::Wifi);
      }
      if self.ethernet {
         connection_types.push(ConnectionType::Ethernet);
      }
      if self.cellular {
         connection_types.push(ConnectionType::Cellular);
      }

      connection_types
   }
}

/// Information about the current network connection.
///
/// Combines reachability, cost/constraint flags, and the physical [`ConnectionType`]
/// to give callers enough context to make network policy decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
   /// Whether the device has an active network path.
   ///
   /// A connected path can still be limited or not fully usable. Check
   /// [`Self::constrained`] when the caller needs usable internet access.
   pub connected: bool,

   /// Whether data usage is billed or limited (e.g. mobile data plans, capped
   /// hotspots).
   ///
   /// Platform mapping:
   /// - **Windows:** `NetworkCostType` is `Unknown`, `Fixed`, or `Variable`
   /// - **Linux:** NetworkManager primary device `Metered` is `YES` or
   ///   `GUESS_YES`; passive fallback defaults to `false`
   /// - **iOS:** `NWPath.isExpensive`
   /// - **Android:** absence of `NET_CAPABILITY_NOT_METERED`
   pub metered: bool,

   /// Whether the connection is constrained -- approaching or over its data limit,
   /// roaming, or background data usage is restricted.
   ///
   /// Platform mapping:
   /// - **Windows:** `ConstrainedInternetAccess`, `ApproachingDataLimit`,
   ///   `OverDataLimit`, `Roaming`, or `BackgroundDataUsageRestricted`
   /// - **Linux:** NetworkManager `Connectivity` is `PORTAL` or `LIMITED`,
   ///   primary device is metered, or ModemManager reports cellular roaming;
   ///   passive fallback defaults to `false`
   /// - **iOS:** `NWPath.isConstrained` (Low Data Mode)
   /// - **Android:** missing `NET_CAPABILITY_VALIDATED`, or Data Saver /
   ///   `RESTRICT_BACKGROUND_STATUS` on a metered active network
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
         let json = serde_json::to_value(variant).unwrap();
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

   #[test]
   fn connection_types_dedupes_known_types_and_ignores_unknown() {
      let mut connection_types = ConnectionTypes::new();

      connection_types.insert(ConnectionType::Cellular);
      connection_types.insert(ConnectionType::Unknown);
      connection_types.insert(ConnectionType::Wifi);
      connection_types.insert(ConnectionType::Cellular);
      connection_types.insert(ConnectionType::Ethernet);

      assert_eq!(
         connection_types.into_vec(),
         vec![
            ConnectionType::Wifi,
            ConnectionType::Ethernet,
            ConnectionType::Cellular,
         ]
      );
   }

   #[test]
   fn connection_types_is_empty_when_only_unknown_was_inserted() {
      let mut connection_types = ConnectionTypes::new();

      connection_types.insert(ConnectionType::Unknown);

      assert!(connection_types.into_vec().is_empty());
   }
}
