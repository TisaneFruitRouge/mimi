use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use mimi_protocol::{
    Event, Locality, ModelInfo, ModelPull, NewProvider, ProbeRequest, ProbeResult, Provider,
    ProviderKind, ProviderPreset, ProviderUpdate, PullRequest,
};
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::providers::store::{self, ProviderRecord};
use crate::providers::{self, Anthropic, OpenAiCompatible, ProviderError};
use crate::{AppState, now_ms, settings};

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<Vec<Provider>> {
    Ok(Json(store::list(&state.db).await?))
}

pub async fn presets() -> Json<Vec<ProviderPreset>> {
    Json(providers::presets())
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(new): Json<NewProvider>,
) -> ApiResult<Provider> {
    if new.kind == ProviderKind::Builtin {
        return Err(AppError::bad_request(
            "The built-in model source is part of Mimi; it can't be added by hand.",
        ));
    }
    let url = providers::parse_base_url(&new.base_url).map_err(AppError::bad_request)?;
    let record = ProviderRecord {
        provider: Provider {
            id: Uuid::now_v7(),
            name: valid_name(&new.name)?,
            kind: new.kind,
            base_url: providers::base_url_string(&url),
            locality: match new.kind {
                // Always a third-party service, whatever the address looks like.
                ProviderKind::Anthropic => Locality::Cloud,
                _ => new.locality.unwrap_or_else(|| providers::locality_of(&url)),
            },
            has_api_key: false,
            created_at: now_ms(),
        },
        api_key: clean_key(new.api_key),
    };
    store::upsert(&state.db, record.clone()).await?;
    publish(&state).await?;
    Ok(Json(Provider {
        has_api_key: record.api_key.is_some(),
        ..record.provider
    }))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(update): Json<ProviderUpdate>,
) -> ApiResult<Provider> {
    let mut record = store::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Provider"))?;
    if record.provider.kind == ProviderKind::Builtin {
        return Err(AppError::bad_request(
            "The built-in model source can't be changed.",
        ));
    }
    if let Some(name) = update.name {
        record.provider.name = valid_name(&name)?;
    }
    if let Some(base_url) = update.base_url {
        let url = providers::parse_base_url(&base_url).map_err(AppError::bad_request)?;
        record.provider.base_url = providers::base_url_string(&url);
        if update.locality.is_none() {
            record.provider.locality = providers::locality_of(&url);
        }
    }
    if let Some(locality) = update.locality {
        record.provider.locality = locality;
    }
    if record.provider.kind == ProviderKind::Anthropic {
        record.provider.locality = Locality::Cloud;
    }
    if let Some(key) = update.api_key {
        record.api_key = clean_key(Some(key));
    }
    record.provider.has_api_key = record.api_key.is_some();
    store::upsert(&state.db, record.clone()).await?;
    publish(&state).await?;
    Ok(Json(record.provider))
}

pub async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    if store::get(&state.db, id)
        .await?
        .is_some_and(|r| r.provider.kind == ProviderKind::Builtin)
    {
        return Err(AppError::bad_request(
            "The built-in model source is part of Mimi; remove its models instead.",
        ));
    }
    if !store::delete(&state.db, id).await? {
        return Err(AppError::not_found("Provider"));
    }
    let mut current = settings::load(&state.db).await?;
    if current
        .default_model
        .as_ref()
        .is_some_and(|m| m.provider_id == id)
    {
        current.default_model = None;
        settings::save(&state.db, &current).await?;
        state
            .events
            .publish(Event::SettingsChanged { settings: current });
    }
    publish(&state).await?;
    Ok(Json(()))
}

