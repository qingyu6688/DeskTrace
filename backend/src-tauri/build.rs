// 自定义命令必须在应用清单中逐个登记，才能由 capability 控制哪个窗口可以调用。
const COMMANDS: &[&str] = &[
    "get_bootstrap_status",
    "get_overview",
    "list_objects",
    "get_object_detail",
    "list_object_events",
    "refresh_application_catalog",
    "get_collector_status",
    "set_collection_state",
    "pick_folder_scope",
    "add_folder_scope",
    "add_desktop_scope",
    "list_folder_scopes",
    "update_folder_scope",
    "remove_folder_scope",
    "add_exclusion",
    "list_exclusions",
    "remove_exclusion",
    "get_settings",
    "update_settings",
    "open_object_location",
    "open_application_settings",
    "open_recent_items_settings",
    "request_export",
    "get_job_status",
    "cancel_job",
    "prepare_clear_usage_data",
    "clear_usage_data",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )?;
    Ok(())
}
