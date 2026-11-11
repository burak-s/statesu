//! HTTP surface for media storage: user R2 connections and the per-blog
//! storage preference. Handlers stay thin; rules live in the media service.
//! The whole secret-management surface is session-only by design: personal
//! access tokens never touch storage credentials.

use axum::Json;
use axum::extract::{Path, RawQuery, Request, State};
use axum::http::StatusCode;
use http_body_util::BodyExt;

use crate::error::Error;
use crate::http::AppState;
use crate::http::auth::{AuthedSession, AuthedUser, AuthedWriter};
use crate::http::json_body::StrictJson;
use crate::http::post::parse_pagination_query;
use crate::media::service::MediaResponse;
use crate::media::{
    BlogStorageResponse, CreateConnectionRequest, ReplaceStorageRequest, StorageConnectionResponse,
    UpdateConnectionRequest,
};
use crate::post::service::ListResponse;

/// Raw binary upload; the request body is the image itself. The media
/// service validates the bytes, so a mismatched Content-Type header is
/// irrelevant (and clients need not agree on one).
pub async fn create_media(
    State(state): State<AppState>,
    AuthedWriter(user): AuthedWriter,
    Path(blog_id): Path<String>,
    request: Request,
) -> Result<(StatusCode, Json<MediaResponse>), Error> {
    // Charge attempts, not stored objects: deleting an upload, changing
    // blogs/PATs or sending invalid files must not refill this budget.
    // Reject before buffering the body or doing any codec/storage work.
    state
        .limiters
        .media_user
        .take_slot(user.id)
        .map_err(|retry_after_secs| Error::RateLimited { retry_after_secs })?;
    // The body is read here instead of through the `Bytes` extractor:
    // that one silently caps at 2 MiB regardless of configuration, and
    // the upload limit must honor STATESU_MAX_MEDIA_BYTES. The middleware
    // already bounds the request with the same limit, so collection stops
    // against the configured surface, never an extractor default.
    let body = request
        .into_body()
        .collect()
        .await
        .map_err(|_| Error::InvalidBody)?
        .to_bytes();
    let created = state.media.upload(&user.id, &blog_id, &body).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list_media(
    State(state): State<AppState>,
    session: AuthedUser,
    Path(blog_id): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Json<ListResponse<MediaResponse>>, Error> {
    let query = parse_pagination_query(query.as_deref());
    Ok(Json(
        state
            .media
            .page_for_blog(
                &session.user.id,
                &blog_id,
                query.page.as_deref(),
                query.size.as_deref(),
            )
            .await?,
    ))
}

pub async fn delete_media(
    State(state): State<AppState>,
    AuthedWriter(user): AuthedWriter,
    Path((blog_id, media_id)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    state
        .media
        .delete_media(&user.id, &blog_id, &media_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_connection(
    State(state): State<AppState>,
    AuthedSession(user): AuthedSession,
    Path(blog_id): Path<String>,
    StrictJson(body): StrictJson<CreateConnectionRequest>,
) -> Result<(StatusCode, Json<StorageConnectionResponse>), Error> {
    let created = state
        .media
        .create_connection(&user.id, &blog_id, &body)
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list_connections(
    State(state): State<AppState>,
    AuthedSession(user): AuthedSession,
    Path(blog_id): Path<String>,
) -> Result<Json<Vec<StorageConnectionResponse>>, Error> {
    Ok(Json(
        state.media.list_connections(&user.id, &blog_id).await?,
    ))
}

pub async fn update_connection(
    State(state): State<AppState>,
    AuthedSession(user): AuthedSession,
    Path((blog_id, connection_id)): Path<(String, String)>,
    StrictJson(body): StrictJson<UpdateConnectionRequest>,
) -> Result<Json<StorageConnectionResponse>, Error> {
    let updated = state
        .media
        .update_connection(&user.id, &blog_id, &connection_id, &body)
        .await?;
    Ok(Json(updated))
}

pub async fn delete_connection(
    State(state): State<AppState>,
    AuthedSession(user): AuthedSession,
    Path((blog_id, connection_id)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    state
        .media
        .delete_connection(&user.id, &blog_id, &connection_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn blog_storage(
    State(state): State<AppState>,
    AuthedSession(user): AuthedSession,
    Path(blog_id): Path<String>,
) -> Result<Json<BlogStorageResponse>, Error> {
    Ok(Json(state.media.blog_storage(&user.id, &blog_id).await?))
}

pub async fn set_blog_storage(
    State(state): State<AppState>,
    AuthedSession(user): AuthedSession,
    Path(blog_id): Path<String>,
    StrictJson(body): StrictJson<ReplaceStorageRequest>,
) -> Result<Json<BlogStorageResponse>, Error> {
    let preference = state
        .media
        .set_blog_storage(&user.id, &blog_id, &body)
        .await?;
    Ok(Json(preference))
}
