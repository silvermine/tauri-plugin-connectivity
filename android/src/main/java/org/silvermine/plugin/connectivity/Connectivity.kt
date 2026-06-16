package org.silvermine.plugin.connectivity

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
import app.tauri.plugin.JSObject

class Connectivity(context: Context) {
   private val connectivityManager =
      context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager

   fun connectionStatus(): JSObject {
      val manager = connectivityManager ?: return NativeConnectionStatus.disconnected().toJSObject()
      val activeNetwork = manager.activeNetwork ?: return NativeConnectionStatus.disconnected().toJSObject()
      val capabilities = manager.getNetworkCapabilities(activeNetwork)
         ?: return NativeConnectionStatus.disconnected().toJSObject()

      val hasInternet = capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
      val isValidated = capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)

      if (!AndroidConnectivityMapper.isConnected(hasInternet)) {
         return NativeConnectionStatus.disconnected().toJSObject()
      }

      // `TEMPORARILY_NOT_METERED` means the active network should be treated as
      // effectively unmetered while Android exposes that capability.
      val hasTemporarilyNotMetered = hasTemporarilyNotMetered(capabilities)
      val metered = AndroidConnectivityMapper.isMetered(
         capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED),
         hasTemporarilyNotMetered
      )
      val status = NativeConnectionStatus(
         connected = true,
         metered = metered,
         constrained = AndroidConnectivityMapper.isConstrained(
            isValidated,
            isBackgroundRestricted(manager),
            metered
         ),
         connectionType = AndroidConnectivityMapper.connectionType(
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI),
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET),
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)
         )
      )

      return status.toJSObject()
   }

   private fun isBackgroundRestricted(manager: ConnectivityManager): Boolean {
      // Data Saver's restrict-background status was added after API 23.
      if (Build.VERSION.SDK_INT < Build.VERSION_CODES.N) {
         return false
      }

      return manager.restrictBackgroundStatus == ConnectivityManager.RESTRICT_BACKGROUND_STATUS_ENABLED
   }

   private fun hasTemporarilyNotMetered(capabilities: NetworkCapabilities): Boolean {
      // Guard the API 30 capability so the plugin still supports API 23+.
      if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
         return false
      }

      return capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_TEMPORARILY_NOT_METERED)
   }
}

data class NativeConnectionStatus(
   val connected: Boolean,
   val metered: Boolean,
   val constrained: Boolean,
   val connectionType: ConnectionType
) {
   fun toJSObject(): JSObject {
      val status = JSObject()

      status.put("connected", connected)
      status.put("metered", metered)
      status.put("constrained", constrained)
      status.put("connectionType", connectionType.serializedName)

      return status
   }

   companion object {
      fun disconnected(): NativeConnectionStatus {
         return NativeConnectionStatus(
            connected = false,
            metered = false,
            constrained = false,
            connectionType = ConnectionType.UNKNOWN
         )
      }
   }
}

enum class ConnectionType(val serializedName: String) {
   WIFI("wifi"),
   ETHERNET("ethernet"),
   CELLULAR("cellular"),
   UNKNOWN("unknown")
}

object AndroidConnectivityMapper {
   fun isConnected(hasInternet: Boolean): Boolean {
      return hasInternet
   }

   fun isMetered(hasNotMetered: Boolean, hasTemporarilyNotMetered: Boolean): Boolean {
      return !hasNotMetered && !hasTemporarilyNotMetered
   }

   fun isConstrained(
      isValidated: Boolean,
      isBackgroundRestricted: Boolean,
      isMetered: Boolean
   ): Boolean {
      // Unvalidated networks include captive portals and other limited paths.
      // Data Saver restricts background data only on metered networks.
      return !isValidated || (isBackgroundRestricted && isMetered)
   }

   fun connectionType(
      hasWifi: Boolean,
      hasEthernet: Boolean,
      hasCellular: Boolean
   ): ConnectionType {
      if (hasWifi) {
         return ConnectionType.WIFI
      }

      if (hasEthernet) {
         return ConnectionType.ETHERNET
      }

      if (hasCellular) {
         return ConnectionType.CELLULAR
      }

      return ConnectionType.UNKNOWN
   }
}
