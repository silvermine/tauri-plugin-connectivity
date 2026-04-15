use windows::Networking::Connectivity::{
   ConnectionCost, ConnectionProfile, NetworkConnectivityLevel, NetworkCostType, NetworkInformation,
};

use crate::error::Result;
use crate::types::{ConnectionStatus, ConnectionType};

/// [`IanaInterfaceType`](https://www.iana.org/assignments/ianaiftype-mib/ianaiftype-mib) values.
/// IANA interface type for Ethernet-like interfaces (`ethernetCsmacd`).
const IANA_ETHERNET_CSMACD: u32 = 6;
/// IANA interface type for IEEE 802.11 wireless LAN.
const IANA_IEEE80211: u32 = 71;
/// IANA interface types for WWAN mobile broadband transports.
const IANA_WWANPP: u32 = 243;
const IANA_WWANPP2: u32 = 244;

/// Returns the current network connection status using WinRT
/// [`NetworkInformation`](https://learn.microsoft.com/en-us/uwp/api/windows.networking.connectivity.networkinformation?view=winrt-28000).
///
/// Windows exposes a "preferred" internet profile rather than a single canonical
/// device-wide network. We query that profile and derive connectivity, cost, and
/// transport information from the resulting
/// [`ConnectionProfile`](https://learn.microsoft.com/en-us/uwp/api/windows.networking.connectivity.connectionprofile?view=winrt-28000).
pub fn connection_status() -> Result<ConnectionStatus> {
   let profile = match NetworkInformation::GetInternetConnectionProfile() {
      Ok(profile) => profile,
      Err(error) if is_missing_profile_error(&error) => {
         return Ok(ConnectionStatus::disconnected());
      }
      Err(error) => {
         return Err(error.into());
      }
   };

<<<<<<< Updated upstream
   if !has_internet_access(profile.GetNetworkConnectivityLevel()?) {
=======
   let connectivity_level = profile
      .GetNetworkConnectivityLevel()
      .inspect_err(|error| warn!(%error, "failed to query Windows connectivity level"))?;

   debug!(
      connectivity_level = ?connectivity_level,
      "queried Windows connectivity level"
   );

   if !has_network_connectivity(connectivity_level) {
      debug!(
         connectivity_level = ?connectivity_level,
         "connectivity level does not indicate internet or constrained access"
      );
>>>>>>> Stashed changes
      return Ok(ConnectionStatus::disconnected());
   }

   let connection_cost = profile.GetConnectionCost()?;

<<<<<<< Updated upstream
   Ok(ConnectionStatus {
=======
   let cost_type = connection_cost
      .NetworkCostType()
      .inspect_err(|error| warn!(%error, "failed to query Windows network cost type"))?;

   let constrained = is_constrained_connectivity(connectivity_level)
      || is_constrained_cost(&connection_cost).inspect_err(
         |error| warn!(%error, "failed to query Windows constrained connection flags"),
      )?;

   let connection_type = resolve_connection_type(&profile)
      .inspect_err(|error| warn!(%error, "failed to resolve Windows connection type"))?;

   let status = ConnectionStatus {
>>>>>>> Stashed changes
      connected: true,
      metered: is_metered(connection_cost.NetworkCostType()?),
      constrained: is_constrained(&connection_cost)?,
      connection_type: connection_type(&profile)?,
   })
}

/// The WinRT binding can return a success-coded error when the API succeeds but
/// does not provide a preferred internet connection profile. Treat only S_OK as
/// this missing-profile case; other success HRESULTs are informational results
/// that should not be silently swallowed.
fn is_missing_profile_error(error: &windows::core::Error) -> bool {
   error.code() == windows::core::HRESULT(0)
}

/// Treat full or constrained internet access as connected.
///
/// The returned [`ConnectionProfile`](https://learn.microsoft.com/en-us/uwp/api/windows.networking.connectivity.connectionprofile?view=winrt-28000)
/// can still represent local-only or constrained access. The plugin reports
/// constrained internet access as connected but constrained so captive portal
/// and similar limited-internet cases do not look fully offline.
///
/// Microsoft also notes that connectivity level is only a hint and apps should
/// re-check at the decision point rather than assume earlier results.
fn has_network_connectivity(connectivity_level: NetworkConnectivityLevel) -> bool {
   matches!(
      connectivity_level,
      NetworkConnectivityLevel::InternetAccess
         | NetworkConnectivityLevel::ConstrainedInternetAccess
   )
}

/// Windows reports metering through
/// [`ConnectionCost`](https://learn.microsoft.com/en-us/uwp/api/windows.networking.connectivity.connectioncost?view=winrt-28000).
/// We treat unknown, fixed-cost, and variable-cost plans as metered, and only
/// explicit unrestricted plans as not metered.
fn is_metered(cost_type: NetworkCostType) -> bool {
   matches!(
      cost_type,
      NetworkCostType::Unknown | NetworkCostType::Fixed | NetworkCostType::Variable
   )
}

/// Windows exposes several cost-related flags. We treat approaching/over-limit
/// roaming, and background data restrictions as constrained because callers use
/// this field for conservative network policy decisions. The relevant flags all
/// come from
/// [`ConnectionCost`](https://learn.microsoft.com/en-us/uwp/api/windows.networking.connectivity.connectioncost?view=winrt-28000).
fn is_constrained_cost(connection_cost: &ConnectionCost) -> Result<bool> {
   Ok(connection_cost.ApproachingDataLimit()?
      || connection_cost.OverDataLimit()?
      || connection_cost.Roaming()?
      || connection_cost.BackgroundDataUsageRestricted()?)
}

/// Windows reports captive portal and similar limited-internet cases through
/// [`NetworkConnectivityLevel::ConstrainedInternetAccess`].
fn is_constrained_connectivity(connectivity_level: NetworkConnectivityLevel) -> bool {
   connectivity_level == NetworkConnectivityLevel::ConstrainedInternetAccess
}

/// Prefer the explicit WLAN/WWAN profile checks first.
///
/// Windows already exposes higher-level transport classification on
/// [`ConnectionProfile`](https://learn.microsoft.com/en-us/uwp/api/windows.networking.connectivity.connectionprofile?view=winrt-28000).
/// The adapter interface type is only a fallback when those profile-level checks
/// do not classify the transport.
fn resolve_connection_type(profile: &ConnectionProfile) -> Result<ConnectionType> {
   if profile.IsWlanConnectionProfile()? {
      return Ok(ConnectionType::Wifi);
   }

   if profile.IsWwanConnectionProfile()? {
      return Ok(ConnectionType::Cellular);
   }

   Ok(map_iana_interface_type(
      profile.NetworkAdapter()?.IanaInterfaceType()?,
   ))
}

/// Maps the adapter's IANA interface type to a plugin-level transport.
///
/// IANA assigns standard numeric identifiers for network interface categories.
/// Windows surfaces those identifiers on the adapter, which lets us recognize
/// common transports when the higher-level profile checks do not classify the
/// transport.
///
/// References:
/// - IANA interface type registry: <https://www.iana.org/assignments/ianaiftype-mib/ianaiftype-mib>
/// - Windows `NetworkAdapter`: <https://learn.microsoft.com/en-us/uwp/api/windows.networking.connectivity.networkadapter?view=winrt-28000>
fn map_iana_interface_type(iana_interface_type: u32) -> ConnectionType {
   match iana_interface_type {
      IANA_ETHERNET_CSMACD => ConnectionType::Ethernet,
      IANA_IEEE80211 => ConnectionType::Wifi,
      IANA_WWANPP | IANA_WWANPP2 => ConnectionType::Cellular,
      _ => ConnectionType::Unknown,
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use windows::core::{Error, HRESULT};

   #[test]
   fn detects_internet_access_for_full_and_constrained_levels() {
      assert!(has_network_connectivity(
         NetworkConnectivityLevel::InternetAccess
      ));
      assert!(has_network_connectivity(
         NetworkConnectivityLevel::ConstrainedInternetAccess
      ));
      assert!(!has_network_connectivity(NetworkConnectivityLevel::None));
      assert!(!has_network_connectivity(
         NetworkConnectivityLevel::LocalAccess
      ));
   }

   #[test]
   fn treats_constrained_internet_access_as_constrained() {
      assert!(is_constrained_connectivity(
         NetworkConnectivityLevel::ConstrainedInternetAccess
      ));
      assert!(!is_constrained_connectivity(
         NetworkConnectivityLevel::InternetAccess
      ));
   }

   #[test]
   fn identifies_metered_cost_types() {
      assert!(is_metered(NetworkCostType::Unknown));
      assert!(!is_metered(NetworkCostType::Unrestricted));
      assert!(is_metered(NetworkCostType::Fixed));
      assert!(is_metered(NetworkCostType::Variable));
   }

<<<<<<< Updated upstream
    #[test]
    fn treats_empty_windows_error_as_missing_profile() {
       assert!(is_missing_profile_error(&Error::empty()));
    }

<<<<<<< Updated upstream
    #[test]
    fn does_not_treat_failure_hresult_as_missing_profile() {
       assert!(!is_missing_profile_error(&Error::from_hresult(HRESULT(-1))));
    }
=======
=======
   #[test]
   fn treats_empty_windows_error_as_missing_profile() {
      assert!(is_missing_profile_error(&Error::empty()));
   }

>>>>>>> Stashed changes
   #[test]
   fn does_not_treat_success_false_hresult_as_missing_profile() {
      assert!(!is_missing_profile_error(&Error::from_hresult(HRESULT(1))));
   }

   #[test]
   fn does_not_treat_failure_hresult_as_missing_profile() {
      assert!(!is_missing_profile_error(&Error::from_hresult(HRESULT(-1))));
   }
>>>>>>> Stashed changes

   #[test]
   fn maps_ethernet_interface_type() {
      assert_eq!(
         map_iana_interface_type(IANA_ETHERNET_CSMACD),
         ConnectionType::Ethernet
      );
   }

   #[test]
   fn maps_wifi_interface_type() {
      assert_eq!(
         map_iana_interface_type(IANA_IEEE80211),
         ConnectionType::Wifi
      );
   }

   #[test]
   fn maps_wwan_interface_types() {
      assert_eq!(
         map_iana_interface_type(IANA_WWANPP),
         ConnectionType::Cellular
      );
      assert_eq!(
         map_iana_interface_type(IANA_WWANPP2),
         ConnectionType::Cellular
      );
   }

   #[test]
   fn maps_unrecognized_interface_type_to_unknown() {
      assert_eq!(map_iana_interface_type(999), ConnectionType::Unknown);
   }
}
