public enum ConnectionType: String, Encodable {
   case wifi
   case ethernet
   case cellular
   case unknown
}

/// Priority logic:
/// (wifi → ethernet → cellular → unknown)
public enum IosConnectivityMapper {
   public static func connectionType(
      hasWifi: Bool,
      hasEthernet: Bool,
      hasCellular: Bool
   ) -> ConnectionType {
      if hasWifi {
         return .wifi
      }

      if hasEthernet {
         return .ethernet
      }

      if hasCellular {
         return .cellular
      }

      // A satisfied path that exposes none of the interfaces above (for example
      // one using only `.other` or `.loopback`) resolves to `.unknown`.
      return .unknown
   }
}
