use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use futures::future::join_all;
use hearth_protocol::{DetectedServer, HardwareInfo, Locality, ModelRef, Recommendations};

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
        let client = providers::connect(&state.http, &record).ok()?;
        tokio::time::timeout(PROBE_TIMEOUT, client.list_models())
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

    Ok(Json(recommend::recommend(hardware, available, detected)))
}
