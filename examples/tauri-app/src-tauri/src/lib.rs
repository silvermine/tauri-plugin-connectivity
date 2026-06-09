#[cfg(debug_assertions)]
const DEFAULT_LOG_FILTER: &str = "warn,tauri_plugin_connectivity=debug";
#[cfg(not(debug_assertions))]
const DEFAULT_LOG_FILTER: &str = "warn";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
   init_logging();

   tauri::Builder::default()
      .plugin(tauri_plugin_connectivity::init())
      .run(tauri::generate_context!())
      .expect("error while running connectivity example");
}

fn init_logging() {
   let filter = tracing_subscriber::EnvFilter::try_from_default_env()
      .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER));

   let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
