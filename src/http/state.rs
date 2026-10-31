use axum::Json;
use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::error::Error;
use crate::http::AppState;
use crate::http::auth::AuthedUser;
use crate::http::json_body::StrictJson;
use crate::state::service::{CreateStateRequest, LatestStateResponse, ListResponse, StateResponse};

pub async fn create(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    StrictJson(body): StrictJson<CreateStateRequest>,
) -> Result<(StatusCode, Json<StateResponse>), Error> {
    let created = state.states.create(user.id, &body).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Json<ListResponse>, Error> {
    let query = parse_state_query(query.as_deref());
    Ok(Json(
        state
            .states
            .list(query.email, query.page.as_deref(), query.size.as_deref())
            .await?,
    ))
}

pub async fn latest(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Json<LatestStateResponse>, Error> {
    let query = parse_state_query(query.as_deref());
    Ok(Json(state.states.latest(query.email).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    axum::extract::Path(state_id): axum::extract::Path<String>,
    AuthedUser(user): AuthedUser,
) -> Result<impl IntoResponse, Error> {
    state.states.delete(user.id, &state_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Default)]
struct StateQuery {
    email: Option<String>,
    page: Option<String>,
    size: Option<String>,
}

fn parse_state_query(raw: Option<&str>) -> StateQuery {
    let mut query = StateQuery::default();
    let Some(raw) = raw else {
        return query;
    };
    for (key, value) in form_urlencoded::parse(raw.as_bytes()) {
        match key.as_ref() {
            "email" if query.email.is_none() => query.email = Some(value.into_owned()),
            "page" if query.page.is_none() => query.page = Some(value.into_owned()),
            "size" if query.size.is_none() => query.size = Some(value.into_owned()),
            _ => {}
        }
    }
    query
}
