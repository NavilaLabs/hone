use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use hone_core::{AppendError, Appender, Cursor, ServerCode};
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;

use crate::config::{self, Config, Project};

struct Shared {
    config: Config,
    // Bumped after every append, so that subscribers read again.
    changes: BTreeMap<String, watch::Sender<u64>>,
    stopped: watch::Receiver<bool>,
}

type Answer = Result<Response, Refusal>;

// Boxed, because a response is large and most calls succeed.
struct Refusal(Box<Response>);

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        *self.0
    }
}

pub(crate) async fn serve(
    config: Config,
    listener: tokio::net::TcpListener,
    stopped: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let changes = config
        .projects
        .keys()
        .map(|project| (project.clone(), watch::channel(0).0))
        .collect();
    let shared = Arc::new(Shared {
        config,
        changes,
        stopped: stopped.clone(),
    });
    let app = Router::new()
        .route(
            "/v1/projects/{project}/cursor",
            get(cursor).fallback(method_not_allowed),
        )
        .route(
            "/v1/projects/{project}/events",
            get(read).post(append).fallback(method_not_allowed),
        )
        .fallback(not_found)
        .with_state(shared);
    let mut stopped = stopped;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = stopped.wait_for(|stopped| *stopped).await;
        })
        .await
}

async fn cursor(
    State(shared): State<Arc<Shared>>,
    Path(project): Path<String>,
    headers: HeaderMap,
) -> Answer {
    let identity = authenticate(&shared, &headers)?;
    project_form(&project)?;
    let project = access(&shared, &identity, &project)?;
    let store = project.store.clone();
    let cursor = blocking(move || store.cursor()).await?;
    Ok(json_response(StatusCode::OK, &json!(cursor)))
}

async fn read(
    State(shared): State<Arc<Shared>>,
    Path(project_id): Path<String>,
    query: Result<Query<BTreeMap<String, String>>, axum::extract::rejection::QueryRejection>,
    headers: HeaderMap,
) -> Answer {
    let identity = authenticate(&shared, &headers)?;
    project_form(&project_id)?;
    let Ok(Query(query)) = query else {
        return Err(error(
            ServerCode::InvalidRequest,
            "the query cannot be read",
        ));
    };
    let cursor = hone_core::parse_cursor_query(query.get("cursor").map_or("", String::as_str))
        .map_err(|message| error(ServerCode::InvalidRequest, &message))?;
    let project = access(&shared, &identity, &project_id)?;
    if accepts_event_stream(&headers) {
        return Ok(subscribe(&shared, &project_id, project, cursor).into_response());
    }
    let store = project.store.clone();
    let lines = blocking(move || store.read(&cursor)).await?;
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        body,
    )
        .into_response())
}

async fn append(
    State(shared): State<Arc<Shared>>,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Answer {
    let identity = authenticate(&shared, &headers)?;
    project_form(&project_id)?;
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !media_type.is_some_and(|media_type| media_type.eq_ignore_ascii_case("application/x-ndjson"))
    {
        return Err(error(
            ServerCode::InvalidRequest,
            "the body must be `application/x-ndjson`",
        ));
    }
    let limit = shared.config.request_bytes;
    let too_large = || {
        error(
            ServerCode::TooLarge,
            &format!("the body exceeds {limit} bytes"),
        )
    };
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if declared.is_some_and(|length| length > limit as u64) {
        return Err(too_large());
    }
    let bytes = axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| too_large())?;
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| error(ServerCode::InvalidRequest, "the body is not UTF-8"))?;
    let project = access(&shared, &identity, &project_id)?;
    let store = project.store.clone();
    let actors = project.actors.clone();
    let result = tokio::task::spawn_blocking(move || {
        store.append(
            Appender {
                identity: &identity,
                actors: &actors,
            },
            &text,
        )
    })
    .await
    .map_err(|_| internal("the append stopped unexpectedly"))?;
    if let Some(changes) = shared.changes.get(&project_id) {
        changes.send_modify(|version| *version += 1);
    }
    match result {
        Ok(cursor) => Ok(json_response(StatusCode::OK, &json!({ "cursor": cursor }))),
        Err(AppendError::Rejected(rejection)) => Err(refusal(
            status(rejection.code),
            &serde_json::to_value(&rejection).unwrap_or(Value::Null),
        )),
        Err(AppendError::Store(failure)) => {
            eprintln!("hone serve: {failure}");
            Err(internal("the store cannot be written"))
        }
    }
}

