package org.silvermine.plugin.connectivity

import android.app.Activity
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

// Android side of the Rust `register_android_plugin(..., "ConnectivityPlugin")`
// bridge.
@TauriPlugin
class ConnectivityPlugin(activity: Activity) : Plugin(activity) {
   private val connectivity = Connectivity(activity.applicationContext)

   @Command
   fun connectionStatus(invoke: Invoke) {
      invoke.resolve(connectivity.connectionStatus())
   }
}
