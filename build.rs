const COMMANDS: &[&str] = &["connection_status", "supported_connection_types"];

fn main() {
   tauri_plugin::Builder::new(COMMANDS)
      .android_path("android")
      .ios_path("ios")
      .build();
}
