use std::fmt;
use std::future::Future;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::canonical;
use crate::log::{self, Cursor};
use crate::process::ProcessError;
use crate::replication::{self, Origin, SyncCode, SyncReport};
use crate::transport::{self, ServerCode};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const BATCH_EVENTS: usize = 1000;
const BATCH_BYTES: usize = 4 * 1024 * 1024;
const WATCH_PUSH_INTERVAL: Duration = Duration::from_secs(2);
const WATCH_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const WATCH_FIRST_WAIT: Duration = Duration::from_secs(1);
const WATCH_LONGEST_WAIT: Duration = Duration::from_secs(60);

/// A replication server and the bearer token to reach it with (concept Section 4, "Server
/// transport"). Its `Debug` output never shows the token.
#[derive(Clone, PartialEq, Eq)]
pub struct Server {
    url: String,
    token: String,
}

impl Server {
    /// The server at the base URL `url`, for example `https://hone.example`, reached with
    /// `token`.
    #[must_use]
    pub fn new(url: &str, token: &str) -> Self {
        Self {
            url: url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
        }
    }
}

impl fmt::Debug for Server {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Server")
            .field("url", &self.url)
            .field("token", &"…")
            .finish()
    }
}

pub(crate) struct Session<'a> {
    http: reqwest::Client,
    server: &'a Server,
    base: String,
    local: &'a Path,
    project: &'a str,
    own: Option<&'a str>,
}

// The library is blocking and is also called from inside a tokio runtime (the MCP server), where
// blocking on another runtime panics, so the client runs on a thread of its own.
pub(crate) fn run<T: Send>(
    work: impl FnOnce() -> Result<T, ProcessError> + Send,
) -> Result<T, ProcessError> {
    std::thread::scope(|scope| {
        scope
            .spawn(work)
            .join()
            .unwrap_or_else(|_| Err(unavailable("the HTTP client stopped unexpectedly")))
    })
}

pub(crate) fn block_on<T>(
    work: impl Future<Output = Result<T, ProcessError>>,
) -> Result<T, ProcessError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| unavailable(&format!("no runtime for the HTTP client: {error}")))?
        .block_on(work)
}

