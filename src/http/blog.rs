use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::blog::service::{
    BlogResponse, CreateBlogRequest, OriginsResponse, ReplaceOriginsRequest, UpdateBlogRequest,
};
use crate::error::Error;
use crate::http::AppState;
use crate::http::auth::{AuthedUser, AuthedWriter};
use crate::http::json_body::StrictJson;

pub async fn create(
    State(state): State<AppState>,
    AuthedWriter(user): AuthedWriter,
    StrictJson(body): StrictJson<CreateBlogRequest>,
) -> Result<(StatusCode, Json<BlogResponse>), Error> {
    let created = state.blogs.create(user.id, &body).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list(
    State(state): State<AppState>,
    session: AuthedUser,
) -> Result<Json<Vec<BlogResponse>>, Error> {
    Ok(Json(state.blogs.list_for_owner(session.user.id).await?))
}

pub async fn update(
    State(state): State<AppState>,
    AuthedWriter(user): AuthedWriter,
    Path(blog_id): Path<String>,
    StrictJson(body): StrictJson<UpdateBlogRequest>,
) -> Result<Json<BlogResponse>, Error> {
    Ok(Json(state.blogs.update(user.id, &blog_id, &body).await?))
}

pub async fn set_origins(
    State(state): State<AppState>,
    AuthedWriter(user): AuthedWriter,
    Path(blog_id): Path<String>,
    StrictJson(body): StrictJson<ReplaceOriginsRequest>,
) -> Result<Json<OriginsResponse>, Error> {
    let origins = state.blogs.set_origins(user.id, &blog_id, &body).await?;
    Ok(Json(OriginsResponse { origins }))
}
