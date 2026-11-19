//! Todo-specific browser API. The Rust Todo domain and Records remain authoritative.
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State, rejection::JsonRejection},
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use riesel::{Records, SdkError};
use serde::{Deserialize, Serialize};
use std::{future::Future, io, net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::{Task, TaskData, Todo, TodoError};
use riesel::storage::StoreError;

#[derive(Clone)]
struct ApiState {
    todo: Arc<Todo>,
    host: String,
    origin: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskView {
    pub id: Uuid,
    pub revision: u64,
    pub title: String,
    pub completed: bool,
}

impl From<Task> for TaskView {
    fn from(task: Task) -> Self {
        Self {
            id: task.id,
            revision: task.revision,
            title: task.value.title,
            completed: task.value.completed,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateInput {
    title: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchInput {
    expected_revision: u64,
    title: Option<String>,
    completed: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteInput {
    expected_revision: u64,
}

#[derive(Serialize)]
struct Problem {
    error: &'static str,
}

struct ApiError {
    status: StatusCode,
    code: &'static str,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str) -> Self {
        Self { status, code }
    }

    fn invalid_input() -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request")
    }

    fn invalid_json(rejection: JsonRejection) -> Self {
        Self::new(rejection.status(), "invalid_json")
    }
}

impl From<TodoError> for ApiError {
    fn from(error: TodoError) -> Self {
        match error {
            TodoError::EmptyTitle => Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_title"),
            TodoError::NotFound => Self::new(StatusCode::NOT_FOUND, "not_found"),
            TodoError::Storage(SdkError::Store(StoreError::RevisionConflict)) => {
                Self::new(StatusCode::CONFLICT, "revision_conflict")
            }
            TodoError::Storage(_) => Self::new(StatusCode::SERVICE_UNAVAILABLE, "storage_error"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(Problem { error: self.code })).into_response()
    }
}

/// Enforce a single local origin. CORS alone would not prevent cross-site writes.
/// Host checks also reject DNS-rebinding requests to a loopback listener.
async fn local_origin_guard(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    if headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        != Some(state.host.as_str())
    {
        return ApiError::new(StatusCode::FORBIDDEN, "invalid_host").into_response();
    }
    if headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        == Some("cross-site")
    {
        return ApiError::new(StatusCode::FORBIDDEN, "cross_site_request").into_response();
    }
    if matches!(
        request.method(),
        &Method::POST | &Method::PATCH | &Method::PUT | &Method::DELETE
    ) && headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != Some(state.origin.as_str())
    {
        return ApiError::new(StatusCode::FORBIDDEN, "invalid_origin").into_response();
    }
    next.run(request).await
}

fn parse_id(value: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value).map_err(|_| ApiError::invalid_input())
}

async fn blocking<T: Send + 'static>(
    state: ApiState,
    action: impl FnOnce(&Todo) -> Result<T, TodoError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(move || action(&state.todo))
        .await
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "worker_failure"))?
        .map_err(ApiError::from)
}

async fn list(State(state): State<ApiState>) -> Result<Json<Vec<TaskView>>, ApiError> {
    let tasks = blocking(state, Todo::list_tasks).await?;
    Ok(Json(tasks.into_iter().map(Into::into).collect()))
}

async fn read(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<TaskView>, ApiError> {
    let id = parse_id(&id)?;
    let task = blocking(state, move |todo| todo.get_task(id)).await?;
    Ok(Json(task.ok_or(TodoError::NotFound).map_err(ApiError::from)?.into()))
}

async fn create(
    State(state): State<ApiState>,
    payload: Result<Json<CreateInput>, JsonRejection>,
) -> Result<(StatusCode, Json<TaskView>), ApiError> {
    let Json(input) = payload.map_err(ApiError::invalid_json)?;
    let task = blocking(state, move |todo| todo.create_task(input.title)).await?;
    Ok((StatusCode::CREATED, Json(task.into())))
}

async fn update(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    payload: Result<Json<PatchInput>, JsonRejection>,
) -> Result<Json<TaskView>, ApiError> {
    let id = parse_id(&id)?;
    let Json(input) = payload.map_err(ApiError::invalid_json)?;
    let task = match (input.title, input.completed) {
        (Some(title), None) => {
            blocking(state, move |todo| todo.edit_title(id, input.expected_revision, title)).await?
        }
        (None, Some(completed)) => {
            blocking(state, move |todo| {
                todo.set_completed(id, input.expected_revision, completed)
            })
            .await?
        }
        _ => return Err(ApiError::invalid_input()),
    };
    Ok(Json(task.into()))
}

async fn remove(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    payload: Result<Json<DeleteInput>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let Json(input) = payload.map_err(ApiError::invalid_json)?;
    blocking(state, move |todo| {
        todo.delete_task(id, input.expected_revision)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn routes(records: Records<TaskData>, address: SocketAddr) -> Router {
    let state = ApiState {
        todo: Arc::new(Todo::new(records)),
        host: address.to_string(),
        origin: format!("http://{address}"),
    };
    Router::new()
        .route("/api/tasks", get(list).post(create))
        .route("/api/tasks/{id}", get(read).patch(update).delete(remove))
        .layer(DefaultBodyLimit::max(8 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            local_origin_guard,
        ))
        .with_state(state)
}

/// Serve only on loopback. The shutdown future belongs to the Riesel entrypoint;
/// ending that entrypoint also ends the HTTP listener and Storage session.
pub async fn serve(
    listener: TcpListener,
    records: Records<TaskData>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    let address = listener.local_addr()?;
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Todo HTTP accepts loopback listeners only",
        ));
    }
    axum::serve(listener, routes(records, address))
        .with_graceful_shutdown(shutdown)
        .await
}
