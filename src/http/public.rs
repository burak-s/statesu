use axum::Json;
use axum::extract::{Path, RawQuery, State};

use crate::blog::service::PublicBlogResponse;
use crate::error::Error;
use crate::http::AppState;
use crate::http::post::parse_pagination_query;
use crate::post::service::{ListResponse, PublicPostResponse};

/// Public delivery endpoints: unauthenticated reads of one blog's published
/// posts. Privacy comes from publication state, not transport; CORS is
/// handled by the guard middleware per blog.
pub async fn blog(
    State(state): State<AppState>,
    Path(blog_slug): Path<String>,
) -> Result<Json<PublicBlogResponse>, Error> {
    Ok(Json(state.blogs.public_blog(&blog_slug).await?))
}

pub async fn posts(
    State(state): State<AppState>,
    Path(blog_slug): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Json<ListResponse<PublicPostResponse>>, Error> {
    let query = parse_pagination_query(query.as_deref());
    Ok(Json(
        state
            .posts
            .public_page(&blog_slug, query.page.as_deref(), query.size.as_deref())
            .await?,
    ))
}

pub async fn post(
    State(state): State<AppState>,
    Path((blog_slug, post_slug)): Path<(String, String)>,
) -> Result<Json<PublicPostResponse>, Error> {
    Ok(Json(state.posts.public_post(&blog_slug, &post_slug).await?))
}