async fn method_not_allowed(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Answer {
    authenticate(&shared, &headers)?;
    Err(error(
        ServerCode::InvalidRequest,
        "the method is not allowed here",
    ))
}

async fn not_found(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Answer {
    authenticate(&shared, &headers)?;
    Err(error(ServerCode::NotFound, "the path names no request"))
}

fn subscribe(
    shared: &Arc<Shared>,
    project_id: &str,
    project: &Project,
    mut cursor: Cursor,
) -> impl IntoResponse {
    let heartbeat = shared.config.heartbeat;
    let (sender, receiver) = mpsc::channel::<Result<Event, Infallible>>(64);
    let store = project.store.clone();
    let mut changes = shared.changes.get(project_id).map(watch::Sender::subscribe);
    let mut stopped = shared.stopped.clone();
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(heartbeat);
        timer.tick().await;
        loop {
            if *stopped.borrow() {
                return;
            }
            let reader = store.clone();
            let since = cursor.clone();
            let Ok(Ok(lines)) = tokio::task::spawn_blocking(move || reader.read(&since)).await
            else {
                return;
            };
            for line in lines {
                if let Ok(event) = serde_json::from_str::<Value>(&line)
                    && let (Some(device), Some(seq)) =
                        (event["device"].as_str(), event["seq"].as_u64())
                {
                    cursor.insert(device.to_owned(), seq);
                }
                if sender.send(Ok(Event::default().data(line))).await.is_err() {
                    return;
                }
            }
            // The heartbeat also reads again, so that events another process wrote into the
            // data directory, for example a restore from a shared folder, arrive too.
            tokio::select! {
                _ = changed(changes.as_mut()) => {}
                _ = timer.tick() => {}
                _ = stopped.changed() => return,
                () = sender.closed() => return,
            }
        }
    });
    Sse::new(ReceiverStream::new(receiver)).keep_alive(KeepAlive::new().interval(heartbeat))
}

async fn changed(changes: Option<&mut watch::Receiver<u64>>) {
    if let Some(changes) = changes
        && changes.changed().await.is_ok()
    {
        return;
    }
    std::future::pending().await
}

fn authenticate(shared: &Shared, headers: &HeaderMap) -> Result<String, Refusal> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim());
    token
        .and_then(|token| shared.config.identities.get(&config::token_hash(token)))
        .cloned()
        .ok_or_else(|| {
            error(
                ServerCode::Unauthenticated,
                "the bearer token is missing or invalid",
            )
        })
}

fn project_form(project: &str) -> Result<(), Refusal> {
    if hone_core::is_project_id(project) {
        Ok(())
    } else {
        Err(error(
            ServerCode::InvalidRequest,
            &format!("`{project}` is not a project ID"),
        ))
    }
}

fn access<'a>(shared: &'a Shared, identity: &str, project: &str) -> Result<&'a Project, Refusal> {
    let served = shared.config.projects.get(project).ok_or_else(|| {
        error(
            ServerCode::NotFound,
            &format!("the server does not serve `{project}`"),
        )
    })?;
    if served.members.contains(identity) {
        Ok(served)
    } else {
        Err(error(
            ServerCode::Forbidden,
            &format!("`{identity}` has no access to `{project}`"),
        ))
    }
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|media_type| {
            media_type.split(';').next().is_some_and(|media_type| {
                media_type.trim().eq_ignore_ascii_case("text/event-stream")
            })
        })
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, hone_core::ProcessError> + Send + 'static,
) -> Result<T, Refusal> {
    match tokio::task::spawn_blocking(work).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(failure)) => {
            eprintln!("hone serve: {failure}");
            Err(internal("the store cannot be read"))
        }
        Err(_) => Err(internal("the request stopped unexpectedly")),
    }
}

fn error(code: ServerCode, message: &str) -> Refusal {
    refusal(
        status(code),
        &json!({ "code": code.as_str(), "message": message }),
    )
}

// A store that cannot be read is not the client's fault and has no code of the transport; the
// message stays generic, so that no path of the server leaks.
fn internal(message: &str) -> Refusal {
    refusal(
        StatusCode::INTERNAL_SERVER_ERROR,
        &json!({ "code": "internal-error", "message": message }),
    )
}

fn status(code: ServerCode) -> StatusCode {
    StatusCode::from_u16(code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn refusal(status: StatusCode, body: &Value) -> Refusal {
    Refusal(Box::new(json_response(status, body)))
}

fn json_response(status: StatusCode, body: &Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}
