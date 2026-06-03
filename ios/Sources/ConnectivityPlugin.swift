import Foundation
import Network
import Tauri

private enum ConnectionTypePayload: String, Encodable {
   case wifi
   case ethernet
   case cellular
   case unknown
}

private struct ConnectionStatusPayload: Encodable {
   let connected: Bool
   let metered: Bool
   let constrained: Bool
   let connectionType: ConnectionTypePayload
}

class ConnectivityPlugin: Plugin {
   private let monitor = NWPathMonitor()
   private let monitorQueue = DispatchQueue(label: "tauri.plugin.connectivity.path")
   private let stateQueue = DispatchQueue(label: "tauri.plugin.connectivity.state")
   private var latestPath: NWPath?

   override init() {
      super.init()
      monitor.pathUpdateHandler = { [weak self] path in
         self?.stateQueue.async { self?.latestPath = path }
      }
      monitor.start(queue: monitorQueue)
   }

   deinit {
      monitor.cancel()
   }

   @objc public func connectionStatus(_ invoke: Invoke) throws {
      // `monitor.currentPath` may report `.requiresConnection` between `start()`
      // and the first pathUpdateHandler callback. Prefer the cached value, fall
      // back to currentPath only when no update has landed yet.
      let path = stateQueue.sync { latestPath } ?? monitor.currentPath
      let connectionType = Self.resolveConnectionType(path)

      invoke.resolve(ConnectionStatusPayload(
         connected: path.status == .satisfied,
         metered: path.isExpensive,
         constrained: path.isConstrained,
         connectionType: connectionType
      ))
   }

   private static func resolveConnectionType(_ path: NWPath) -> ConnectionTypePayload {
      if path.usesInterfaceType(.wifi) {
         return .wifi
      } else if path.usesInterfaceType(.wiredEthernet) {
         return .ethernet
      } else if path.usesInterfaceType(.cellular) {
         return .cellular
      }
      return .unknown
   }
}

@_cdecl("init_plugin_connectivity")
func initPlugin() -> Plugin {
   return ConnectivityPlugin()
}