pub async fn models(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Vec<ModelInfo>> {
    let record = store::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Provider"))?;
    Ok(Json(providers::list_models(&state, &record).await?))
}

/// Tries connection details without saving them. If the address fails and has no path,
/// retries with `/v1` appended, since users often paste a server's bare address.
pub async fn probe(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ProbeRequest>,
) -> ApiResult<ProbeResult> {
    let url = providers::parse_base_url(&req.base_url).map_err(AppError::bad_request)?;
    let key = clean_key(req.api_key);
    match req.kind {
        ProviderKind::Builtin => {
            return Err(AppError::bad_request(
                "The built-in model source is part of Mimi; it can't be added by hand.",
            ));
        }
        ProviderKind::Anthropic => {
            if key.is_none() {
                return Err(AppError::bad_request("Anthropic needs an API key."));
            }
            let client = Anthropic::new(state.http.clone(), url.clone(), key);
            let models = client.list_models().await.map_err(|e| match e {
                ProviderError::Unauthorized => AppError::bad_request(
                    "Anthropic didn't accept this key. Check that you copied all of it.",
                ),
                other => other.into(),
            })?;
            return Ok(Json(ProbeResult {
                base_url: providers::base_url_string(&url),
                locality: Locality::Cloud,
                models,
            }));
        }
        ProviderKind::OpenaiCompatible => {}
    }
    let attempt = |url: reqwest::Url| {
        let client = OpenAiCompatible::new(state.http.clone(), url.clone(), key.clone());
        async move { client.list_models().await.map(|models| (url, models)) }
    };
    let result = match attempt(url.clone()).await {
        Err(ProviderError::Status(_) | ProviderError::Decode(_)) if url.path() == "/" => {
            attempt(url.join("v1").expect("joining a static path")).await
        }
        other => other,
    };
    let (url, models) = match result {
        Err(ProviderError::Unauthorized) if key.is_none() => {
            return Err(AppError::bad_request("This provider needs an API key."));
        }
        other => other?,
    };
    Ok(Json(ProbeResult {
        base_url: providers::base_url_string(&url),
        locality: providers::locality_of(&url),
        models,
    }))
}

async fn publish(state: &AppState) -> Result<(), AppError> {
    let providers = store::list(&state.db).await?;
    state.events.publish(Event::ProvidersChanged { providers });
    Ok(())
}

fn valid_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::bad_request(
            "The provider's name must be between 1 and 60 characters.",
        ));
    }
    Ok(name.to_owned())
}

fn clean_key(key: Option<String>) -> Option<String> {
    key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty())
}

/// Starts downloading a model through this source (Ollama only). Progress arrives as
/// `model_pull` events.
pub async fn pull(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PullRequest>,
) -> ApiResult<ModelPull> {
    let model = req.model.trim().to_owned();
    if !providers::pull::valid_model_name(&model) {
        return Err(AppError::bad_request("That isn't a valid model name."));
    }
    let record = store::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Provider"))?;
    if record.provider.kind == ProviderKind::Builtin {
        return crate::runtime::download::start(state.clone(), id, model)
            .map(Json)
            .map_err(AppError::bad_request);
    }
    let client = match providers::connect_openai(&state.http, &record) {
        Ok(client) if client.is_ollama().await => client,
        _ => {
            return Err(AppError::bad_request(
                "This model source can't download models. Downloads work through Ollama.",
            ));
        }
    };
    Ok(Json(providers::pull::start(
        state.clone(),
        client,
        id,
        model,
    )))
}

pub async fn pulls(State(state): State<Arc<AppState>>) -> Json<Vec<ModelPull>> {
    Json(state.pulls.running())
}

/// Stops a model download through the built-in source. What was downloaded is kept, so
/// starting it again resumes.
pub async fn cancel_pull(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<PullRequest>,
) -> ApiResult<()> {
    let record = store::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Provider"))?;
    if record.provider.kind != ProviderKind::Builtin || !state.downloads.cancel(req.model.trim()) {
        return Err(AppError::new(
            axum::http::StatusCode::CONFLICT,
            "not_downloading",
            "That model isn't downloading.",
        ));
    }
    Ok(Json(()))
}

/// Deletes a downloaded model from the built-in source, to free space.
pub async fn delete_model(
    State(state): State<Arc<AppState>>,
    Path((id, model)): Path<(Uuid, String)>,
) -> ApiResult<()> {
    let record = store::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Provider"))?;
    if record.provider.kind != ProviderKind::Builtin {
        return Err(AppError::bad_request(
            "Models from this source are managed by the app that provides them.",
        ));
    }
    let current = settings::load(&state.db).await?;
    if current
        .default_model
        .as_ref()
        .is_some_and(|m| m.provider_id == id && m.model == model)
    {
        return Err(AppError::bad_request(
            "Your assistant is using this model. Choose another one first.",
        ));
    }
    state.downloads.cancel(&model);
    state.runtime.stop_model(&state, &model).await;
    let removed =
        crate::runtime::download::remove(&state.paths, &model).map_err(AppError::internal)?;
    if !removed {
        return Err(AppError::not_found("Model"));
    }
    publish(&state).await?;
    state.events.publish(Event::RuntimeChanged {
        runtime: state.runtime.status(&state),
    });
    Ok(Json(()))
}

/// The built-in runtime: whether it's installed, what's loaded, space used.
pub async fn runtime(State(state): State<Arc<AppState>>) -> Json<mimi_protocol::RuntimeStatus> {
    Json(state.runtime.status(&state))
}
