use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use futures::future::join_all;
use mimi_protocol::{DetectedServer, HardwareInfo, Locality, ModelRef, Recommendations};

use super::error::{ApiResult, AppError};
use crate::hardware::recommend::{self, AvailableModel};
use crate::providers::store;
use crate::providers::{self, OpenAiCompatible};
use crate::{AppState, hardware};

async fn detect() -> Result<HardwareInfo, AppError> {
    tokio::task::spawn_blocking(hardware::detect)
        .await
        .map_err(AppError::internal)
}

pub async fn get() -> ApiResult<HardwareInfo> {
    Ok(Json(detect().await?))
}

/// Unresponsive servers shouldn't hold up the setup screen.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn recommendations(State(state): State<Arc<AppState>>) -> ApiResult<Recommendations> {
    let hardware = detect().await?;
    let configured = store::list(&state.db).await?;

    // Models from every private provider, fetched in parallel.
    let private: Vec<_> = configured
        .iter()
        .filter(|p| p.locality != Locality::Cloud)
        .collect();
    let listings = join_all(private.iter().map(|p| async {
        let record = store::get(&state.db, p.id).await.ok().flatten()?;
        tokio::time::timeout(PROBE_TIMEOUT, providers::list_models(&state, &record))
            .await
            .ok()?
            .ok()
    }))
    .await;
    let available = private
        .iter()
        .zip(listings)
        .flat_map(|(p, models)| {
            models
                .unwrap_or_default()
                .into_iter()
                .map(|m| AvailableModel {
                    model: ModelRef {
                        provider_id: p.id,
                        model: m.id,
                    },
                    provider_name: p.name.clone(),
                    locality: p.locality,
                    size_bytes: m.size_bytes,
                })
        })
        .collect();

    // Local servers from the presets that answer right now.
    let presets: Vec<_> = providers::presets()
        .into_iter()
        .filter(|p| p.locality == Locality::Device)
        .collect();
    let probes = join_all(presets.iter().map(|p| async {
        let url = providers::parse_base_url(&p.base_url).ok()?;
        let client = OpenAiCompatible::new(state.http.clone(), url, None);
        tokio::time::timeout(PROBE_TIMEOUT, client.list_models())
            .await
            .ok()?
            .ok()
    }))
    .await;
    let detected = presets
        .into_iter()
        .zip(probes)
        .filter_map(|(p, models)| {
            Some(DetectedServer {
                already_added: configured.iter().any(|c| c.base_url == p.base_url),
                model_count: models?.len() as u32,
                preset_id: p.id,
                name: p.name,
                base_url: p.base_url,
            })
        })
        .collect();

    // The source that downloads models: Mimi's own runtime when it's installed, else the
    // first private Ollama, preferring this computer.
    let builtin = if state.runtime.available() {
        crate::runtime::builtin_source(&state).await
    } else {
        None
    };
    let mut download_provider_id = builtin;
    for locality in [Locality::Device, Locality::Network] {
        if download_provider_id.is_some() {
            break;
        }
        for p in private.iter().filter(|p| p.locality == locality) {
            let Ok(Some(record)) = store::get(&state.db, p.id).await else {
                continue;
            };
            if record.provider.kind == mimi_protocol::ProviderKind::OpenaiCompatible
                && let Ok(client) = providers::connect_openai(&state.http, &record)
                && client.is_ollama().await
            {
                download_provider_id = Some(p.id);
                break;
            }
        }
        if download_provider_id.is_some() {
            break;
        }
    }

    let mut rec = recommend::recommend(hardware, available, detected);
    // The built-in runtime downloads GGUF files, whose sizes differ a little from Ollama's.
    if builtin.is_some() {
        rec.suggested
            .retain_mut(|m| match recommend::gguf_source(&m.id) {
                Some(gguf) => {
                    m.download_bytes = gguf.bytes;
                    true
                }
                None => false,
            });
    }
    rec.download_provider_id = download_provider_id;
    Ok(Json(rec))
}

/// Every model in the catalog, so clients can show friendly names and descriptions.
pub async fn catalog() -> Json<Vec<mimi_protocol::CatalogModel>> {
    Json(recommend::catalog_models())
}

pub async fn integrations(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Vec<mimi_protocol::Integration>> {
    let connected: Vec<String> = crate::connections::store::list(&state.db)
        .await?
        .into_iter()
        .map(|c| c.integration)
        .collect();
    Ok(Json(crate::integrations::catalog_for(&connected)))
}
