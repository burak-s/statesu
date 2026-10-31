use axum::Json;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::error::Error;
use crate::http::AppState;
use crate::http::auth::AuthedUser;
use crate::http::json_body::StrictJson;
use crate::post::service::{CreatePostRequest, ListResponse, PostResponse, UpdatePostRequest};

pub async fn create(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path(blog_id): Path<String>,
    StrictJson(body): StrictJson<CreatePostRequest>,
) -> Result<(StatusCode, Json<PostResponse>), Error> {
    let created = state.posts.create(user.id, &blog_id, &body).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path(blog_id): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Json<ListResponse<PostResponse>>, Error> {
    let query = parse_pagination_query(query.as_deref());
    Ok(Json(
        state
            .posts
            .page_for_blog(
                user.id,
                &blog_id,
                query.page.as_deref(),
                query.size.as_deref(),
            )
            .await?,
    ))
}

pub async fn get(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path((blog_id, post_id)): Path<(String, String)>,
) -> Result<Json<PostResponse>, Error> {
    let post = state.posts.get(user.id, &blog_id, &post_id).await?;
    Ok(Json(post))
}

pub async fn update(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path((blog_id, post_id)): Path<(String, String)>,
    StrictJson(body): StrictJson<UpdatePostRequest>,
) -> Result<Json<PostResponse>, Error> {
    let post = state
        .posts
        .update(user.id, &blog_id, &post_id, &body)
        .await?;
    Ok(Json(post))
}

pub async fn delete(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path((blog_id, post_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, Error> {
    state.posts.delete(user.id, &blog_id, &post_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Default)]
pub(crate) struct PaginationQuery {
    pub(crate) page: Option<String>,
    pub(crate) size: Option<String>,
}

pub(crate) fn parse_pagination_query(raw: Option<&str>) -> PaginationQuery {
    let mut query = PaginationQuery::default();
    let Some(raw) = raw else {
        return query;
    };
    for (key, value) in form_urlencoded::parse(raw.as_bytes()) {
        match key.as_ref() {
            "page" if query.page.is_none() => query.page = Some(value.into_owned()),
            "size" if query.size.is_none() => query.size = Some(value.into_owned()),
            _ => {}
        }
    }
    query
}
