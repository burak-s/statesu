use axum::Json;
use axum::extract::{Path, RawQuery, State};

use crate::blog::service::PublicBlogResponse;
use crate::error::Error;
use crate::http::AppState;
use crate::http::post::parse_pagination_query;
use crate::media::MediaPublicMeta;
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
) -> Result<Json<ListResponse<PublicPostEnvelope>>, Error> {
    let query = parse_pagination_query(query.as_deref());
    let (listing, blog_id) = state
        .posts
        .public_page(&blog_slug, query.page.as_deref(), query.size.as_deref())
        .await?;
    // One metadata pass over every item's Markdown; containment filtering
    // stays cheap because the media id scan caps candidates per request.
    let contents: Vec<&str> = listing
        .items
        .iter()
        .map(|post| post.content_markdown.as_str())
        .collect();
    let metadata = state.media.public_metadata(&blog_id, &contents).await?;
    let items = listing
        .items
        .into_iter()
        .map(|post| referenced_envelope(post, &metadata))
        .collect();
    Ok(Json(ListResponse {
        items,
        page: listing.page,
        size: listing.size,
        total: listing.total,
    }))
}

pub async fn post(
    State(state): State<AppState>,
    Path((blog_slug, post_slug)): Path<(String, String)>,
) -> Result<Json<PublicPostEnvelope>, Error> {
    let (post, blog_id) = state.posts.public_post(&blog_slug, &post_slug).await?;
    let media = state
        .media
        .public_metadata(&blog_id, &[post.content_markdown.as_str()])
        .await?;
    Ok(Json(PublicPostEnvelope { post, media }))
}

/// Public post payload: the stored Markdown plus, additively, the metadata
/// for any referenced media the delivery system itself knows about.
#[derive(Debug, serde::Serialize)]
pub struct PublicPostEnvelope {
    #[serde(flatten)]
    pub post: PublicPostResponse,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<MediaPublicMeta>,
}

/// Keeps only metadata whose canonical source URL appears in this post's
/// Markdown; one containment scan per item over the tiny candidate set.
fn referenced_envelope(
    post: PublicPostResponse,
    metadata: &[MediaPublicMeta],
) -> PublicPostEnvelope {
    let referencing = metadata
        .iter()
        .filter(|meta| post.content_markdown.contains(&meta.source_url))
        .cloned()
        .collect();
    PublicPostEnvelope {
        post,
        media: referencing,
    }
}
