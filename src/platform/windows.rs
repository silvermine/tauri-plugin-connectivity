use tracing::{debug, warn};
use windows::Networking::Connectivity::{
   ConnectionCost, ConnectionProfile, NetworkConnectivityLevel, NetworkCostType, NetworkInformation,
};
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
   GAA_FLAG_INCLUDE_ALL_INTERFACES, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
   GAA_FLAG_SKIP_MULTICAST, GAA_FLAG_SKIP_UNICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::Networking::WinSock::AF_UNSPEC;

use crate::error::{Error, Result};
use crate::types::{ConnectionStatus, ConnectionType, ConnectionTypes};

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
   debug!("querying Windows internet connection profile");

   let profile = match NetworkInformation::GetInternetConnectionProfile() {
      Ok(profile) => profile,
      Err(error) if is_missing_profile_error(&error) => {
         debug!("Windows did not return an internet connection profile");
         return Ok(ConnectionStatus::disconnected());
      }
      Err(error) => {
         warn!(%error, "failed to query Windows internet connection profile");
         return Err(error.into());
      }
   };

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
      return Ok(ConnectionStatus::disconnected());
   }

   let connection_cost = profile
      .GetConnectionCost()
      .inspect_err(|error| warn!(%error, "failed to query Windows connection cost"))?;

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
      connected: true,
      metered: is_metered(cost_type),
      constrained,
      connection_type,
   };

   debug!(
      ?cost_type,
      constrained = status.constrained,
      connection_type = ?status.connection_type,
      metered = status.metered,
      "resolved Windows connection status"
   );

   Ok(status)
}

/// Returns the supported physical connection transport classes.
pub fn supported_connection_types() -> Result<Vec<ConnectionType>> {
   debug!("querying Windows supported connection types");

   // Use Win32 adapter enumeration rather than WinRT connection profiles:
   // `GetAdaptersAddresses` returns adapters present on the local computer,
   // while `NetworkInformation::GetConnectionProfiles()` can include saved
   // profiles that are not current hardware. The API and buffer contract are
   // documented here:
   // https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getadaptersaddresses
   Ok(collect_supported_connection_types(
      adapter_interface_types()?
   ))
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
      debug!("Windows classified the preferred profile as WLAN");
      return Ok(ConnectionType::Wifi);
   }

   if profile.IsWwanConnectionProfile()? {
      debug!("Windows classified the preferred profile as WWAN");
      return Ok(ConnectionType::Cellular);
   }

   let iana_interface_type = profile.NetworkAdapter()?.IanaInterfaceType()?;

   debug!(
      iana_interface_type,
      "falling back to IANA interface type for connection classification"
   );

   Ok(map_iana_interface_type(iana_interface_type))
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

fn collect_supported_connection_types(
   iana_interface_types: impl IntoIterator<Item = u32>,
) -> Vec<ConnectionType> {
   let mut connection_types = ConnectionTypes::new();

   for iana_interface_type in iana_interface_types {
      connection_types.insert(map_iana_interface_type(iana_interface_type));
   }

   connection_types.into_vec()
}

fn adapter_interface_types() -> Result<Vec<u32>> {
   // Microsoft recommends a 15 KB initial buffer to avoid repeated allocation
   // for typical adapter lists. If the buffer is still too small,
   // `ERROR_BUFFER_OVERFLOW` returns the required size.
   // https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getadaptersaddresses
   let mut size = 15 * 1024;
   let mut buffer = adapter_buffer(size);

   let mut result = unsafe {
      // `GAA_FLAG_INCLUDE_ALL_INTERFACES` includes adapters regardless of
      // operational state, matching the supported-hardware contract. The skip
      // flags avoid populating address lists we do not inspect.
      // https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getadaptersaddresses
      GetAdaptersAddresses(
         AF_UNSPEC.0.into(),
         GAA_FLAG_INCLUDE_ALL_INTERFACES
            | GAA_FLAG_SKIP_UNICAST
            | GAA_FLAG_SKIP_ANYCAST
            | GAA_FLAG_SKIP_MULTICAST
            | GAA_FLAG_SKIP_DNS_SERVER,
         None,
         Some(buffer.as_mut_ptr()),
         &mut size,
      )
   };

   if result == ERROR_BUFFER_OVERFLOW.0 {
      buffer = adapter_buffer(size);
      result = unsafe {
         GetAdaptersAddresses(
            AF_UNSPEC.0.into(),
            GAA_FLAG_INCLUDE_ALL_INTERFACES
               | GAA_FLAG_SKIP_UNICAST
               | GAA_FLAG_SKIP_ANYCAST
               | GAA_FLAG_SKIP_MULTICAST
               | GAA_FLAG_SKIP_DNS_SERVER,
            None,
            Some(buffer.as_mut_ptr()),
            &mut size,
         )
      };
   }

   if result != NO_ERROR.0 {
      return Err(Error::DetectionFailed {
         message: String::from("GetAdaptersAddresses failed"),
         code: Some(result as i32),
      });
   }

   let mut iana_interface_types = Vec::new();
   let mut adapter = buffer.as_ptr();

   while !adapter.is_null() {
      let adapter_ref = unsafe { &*adapter };
      iana_interface_types.push(adapter_ref.IfType);
      adapter = adapter_ref.Next;
   }

   Ok(iana_interface_types)
}

fn adapter_buffer(size_in_bytes: u32) -> Vec<IP_ADAPTER_ADDRESSES_LH> {
   let adapter_count =
      (size_in_bytes as usize).div_ceil(std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>());

   vec![IP_ADAPTER_ADDRESSES_LH::default(); adapter_count.max(1)]
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

   #[test]
   fn treats_empty_windows_error_as_missing_profile() {
      assert!(is_missing_profile_error(&Error::empty()));
   }

   #[test]
   fn does_not_treat_success_false_hresult_as_missing_profile() {
      assert!(!is_missing_profile_error(&Error::from_hresult(HRESULT(1))));
   }

   #[test]
   fn does_not_treat_failure_hresult_as_missing_profile() {
      assert!(!is_missing_profile_error(&Error::from_hresult(HRESULT(-1))));
   }

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

   #[test]
   fn collects_supported_connection_types_from_adapter_interface_types() {
      assert_eq!(
         collect_supported_connection_types([
            IANA_WWANPP,
            999,
            IANA_IEEE80211,
            IANA_WWANPP2,
            IANA_ETHERNET_CSMACD,
         ]),
         vec![
            ConnectionType::Wifi,
            ConnectionType::Ethernet,
            ConnectionType::Cellular,
         ]
      );
   }
}
