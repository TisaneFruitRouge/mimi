//! Settings › Phone: pairing phones, listing and removing them, choosing the relay.
//! Pairing codes and the relay are this computer's business: phones can't make codes
//! for other phones or move the relay from under themselves.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use mimi_protocol::{DeviceUpdate, PairRequest, Paired, PairingOffer, Relay, RemoteAccess};
use uuid::Uuid;

use super::Auth;
use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::remote::{self, Peer, store};

const NAME_LIMIT: usize = 60;

fn this_device(auth: Auth) -> Option<Uuid> {
    match auth {
        Auth::Device(id) => Some(id),
        _ => None,
    }
}

fn local_only(auth: Auth) -> Result<(), AppError> {
    match auth {
        Auth::Bearer | Auth::Session => Ok(()),
        Auth::Device(_) => Err(AppError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "This can only be changed on the computer Mimi runs on.",
        )),
    }
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
) -> ApiResult<RemoteAccess> {
    Ok(Json(remote::status(&state, this_device(auth)).await))
}

pub async fn offer(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
) -> ApiResult<PairingOffer> {
    local_only(auth)?;
    remote::offer(&state).await.map(Json).map_err(|e| {
        tracing::warn!("couldn't make a pairing code: {e:#}");
        AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "Phones can't reach this computer right now. Check that it's online, then try again.",
        )
    })
}

pub async fn cancel_offers(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
) -> ApiResult<()> {
    local_only(auth)?;
    remote::cancel_offers(&state).await;
    Ok(Json(()))
}

pub async fn set_relay(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Json(relay): Json<Relay>,
) -> ApiResult<RemoteAccess> {
    local_only(auth)?;
    let relay = remote::check_relay(&relay).map_err(AppError::bad_request)?;
    remote::set_relay(&state, relay)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(remote::status(&state, None).await))
}

/// A phone may rename or remove itself; this computer may rename or remove any.
fn may_manage(auth: Auth, id: Uuid) -> Result<(), AppError> {
    match auth {
        Auth::Device(own) if own != id => Err(AppError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Other phones can only be removed on the computer Mimi runs on.",
        )),
        _ => Ok(()),
    }
}

pub async fn rename(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<Uuid>,
    Json(update): Json<DeviceUpdate>,
) -> ApiResult<RemoteAccess> {
    may_manage(auth, id)?;
    let name = clean_name(&update.name)?;
    if !store::rename(&state.db, id, name).await? {
        return Err(AppError::not_found("Phone"));
    }
    remote::publish(&state).await;
    Ok(Json(remote::status(&state, this_device(auth)).await))
}

pub async fn remove(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
    Path(id): Path<Uuid>,
) -> ApiResult<()> {
    may_manage(auth, id)?;
    if !remote::remove(&state, id)
        .await
        .map_err(AppError::internal)?
    {
        return Err(AppError::not_found("Phone"));
    }
    Ok(Json(()))
}

/// A phone's first request: trades the code from the QR code for a token bound to the
/// phone's key. Only reachable over iroh (it sits outside the token check, so it insists
/// on a peer).
pub async fn pair(
    State(state): State<Arc<AppState>>,
    peer: Option<Extension<Peer>>,
    Json(req): Json<PairRequest>,
) -> ApiResult<Paired> {
    let Some(Extension(peer)) = peer else {
        return Err(AppError::not_found("Route"));
    };
    let name = clean_name(&req.name)?;
    if !state.remote.redeem(req.code.trim()) {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "pairing_expired",
            "This code has expired or was already used. Show a new one on your computer.",
        ));
    }
    let (device, token) = store::create(&state.db, name, peer.endpoint_id)
        .await
        .map_err(AppError::internal)?;
    tracing::info!(id = %device.id, "phone paired");
    remote::publish(&state).await;
    let device = remote::status(&state, Some(device.id))
        .await
        .devices
        .into_iter()
        .find(|d| d.id == device.id)
        .ok_or_else(|| AppError::internal("the new phone is missing"))?;
    Ok(Json(Paired { device, token }))
}

fn clean_name(name: &str) -> Result<String, AppError> {
    let name: String = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_LIMIT)
        .collect();
    if name.is_empty() {
        return Err(AppError::bad_request("Give the phone a name."));
    }
    Ok(name)
}