impl<'a> Session<'a> {
    pub(crate) fn new(
        server: &'a Server,
        local: &'a Path,
        project: &'a str,
        own: Option<&'a str>,
    ) -> Result<Self, ProcessError> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|error| unavailable(&chain(&error)))?;
        Ok(Self {
            http,
            base: format!("{}/v1/projects/{project}", server.url),
            server,
            local,
            project,
            own,
        })
    }

    pub(crate) async fn sync(&self) -> Result<SyncReport, ProcessError> {
        Ok(self.sync_once().await?.0)
    }

    async fn sync_once(&self) -> Result<(SyncReport, Pushed), ProcessError> {
        let mut report = SyncReport::default();
        let server_cursor = self.cursor().await?;
        let pushed = self.push(&server_cursor, &mut report).await?;
        self.pull(&mut report).await?;
        report.finish();
        Ok((report, pushed))
    }

    pub(crate) async fn watch(
        &self,
        until: impl Future<Output = ()>,
        mut on_report: impl FnMut(&SyncReport),
    ) -> Result<(), ProcessError> {
        let mut until = std::pin::pin!(until);
        let (report, mut pushed) = self.sync_once().await?;
        on_report(&report);
        let mut wait = WATCH_FIRST_WAIT;
        loop {
            let url = self.events_url(&self.local_cursor()?);
            let request = self
                .authorized(self.http.get(&url))
                .header("Accept", "text/event-stream")
                .send();
            let response = tokio::select! {
                () = &mut until => return Ok(()),
                response = request => response,
            };
            if let Ok(mut response) = response {
                if !response.status().is_success() {
                    return Err(refused(response).await);
                }
                wait = WATCH_FIRST_WAIT;
                let mut buffer = Vec::new();
                let mut line_number = 0;
                let mut push_timer = tokio::time::interval(WATCH_PUSH_INTERVAL);
                loop {
                    tokio::select! {
                        () = &mut until => return Ok(()),
                        chunk = tokio::time::timeout(WATCH_IDLE_TIMEOUT, response.chunk()) => {
                            let Ok(Ok(Some(chunk))) = chunk else { break };
                            buffer.extend_from_slice(&chunk);
                            let lines = take_data_lines(&mut buffer);
                            if lines.is_empty() {
                                continue;
                            }
                            let mut report = SyncReport::default();
                            let first = line_number + 1;
                            line_number += lines.len();
                            self.import(&lines, &url, first, &mut report)?;
                            report.finish();
                            if !report.received.is_empty() || !report.problems.is_empty() {
                                on_report(&report);
                            }
                        }
                        // A rejected own event blocks the device; pushing again would only repeat
                        // the rejection.
                        _ = push_timer.tick(), if !pushed.rejected => {
                            let mut report = SyncReport::default();
                            let cursor = Cursor::from_iter(
                                self.own.map(|own| (own.to_owned(), pushed.end)),
                            );
                            pushed = self.push(&cursor, &mut report).await?;
                            report.finish();
                            if !report.sent.is_empty() || pushed.rejected {
                                on_report(&report);
                            }
                        }
                    }
                }
            }
            tokio::select! {
                () = &mut until => return Ok(()),
                () = tokio::time::sleep(wait) => {}
            }
            wait = (wait * 2).min(WATCH_LONGEST_WAIT);
        }
    }

    async fn cursor(&self) -> Result<Cursor, ProcessError> {
        let url = format!("{}/cursor", self.base);
        let response = self.call(self.http.get(&url), &url).await?;
        let body = json_body(response).await;
        transport::parse_cursor_json(&body).map_err(|message| invalid_answer(&url, &message))
    }

    async fn push(
        &self,
        server_cursor: &Cursor,
        report: &mut SyncReport,
    ) -> Result<Pushed, ProcessError> {
        let own_end = self
            .own
            .map_or(0, |own| server_cursor.get(own).copied().unwrap_or(0));
        let Some(own) = self.own else {
            return Ok(Pushed {
                end: own_end,
                rejected: false,
            });
        };
        let local_log =
            log::read(self.local, self.project, self.local).map_err(|error| self.io(&error))?;
        let pending: Vec<String> = local_log
            .after(&Cursor::from([(own.to_owned(), own_end)]))
            .iter()
            .filter(|event| event["device"] == own)
            .map(canonical::to_canonical)
            .collect();
        let url = format!("{}/events", self.base);
        let mut stored = own_end;
        for batch in batches(&pending) {
            let request = self
                .http
                .post(&url)
                .header("Content-Type", "application/x-ndjson")
                .body(
                    batch
                        .iter()
                        .map(|line| format!("{line}\n"))
                        .collect::<String>(),
                );
            let response = self
                .authorized(request)
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await
                .map_err(|error| unavailable(&format!("{url}: {}", chain(&error))))?;
            let status = response.status();
            let body: Value = json_body(response).await;
            let code = body["code"].as_str().and_then(ServerCode::parse);
            if let Some(cursor) = body
                .get("cursor")
                .and_then(|cursor| transport::parse_cursor_json(cursor).ok())
            {
                let end = cursor.get(own).copied().unwrap_or(0);
                if end > stored {
                    *report.sent.entry(own.to_owned()).or_default() += end - stored;
                    stored = end;
                }
            }
            if status.is_success() {
                continue;
            }
            match code.and_then(sync_code) {
                Some(sync_code) => {
                    let id = body["event"].as_str().unwrap_or_default();
                    let (path, line) = local_log.locations.get(id).map_or_else(
                        || (url.clone(), None),
                        |(path, number)| {
                            (self.local.join(path).display().to_string(), Some(*number))
                        },
                    );
                    let message = body["message"].as_str().unwrap_or_default().to_owned();
                    report.problem(sync_code, path, line, message);
                    return Ok(Pushed {
                        end: stored,
                        rejected: true,
                    });
                }
                None => return Err(refused_with(status.as_u16(), &body)),
            }
        }
        Ok(Pushed {
            end: stored,
            rejected: false,
        })
    }

    async fn pull(&self, report: &mut SyncReport) -> Result<(), ProcessError> {
        let url = self.events_url(&self.local_cursor()?);
        let mut response = self
            .call(
                self.http.get(&url).header("Accept", "application/x-ndjson"),
                &url,
            )
            .await?;
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| unavailable(&format!("{url}: {}", chain(&error))))?
        {
            body.extend_from_slice(&chunk);
        }
        let lines: Vec<&[u8]> = body
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .collect();
        self.import(&lines, &url, 1, report)
    }

    fn import<L: AsRef<[u8]>>(
        &self,
        lines: &[L],
        url: &str,
        first_line: usize,
        report: &mut SyncReport,
    ) -> Result<(), ProcessError> {
        let raw: Vec<&[u8]> = lines.iter().map(AsRef::as_ref).collect();
        let mut remote_log = log::from_lines(&raw, self.project);
        for (_, number) in remote_log.locations.values_mut() {
            *number += first_line - 1;
        }
        for invalid in &mut remote_log.invalid {
            invalid.line = invalid.line.map(|number| number + first_line - 1);
        }
        for invalid in &remote_log.invalid {
            report.problem(
                SyncCode::InvalidEvent,
                url,
                invalid.line,
                invalid.message.clone(),
            );
        }
        let _lock = log::lock(self.local).map_err(|error| self.io(&error))?;
        let local_log =
            log::read(self.local, self.project, self.local).map_err(|error| self.io(&error))?;
        replication::pull(
            self.local,
            &local_log,
            &Origin::Server(url),
            &remote_log,
            report,
        )
        .map_err(|error| self.io(&error))
    }

    fn local_cursor(&self) -> Result<Cursor, ProcessError> {
        Ok(log::read(self.local, self.project, self.local)
            .map_err(|error| self.io(&error))?
            .cursor())
    }

    fn events_url(&self, cursor: &Cursor) -> String {
        format!(
            "{}/events?cursor={}",
            self.base,
            transport::cursor_query(cursor)
        )
    }

    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.bearer_auth(&self.server.token)
    }

    async fn call(
        &self,
        request: reqwest::RequestBuilder,
        url: &str,
    ) -> Result<reqwest::Response, ProcessError> {
        let response = self
            .authorized(request)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| unavailable(&format!("{url}: {}", chain(&error))))?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(refused(response).await)
        }
    }

    fn io(&self, error: &std::io::Error) -> ProcessError {
        ProcessError::IoError {
            message: format!("{}: {error}", self.local.display()),
        }
    }
}

