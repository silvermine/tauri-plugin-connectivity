const COMMANDS: &[&str] = &["connection_status"];

fn main() {
   tauri_plugin::Builder::new(COMMANDS).build();
}
