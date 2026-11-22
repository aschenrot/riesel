//! Versioned task API. This layer maps HTTP to the existing Todo domain.
use super::HttpState;
use crate::{Task, TodoError};
use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use riesel::{SdkError, storage::StoreError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug)]
pub(super) enum ApiError {
    BadInput,
    Forbidden,
    NotFound,
    Conflict,
    TooLarge,
    UnsupportedMedia,
    MethodNotAllowed,
    Unavailable,
    OutcomeUnknown,
    Internal,
}

#[derive(Serialize)]
struct ErrorMessage {
    code: &'static str,
    message: &'static str,
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorMessage,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::BadInput => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid request",
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "Request origin is not allowed",
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found", "Task not found"),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "revision_conflict",
                "Task revision is stale",
            ),
            Self::TooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "too_large",
                "Request is too large",
            ),
            Self::UnsupportedMedia => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media",
                "JSON is required",
            ),
            Self::MethodNotAllowed => (
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method not allowed",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                "Service unavailable",
            ),
            // Do not collapse an accepted but unconfirmed mutation into
            // "not executed". The HTTP client may reconcile by reading.
            Self::OutcomeUnknown => (
                StatusCode::SERVICE_UNAVAILABLE,
                "outcome_unknown",
                "Request outcome could not be confirmed",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Storage operation failed",
            ),
        };
        (
            status,
            Json(ErrorBody {
                error: ErrorMessage { code, message },
            }),
        )
            .into_response()
    }
}

impl From<TodoError> for ApiError {
    fn from(error: TodoError) -> Self {
        match error {
            TodoError::EmptyTitle => Self::BadInput,
            TodoError::NotFound => Self::NotFound,
            TodoError::Storage(SdkError::Store(StoreError::NotFound)) => Self::NotFound,
            TodoError::Storage(SdkError::Store(StoreError::RevisionConflict)) => Self::Conflict,
            _ => Self::Internal,
        }
    }
}

fn json<T>(result: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    match result {
        Ok(Json(value)) => Ok(value),
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            Err(ApiError::TooLarge)
        }
        Err(rejection) if rejection.status() == StatusCode::UNSUPPORTED_MEDIA_TYPE => {
            Err(ApiError::UnsupportedMedia)
        }
        Err(_) => Err(ApiError::BadInput),
    }
}

#[derive(Serialize)]
pub(super) struct TaskView {
    id: Uuid,
    revision: String,
    title: String,
    completed: bool,
}

impl From<Task> for TaskView {
    fn from(task: Task) -> Self {
        Self {
            id: task.id,
            revision: task.revision.to_string(),
            title: task.value.title,
            completed: task.value.completed,
        }
    }
}

#[derive(Serialize)]
pub(super) struct TaskPage {
    records: Vec<TaskView>,
    next_cursor: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListQuery {
    after: Option<String>,
    limit: Option<u64>,
}

pub(super) async fn list(
    State(state): State<HttpState>,
    input: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<TaskPage>, ApiError> {
    let Query(query) = input.map_err(|_| ApiError::BadInput)?;
    let after = query
        .after
        .map(|id| Uuid::parse_str(&id).map_err(|_| ApiError::BadInput))
        .transpose()?;
    let limit = query.limit.unwrap_or(32);
    if !(1..=32).contains(&limit) {
        return Err(ApiError::BadInput);
    }
    let page = state
        .invoke(move |todo| todo.list_tasks_page(after, limit))
        .await?;
    Ok(Json(TaskPage {
        records: page.records.into_iter().map(TaskView::from).collect(),
        next_cursor: page.next_cursor,
    }))
}

fn parse_id(text: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(text).map_err(|_| ApiError::BadInput)
}

/// Decimal strings preserve the full u64 revision domain in JavaScript.
fn parse_revision(text: &str) -> Result<u64, ApiError> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ApiError::BadInput);
    }
    text.parse::<u64>().map_err(|_| ApiError::BadInput)
}

pub(super) async fn show(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Json<TaskView>, ApiError> {
    let id = parse_id(&id)?;
    let task = state
        .invoke(move |todo| todo.get_task(id).and_then(|x| x.ok_or(TodoError::NotFound)))
        .await?;
    Ok(Json(task.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateTask {
    title: String,
}

pub(super) async fn create(
    State(state): State<HttpState>,
    input: Result<Json<CreateTask>, JsonRejection>,
) -> Result<(StatusCode, Json<TaskView>), ApiError> {
    let input = json(input)?;
    let task = state
        .invoke(move |todo| todo.create_task(input.title))
        .await?;
    Ok((StatusCode::CREATED, Json(task.into())))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PatchTask {
    revision: String,
    title: Option<String>,
    completed: Option<bool>,
}

pub(super) async fn patch(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    input: Result<Json<PatchTask>, JsonRejection>,
) -> Result<Json<TaskView>, ApiError> {
    let id = parse_id(&id)?;
    let input = json(input)?;
    let revision = parse_revision(&input.revision)?;
    let task = match (input.title, input.completed) {
        (Some(title), None) => {
            state
                .invoke(move |todo| todo.edit_title(id, revision, title))
                .await?
        }
        (None, Some(completed)) => {
            state
                .invoke(move |todo| todo.set_completed(id, revision, completed))
                .await?
        }
        _ => return Err(ApiError::BadInput),
    };
    Ok(Json(task.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeleteTask {
    revision: String,
}

pub(super) async fn delete(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    input: Result<Json<DeleteTask>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let revision = parse_revision(&json(input)?.revision)?;
    state
        .invoke(move |todo| todo.delete_task(id, revision))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