// The server's cursor of the own device after a push, and whether the server rejected an event.
struct Pushed {
    end: u64,
    rejected: bool,
}

fn batches(lines: &[String]) -> Vec<&[String]> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut bytes = 0;
    for (index, line) in lines.iter().enumerate() {
        let full = index - start >= BATCH_EVENTS
            || (index > start && bytes + line.len() + 1 > BATCH_BYTES);
        if full {
            batches.push(&lines[start..index]);
            start = index;
            bytes = 0;
        }
        bytes += line.len() + 1;
    }
    if start < lines.len() {
        batches.push(&lines[start..]);
    }
    batches
}

// One event per `data:` line; comments, empty lines, and an unfinished last line stay out.
fn take_data_lines(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let Some(end) = buffer.iter().rposition(|byte| *byte == b'\n') else {
        return Vec::new();
    };
    let complete: Vec<u8> = buffer.drain(..=end).collect();
    complete
        .split(|byte| *byte == b'\n')
        .filter_map(|line| line.strip_prefix(b"data:"))
        .map(|data| data.strip_prefix(b" ").unwrap_or(data).to_vec())
        .collect()
}

fn sync_code(code: ServerCode) -> Option<SyncCode> {
    match code {
        ServerCode::ScopeMismatch => Some(SyncCode::ScopeMismatch),
        ServerCode::IdentityMismatch => Some(SyncCode::IdentityMismatch),
        ServerCode::RoleMismatch => Some(SyncCode::RoleMismatch),
        ServerCode::InvalidEvent => Some(SyncCode::InvalidEvent),
        ServerCode::SeqGap => Some(SyncCode::SeqGap),
        ServerCode::Fork => Some(SyncCode::Fork),
        ServerCode::IdConflict => Some(SyncCode::IdConflict),
        _ => None,
    }
}

async fn refused(response: reqwest::Response) -> ProcessError {
    let status = response.status().as_u16();
    let body: Value = json_body(response).await;
    refused_with(status, &body)
}

async fn json_body(response: reqwest::Response) -> Value {
    response
        .bytes()
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null)
}

fn refused_with(status: u16, body: &Value) -> ProcessError {
    let code = body["code"]
        .as_str()
        .filter(|code| ServerCode::parse(code).is_some())
        .map_or_else(|| format!("http-{status}"), str::to_owned);
    let message = body["message"].as_str().map_or_else(
        || format!("the server answered with status {status}"),
        str::to_owned,
    );
    ProcessError::Refused { code, message }
}

fn invalid_answer(url: &str, message: &str) -> ProcessError {
    ProcessError::Refused {
        code: "invalid-answer".into(),
        message: format!("{url}: {message}"),
    }
}

fn unavailable(message: &str) -> ProcessError {
    ProcessError::Unavailable {
        message: message.to_owned(),
    }
}

// reqwest's own message hides the cause, for example a refused connection.
fn chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}
