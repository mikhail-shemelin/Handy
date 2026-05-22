use crate::managers::model::{ModelInfo, ModelManager};
use crate::managers::transcription::{ModelStateEvent, TranscriptionManager};
use crate::settings::{get_settings, write_settings, ModelUnloadTimeout, TranscriptionProvider};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
#[specta::specta]
pub async fn get_available_models(
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<Vec<ModelInfo>, String> {
    Ok(model_manager.get_available_models())
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_info(
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<Option<ModelInfo>, String> {
    Ok(model_manager.get_model_info(&model_id))
}

#[tauri::command]
#[specta::specta]
pub async fn download_model(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<(), String> {
    let result = model_manager
        .download_model(&model_id)
        .await
        .map_err(|e| e.to_string());

    if let Err(ref error) = result {
        let _ = app_handle.emit(
            "model-download-failed",
            serde_json::json!({ "model_id": &model_id, "error": error }),
        );
    }

    result
}

#[tauri::command]
#[specta::specta]
pub async fn delete_model(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    model_id: String,
) -> Result<(), String> {
    // If deleting the active model, unload it and clear the setting
    let settings = get_settings(&app_handle);
    if settings.selected_model == model_id {
        transcription_manager
            .unload_model()
            .map_err(|e| format!("Failed to unload model: {}", e))?;

        let mut settings = get_settings(&app_handle);
        settings.selected_model = String::new();
        write_settings(&app_handle, settings);
    }

    model_manager
        .delete_model(&model_id)
        .map_err(|e| e.to_string())
}

/// Shared logic for switching the active model, used by both the Tauri command
/// and the tray menu handler.
///
/// Validates the model, updates the persisted setting, and loads the model
/// unless the unload timeout is set to "Immediately" (in which case the model
/// will be loaded on-demand during the next transcription).
fn switch_active_model_internal(
    app: &AppHandle,
    model_id: &str,
    force_load: bool,
) -> Result<(), String> {
    let model_manager = app.state::<Arc<ModelManager>>();
    let transcription_manager = app.state::<Arc<TranscriptionManager>>();

    // Atomically claim the loading slot — prevents concurrent model loads
    // from tray double-clicks or overlapping commands. The guard resets the
    // flag on drop (including early returns, errors, and panics).
    let _loading_guard = transcription_manager
        .try_start_loading()
        .ok_or_else(|| "Model load already in progress".to_string())?;

    // Check if model exists and is available
    let model_info = model_manager
        .get_model_info(model_id)
        .ok_or_else(|| format!("Model not found: {}", model_id))?;

    if !model_info.is_downloaded {
        return Err(format!("Model not downloaded: {}", model_id));
    }

    let settings = get_settings(app);
    let unload_timeout = settings.model_unload_timeout;
    let old_model = settings.selected_model.clone();
    let old_provider = settings.transcription_provider;

    // Persist the new selection early so the frontend sees the correct model
    // when it reacts to events emitted by load_model.
    let mut settings = settings;
    settings.selected_model = model_id.to_string();
    settings.transcription_provider = TranscriptionProvider::Local;

    // Reset language to auto if the new model doesn't support the currently selected language.
    // This prevents stale language settings from causing errors (e.g. Canary receiving zh-Hans)
    // and stops downstream processing (e.g. OpenCC) from running on an irrelevant language.
    if settings.selected_language != "auto"
        && !model_info.supported_languages.is_empty()
        && !model_info
            .supported_languages
            .contains(&settings.selected_language)
    {
        log::info!(
            "Resetting language from '{}' to 'auto' (not supported by {})",
            settings.selected_language,
            model_id
        );
        settings.selected_language = "auto".to_string();
    }

    write_settings(app, settings);

    // Skip eager loading if unload is set to "Immediately" — the model
    // will be loaded on-demand during the next transcription.
    if unload_timeout == ModelUnloadTimeout::Immediately && !force_load {
        // Notify frontend — load_model won't be called so no events
        // would otherwise be emitted.
        let _ = app.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "selection_changed".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: Some(model_info.name.clone()),
                error: None,
            },
        );
        log::info!(
            "Model selection changed to {} (not loading — unload set to Immediately).",
            model_id
        );
        return Ok(());
    }

    // Load the model. On failure, revert the persisted selection.
    if let Err(e) = transcription_manager.load_model(model_id) {
        let mut latest_settings = get_settings(app);
        if latest_settings.transcription_provider == TranscriptionProvider::Local
            && latest_settings.selected_model == model_id
        {
            latest_settings.selected_model = old_model;
            latest_settings.transcription_provider = old_provider;
            write_settings(app, latest_settings);
        }
        return Err(e.to_string());
    }

    Ok(())
}

pub fn switch_active_model(app: &AppHandle, model_id: &str) -> Result<(), String> {
    switch_active_model_internal(app, model_id, false)
}

pub fn switch_active_model_and_load(app: &AppHandle, model_id: &str) -> Result<(), String> {
    switch_active_model_internal(app, model_id, true)
}

#[tauri::command]
#[specta::specta]
pub async fn set_active_model(
    app_handle: AppHandle,
    _model_manager: State<'_, Arc<ModelManager>>,
    _transcription_manager: State<'_, Arc<TranscriptionManager>>,
    model_id: String,
) -> Result<(), String> {
    switch_active_model(&app_handle, &model_id)
}

#[tauri::command]
#[specta::specta]
pub async fn auto_select_local_model_if_active_route_is_local(
    app_handle: AppHandle,
    _model_manager: State<'_, Arc<ModelManager>>,
    _transcription_manager: State<'_, Arc<TranscriptionManager>>,
    model_id: String,
) -> Result<bool, String> {
    let settings = get_settings(&app_handle);
    if settings.uses_openai_transcription() {
        return Ok(false);
    }

    switch_active_model(&app_handle, &model_id)?;
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub async fn get_current_model(app_handle: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app_handle);
    Ok(settings.selected_model)
}

#[tauri::command]
#[specta::specta]
pub async fn get_transcription_model_status(
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
) -> Result<Option<String>, String> {
    Ok(transcription_manager.get_current_model())
}

#[tauri::command]
#[specta::specta]
pub async fn is_model_loading(
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
) -> Result<bool, String> {
    // Check if transcription manager has a loaded model
    let current_model = transcription_manager.get_current_model();
    Ok(current_model.is_none())
}

fn has_available_transcription_route(
    settings: &crate::settings::AppSettings,
    models: &[ModelInfo],
    include_downloads: bool,
) -> bool {
    let local_available = models
        .iter()
        .any(|model| model.is_downloaded || (include_downloads && model.is_downloading));

    match settings.transcription_provider {
        TranscriptionProvider::Local => local_available,
        TranscriptionProvider::Openai => {
            settings.has_configured_openai_transcription() || local_available
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn has_any_models_available(
    app: tauri::AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<bool, String> {
    let settings = crate::settings::get_settings(&app);
    let models = model_manager.get_available_models();
    Ok(has_available_transcription_route(&settings, &models, false))
}

#[tauri::command]
#[specta::specta]
pub async fn has_any_models_or_downloads(
    app: tauri::AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<bool, String> {
    let settings = crate::settings::get_settings(&app);
    let models = model_manager.get_available_models();
    // Return true if any models are downloaded OR if any downloads are in progress
    Ok(has_available_transcription_route(&settings, &models, true))
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_download(
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<(), String> {
    model_manager
        .cancel_download(&model_id)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::model::EngineType;
    use crate::settings::{get_default_settings, OPENAI_TRANSCRIPTION_PROVIDER_ID};

    fn model(id: &str, is_downloaded: bool, is_downloading: bool) -> ModelInfo {
        ModelInfo {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            filename: format!("{id}.bin"),
            url: None,
            sha256: None,
            size_mb: 1,
            is_downloaded,
            is_downloading,
            partial_size: 0,
            is_directory: false,
            engine_type: EngineType::Whisper,
            accuracy_score: 0.0,
            speed_score: 0.0,
            supports_translation: false,
            is_recommended: false,
            supported_languages: Vec::new(),
            supports_language_selection: false,
            is_custom: false,
        }
    }

    #[test]
    fn route_availability_counts_downloaded_local_when_active_openai_is_invalid() {
        let mut settings = get_default_settings();
        settings.openai_transcription_enabled = true;
        settings.transcription_provider = TranscriptionProvider::Openai;

        assert!(settings.uses_openai_transcription());
        assert!(!settings.has_configured_openai_transcription());
        assert!(has_available_transcription_route(
            &settings,
            &[model("parakeet-v3", true, false)],
            false
        ));
    }

    #[test]
    fn route_availability_counts_active_configured_openai_even_without_local_models() {
        let mut settings = get_default_settings();
        settings.openai_transcription_enabled = true;
        settings.transcription_provider = TranscriptionProvider::Openai;
        settings.openai_transcription_api_keys.insert(
            OPENAI_TRANSCRIPTION_PROVIDER_ID.to_string(),
            "sk-test".to_string(),
        );

        assert!(has_available_transcription_route(&settings, &[], false));
    }

    #[test]
    fn route_availability_rejects_inactive_configured_openai_without_local_models() {
        let mut settings = get_default_settings();
        settings.openai_transcription_enabled = true;
        settings.transcription_provider = TranscriptionProvider::Local;
        settings.openai_transcription_api_keys.insert(
            OPENAI_TRANSCRIPTION_PROVIDER_ID.to_string(),
            "sk-test".to_string(),
        );

        assert!(!has_available_transcription_route(&settings, &[], false));
    }

    #[test]
    fn route_availability_rejects_unconfigured_openai_without_local_models() {
        let mut settings = get_default_settings();
        settings.openai_transcription_enabled = true;

        assert!(!has_available_transcription_route(&settings, &[], false));
    }

    #[test]
    fn route_availability_only_counts_downloads_when_requested() {
        let settings = get_default_settings();
        let models = [model("parakeet-v3", false, true)];

        assert!(!has_available_transcription_route(
            &settings, &models, false
        ));
        assert!(has_available_transcription_route(&settings, &models, true));
    }
}
