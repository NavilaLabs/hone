//! The reference server over HTTP (concept Section 4, "Server transport"): the read and append
//! test cases through the transport, every error code of a whole call, the subscription,
//! shutdown, and two process stores that converge through the server.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hone_core::{Author, Change, Peer, ProcessError, ProcessStore, Server, StepStatus, SyncCode};
use hone_server::{Config, Handle};
use serde_json::{Value, json};

const PROJECT: &str = "prj-8m2k4d7q";
const TICKET: &str = "tk-9f2x4a8m";
// `printf %s token-<name> | sha256sum`
const ALICE: &str = "c26a7f01074b72beff2295b5cb02eb0b0fa871f4aca30367c51ffcd0c68d4832";
const BOB: &str = "1ccf8933062b5a156c5f57ad39314916ec1cbf46db164a70721323b8523c7068";
const CAROL: &str = "aafedddf5ce7c92b4d5172ecc41ddcff2d4a3bfe1a8a7970fa55b69870663c4c";

fn schemas() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

// A temporary directory with a server configuration, removed on drop.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("hone-server-{}-{name}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("removable");
        }
        std::fs::create_dir_all(&root).expect("creatable");
        Self { root }
    }

    fn store_directory(&self) -> PathBuf {
        self.root.join("data/local").join(PROJECT)
    }

    // A server where alice and bob are members of the project and carol is not.
    fn start(&self, actors: &Value, extra: &str) -> Running {
        self.start_with_members(&["alice", "bob"], actors, extra)
    }

    fn start_with_members(&self, members: &[&str], actors: &Value, extra: &str) -> Running {
        let config = format!(
            "listen: 127.0.0.1:0\n\
             data: data\n\
             {extra}\
             identities:\n  \
               alice: {{ token: \"sha256:{ALICE}\" }}\n  \
               bob: {{ token: \"sha256:{BOB}\" }}\n  \
               carol: {{ token: \"sha256:{CAROL}\" }}\n\
             projects:\n  \
               {PROJECT}:\n    \
                 workspace: local\n    \
                 members: {members}\n    \
                 actors: {actors}\n",
            members = json!(members),
        );
        let path = self.root.join("server.yaml");
        std::fs::write(&path, config).expect("writable");
        let config = Config::load(&path).expect("a valid configuration");
        let handle = hone_server::start(config).expect("starts");
        Running {
            base: format!("http://{}", handle.address()),
            handle: Some(handle),
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("a client"),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Running {
    base: String,
    handle: Option<Handle>,
    http: reqwest::blocking::Client,
}

impl Running {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn events(&self) -> String {
        self.url(&format!("/v1/projects/{PROJECT}/events"))
    }

    fn post(&self, token: &str, body: impl Into<reqwest::blocking::Body>) -> (u16, Value) {
        answer(
            self.http
                .post(self.events())
                .bearer_auth(token)
                .header("Content-Type", "application/x-ndjson")
                .body(body)
                .send()
                .expect("answered"),
        )
    }

    fn get(&self, path: &str, token: Option<&str>) -> (u16, Value) {
        let request = self.http.get(self.url(path));
        let request = match token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        answer(request.send().expect("answered"))
    }

    fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.stop().expect("stops");
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

fn answer(response: reqwest::blocking::Response) -> (u16, Value) {
    let status = response.status().as_u16();
    let text = response.text().expect("a body");
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

fn copy_directory(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("creatable");
    for entry in std::fs::read_dir(from).expect("readable") {
        let entry = entry.expect("readable entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copyable");
        }
    }
}

fn cases(kind: &str) -> Vec<(String, PathBuf)> {
    let mut cases: Vec<(String, PathBuf)> = std::fs::read_dir(schemas().join("tests").join(kind))
        .expect("readable")
        .map(|entry| entry.expect("readable entry"))
        .map(|entry| {
            (
                entry.file_name().into_string().expect("UTF-8"),
                entry.path(),
            )
        })
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no {kind} cases");
    cases
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("readable")).expect("JSON")
}

fn token_of(identity: &str) -> String {
    format!("token-{identity}")
}

#[test]
fn read_cases_return_the_events_after_the_cursor() {
    for (name, case) in cases("read") {
        let scratch = Scratch::new(&format!("read-{name}"));
        copy_directory(&case.join("store"), &scratch.store_directory());
        let server = scratch.start(&json!({}), "");
        let cursor = read_json(&case.join("cursor.json"));
        let query: Vec<String> = cursor
            .as_object()
            .expect("an object")
            .iter()
            .map(|(device, seq)| format!("{device}:{seq}"))
            .collect();
        let response = server
            .http
            .get(format!("{}?cursor={}", server.events(), query.join(",")))
            .bearer_auth("token-alice")
            .send()
            .expect("answered");
        assert_eq!(response.status().as_u16(), 200, "case {name}");
        assert_eq!(
            response.headers()["content-type"],
            "application/x-ndjson",
            "case {name}"
        );
        let expected = std::fs::read_to_string(case.join("expected.ndjson")).expect("readable");
        assert_eq!(response.text().expect("a body"), expected, "case {name}");
    }
}

#[test]
fn append_cases_answer_as_expected_over_http() {
    for (name, case) in cases("append") {
        let scratch = Scratch::new(&format!("append-{name}"));
        if case.join("store").is_dir() {
            copy_directory(&case.join("store"), &scratch.store_directory());
        }
        let request = read_json(&case.join("request.json"));
        let identity = request["identity"].as_str().expect("an identity");
        let server = scratch.start_with_members(&[identity], &request["actors"], "");
        let body = std::fs::read(case.join("events.ndjson")).expect("readable");

        let (status, mut answer) = server.post(&token_of(identity), body);

        let expected = read_json(&case.join("expected.json"));
        assert_eq!(json!(status), expected["status"], "case {name}");
        assert!(
            status == 200 || answer["message"].is_string(),
            "case {name}: an error carries a message"
        );
        let object = answer.as_object_mut().expect("an object");
        object.remove("message");
        object.insert("status".into(), json!(status));
        assert_eq!(answer, expected, "case {name}");
        let (_, cursor) = server.get(
            &format!("/v1/projects/{PROJECT}/cursor"),
            Some(&token_of(identity)),
        );
        assert_eq!(
            cursor, expected["cursor"],
            "case {name}: the cursor afterwards"
        );
    }
}

#[test]
fn whole_calls_are_refused_with_their_codes() {
    let scratch = Scratch::new("codes");
    let server = scratch.start(&json!({}), "limits: { request_bytes: 1024 }\n");
    let cursor = format!("/v1/projects/{PROJECT}/cursor");
    let events = format!("/v1/projects/{PROJECT}/events");
    let check = |(status, body): (u16, Value), expected_status: u16, code: &str, case: &str| {
        assert_eq!(status, expected_status, "{case}: {body}");
        assert_eq!(body["code"], code, "{case}");
        assert!(body["message"].is_string(), "{case}: a message");
    };

    check(
        server.get(&cursor, None),
        401,
        "unauthenticated",
        "no token",
    );
    check(
        server.get(&cursor, Some("token-mallory")),
        401,
        "unauthenticated",
        "wrong token",
    );
    let basic = server
        .http
        .get(server.url(&cursor))
        .header("Authorization", "Basic dG9rZW4tYWxpY2U=")
        .send()
        .expect("answered");
    check(answer(basic), 401, "unauthenticated", "another scheme");
    check(
        server.get("/nowhere", None),
        401,
        "unauthenticated",
        "the token comes before the path",
    );
    check(
        server.get("/v1/projects/prj-zzzzzzzz/cursor", None),
        401,
        "unauthenticated",
        "the token comes before the project",
    );

    check(
        server.get("/v1/projects/payments/cursor", Some("token-alice")),
        400,
        "invalid-request",
        "no project ID",
    );
    for query in [
        "dev-3k9a5h2t:007",
        "dev-3k9a5h2t:5,dev-3k9a5h2t:9",
        "bob:5",
        "dev-3k9a5h2t",
        "dev-3k9a5h2t:-1",
    ] {
        check(
            server.get(&format!("{events}?cursor={query}"), Some("token-alice")),
            400,
            "invalid-request",
            query,
        );
    }
    check(
        server.get(
            "/v1/projects/prj-zzzzzzzz/events?cursor=bob:5",
            Some("token-alice"),
        ),
        400,
        "invalid-request",
        "the request comes before the project",
    );
    let plain = server
        .http
        .post(server.events())
        .bearer_auth("token-alice")
        .header("Content-Type", "text/plain")
        .body("")
        .send()
        .expect("answered");
    check(answer(plain), 400, "invalid-request", "media type");
    check(
        server.post("token-alice", vec![0xff, 0xfe, b'\n']),
        400,
        "invalid-request",
        "not UTF-8",
    );
    let delete = server
        .http
        .delete(server.events())
        .bearer_auth("token-alice")
        .send()
        .expect("answered");
    check(answer(delete), 400, "invalid-request", "method");

    check(
        server.get("/v1/projects/prj-zzzzzzzz/cursor", Some("token-alice")),
        404,
        "not-found",
        "unknown project",
    );
    check(
        server.get("/nowhere", Some("token-alice")),
        404,
        "not-found",
        "unknown path",
    );
    check(
        server.get(&cursor, Some("token-carol")),
        403,
        "forbidden",
        "no member",
    );
    check(
        server.post("token-alice", vec![b'x'; 2048]),
        413,
        "too-large",
        "declared length",
    );
    let chunked = reqwest::blocking::Body::new(std::io::Cursor::new(vec![b'x'; 2048]));
    check(
        server.post("token-alice", chunked),
        413,
        "too-large",
        "streamed body",
    );

    let (status, body) = server.get(&cursor, Some("token-alice"));
    assert_eq!(
        (status, body),
        (200, json!({})),
        "an empty store has the empty cursor"
    );
    assert!(
        !scratch.store_directory().exists(),
        "refused calls create no store"
    );
}

fn subscribe(server: &Running, cursor: &str) -> BufReader<reqwest::blocking::Response> {
    let response = server
        .http
        .get(format!("{}?cursor={cursor}", server.events()))
        .bearer_auth("token-alice")
        .header("Accept", "text/event-stream")
        .send()
        .expect("answered");
    assert_eq!(response.status().as_u16(), 200);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .expect("text")
            .starts_with("text/event-stream")
    );
    BufReader::new(response)
}

// The next `data:` line, skipping heartbeats; `None` once only a heartbeat came.
fn next_data(stream: &mut BufReader<reqwest::blocking::Response>) -> (Option<String>, bool) {
    let mut heartbeat = false;
    loop {
        let mut line = String::new();
        if stream.read_line(&mut line).expect("readable") == 0 {
            return (None, heartbeat);
        }
        let line = line.trim_end_matches('\n');
        if let Some(data) = line.strip_prefix("data: ") {
            return (Some(data.to_owned()), heartbeat);
        }
        if line.starts_with(':') {
            heartbeat = true;
            return (None, heartbeat);
        }
    }
}

#[test]
fn a_subscription_continues_without_gap_or_repetition() {
    let scratch = Scratch::new("subscribe");
    let case = schemas().join("tests/append/stores-in-order");
    copy_directory(&case.join("store"), &scratch.store_directory());
    let mut server = scratch.start(&json!({}), "limits: { heartbeat_seconds: 1 }\n");
    let held =
        std::fs::read_to_string(case.join("store/log/dev-3k9a5h2t/1.ndjson")).expect("readable");
    let body = std::fs::read_to_string(case.join("events.ndjson")).expect("readable");

    let mut stream = subscribe(&server, "dev-3k9a5h2t:1");
    assert_eq!(
        next_data(&mut stream).0.as_deref(),
        held.lines().nth(1),
        "first what `read` returns"
    );
    let (status, _) = server.post("token-alice", body.clone());
    assert_eq!(status, 200);
    let mut received = Vec::new();
    let mut heartbeat = false;
    let started = Instant::now();
    while received.len() < 3 && started.elapsed() < Duration::from_secs(8) {
        let (data, beat) = next_data(&mut stream);
        heartbeat |= beat;
        received.extend(data);
    }
    let mut expected: Vec<&str> = body.lines().collect();
    expected.sort_unstable();
    received.sort_unstable();
    assert_eq!(received, expected, "every new event once");
    // Repeating the same append stores nothing, so nothing new may arrive.
    assert_eq!(server.post("token-alice", body).0, 200);
    let until = Instant::now() + Duration::from_millis(2500);
    while Instant::now() < until {
        let (data, beat) = next_data(&mut stream);
        heartbeat |= beat;
        assert_eq!(data, None, "a duplicate append sends nothing");
    }
    assert!(heartbeat, "a heartbeat comment arrives");

    let stopping = Instant::now();
    server.stop();
    assert!(
        stopping.elapsed() < Duration::from_secs(5),
        "an open subscription does not hold the shutdown"
    );
}

#[test]
fn a_subscription_is_refused_like_a_read() {
    let scratch = Scratch::new("subscribe-refused");
    let server = scratch.start(&json!({}), "");
    let response = server
        .http
        .get(format!("{}?cursor=bob:1", server.events()))
        .bearer_auth("token-alice")
        .header("Accept", "text/event-stream")
        .send()
        .expect("answered");
    let (status, body) = answer(response);
    assert_eq!(
        (status, body["code"].clone()),
        (400, json!("invalid-request"))
    );
}

// A device with its own data directory and a copy of one gate test project.
struct Device {
    root: PathBuf,
}

impl Device {
    fn new(scratch: &Scratch, name: &str) -> Self {
        let root = scratch.root.join(name);
        copy_directory(
            &schemas().join("tests/check/steps-complete-incomplete/project"),
            &root.join("project"),
        );
        Self { root }
    }

    fn store(&self) -> ProcessStore {
        ProcessStore::locate(&self.root.join("project"), Some(&self.root.join("data")))
            .expect("locatable")
    }

    fn set_step(&self, author: &str, role: &str, status: StepStatus) -> Value {
        let author = Author {
            name: author.into(),
            role: role.into(),
        };
        let change = Change::SetStep {
            ticket: TICKET.into(),
            step: "decision".into(),
            status,
            reason: None,
        };
        self.store()
            .append(&author, change)
            .expect("appended")
            .event
    }

    fn sync(
        &self,
        server: &Running,
        identity: &str,
    ) -> Result<hone_core::SyncReport, ProcessError> {
        self.store().sync(Peer::Server(&Server::new(
            &server.base,
            &token_of(identity),
        )))
    }
}

#[test]
fn two_devices_converge_through_the_server() {
    let scratch = Scratch::new("converge");
    let server = scratch.start(&json!({}), "");
    let alice = Device::new(&scratch, "alice");
    let bob = Device::new(&scratch, "bob");
    alice.set_step("alice", "human", StepStatus::Done);
    bob.set_step("bob", "human", StepStatus::Skipped);
    alice.set_step("alice", "human", StepStatus::Open);

    let first = alice.sync(&server, "alice").expect("syncs");
    assert!(first.problems.is_empty(), "{first:?}");
    assert_eq!(
        serde_json::to_value(&first.sent)
            .expect("JSON")
            .as_object()
            .map(|sent| sent.values().cloned().collect::<Vec<_>>()),
        Some(vec![json!(2)])
    );
    let second = bob.sync(&server, "bob").expect("syncs");
    assert!(second.problems.is_empty(), "{second:?}");
    assert_eq!(second.received.values().sum::<u64>(), 2);
    let answer = bob.set_step("bob", "human", StepStatus::Done);
    assert!(bob.sync(&server, "bob").expect("syncs").problems.is_empty());
    let last = alice.sync(&server, "alice").expect("syncs");
    assert!(last.problems.is_empty() && last.received.values().sum::<u64>() == 2);

    let order = |device: &Device| -> Vec<Value> {
        device
            .store()
            .events(TICKET)
            .expect("readable")
            .events
            .iter()
            .map(|event| event["id"].clone())
            .collect()
    };
    assert_eq!(order(&alice).len(), 4);
    assert_eq!(order(&alice), order(&bob));
    assert_eq!(order(&alice).last(), Some(&answer["id"]));
    assert_eq!(
        alice.store().cursor().expect("cursor"),
        bob.store().cursor().expect("cursor")
    );
    let again = alice.sync(&server, "alice").expect("syncs");
    assert!(
        again.sent.is_empty() && again.received.is_empty(),
        "{again:?}"
    );
}

#[test]
fn a_rejected_own_event_is_named_by_its_local_place() {
    let scratch = Scratch::new("rejected");
    let server = scratch.start(&json!({}), "");
    let alice = Device::new(&scratch, "alice");
    alice.set_step("alice", "human", StepStatus::Done);
    alice.set_step("mallory", "human", StepStatus::Open);
    alice.set_step("alice", "human", StepStatus::Done);

    let report = alice.sync(&server, "alice").expect("syncs");
    assert_eq!(
        report.sent.values().sum::<u64>(),
        1,
        "the event before it is stored"
    );
    assert_eq!(report.problems.len(), 1, "{report:?}");
    let problem = &report.problems[0];
    assert_eq!(problem.code, SyncCode::IdentityMismatch);
    assert_eq!(problem.line, Some(2));
    let device =
        std::fs::read_to_string(alice.root.join("data/local").join(PROJECT).join("device"))
            .expect("a device");
    assert_eq!(
        problem.path,
        alice
            .root
            .join("data/local")
            .join(PROJECT)
            .join("log")
            .join(device.trim())
            .join("1.ndjson")
            .display()
            .to_string()
    );
}

#[test]
fn refused_and_unreachable_servers_are_errors() {
    let scratch = Scratch::new("refused");
    let mut server = scratch.start(&json!({}), "");
    let carol = Device::new(&scratch, "carol");
    carol.set_step("carol", "human", StepStatus::Done);
    assert_eq!(
        carol.sync(&server, "carol"),
        Err(ProcessError::Refused {
            code: "forbidden".into(),
            message: format!("`carol` has no access to `{PROJECT}`"),
        })
    );
    assert!(matches!(
        carol.sync(&server, "mallory"),
        Err(ProcessError::Refused { code, .. }) if code == "unauthenticated"
    ));
    let base = server.base.clone();
    server.stop();
    let unreachable = carol
        .store()
        .sync(Peer::Server(&Server::new(&base, "token-carol")));
    assert!(
        matches!(unreachable, Err(ProcessError::Unavailable { .. })),
        "{unreachable:?}"
    );
}

#[test]
fn a_server_sync_works_inside_a_tokio_runtime() {
    let scratch = Scratch::new("inside-runtime");
    let server = scratch.start(&json!({}), "");
    let alice = Device::new(&scratch, "alice");
    alice.set_step("alice", "human", StepStatus::Done);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let report = runtime.block_on(async { alice.sync(&server, "alice") });
    assert_eq!(report.expect("syncs").sent.values().sum::<u64>(), 1);
}

#[test]
fn watch_imports_live_and_pushes_new_own_events() {
    let scratch = Scratch::new("watch");
    let server = scratch.start(&json!({}), "");
    let alice = Device::new(&scratch, "alice");
    let bob = Device::new(&scratch, "bob");
    alice.set_step("alice", "human", StepStatus::Done);
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let reports = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        let watcher = scope.spawn(|| {
            let server = Server::new(&server.base, "token-alice");
            alice.store().watch(
                &server,
                async {
                    let _ = stopped.await;
                },
                |report| reports.lock().expect("lockable").push(report.clone()),
            )
        });
        let wait_for = |condition: &dyn Fn() -> bool| {
            let started = Instant::now();
            while !condition() {
                assert!(started.elapsed() < Duration::from_secs(10), "timed out");
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        let cursor_has = |device: &Device, events: u64| {
            device
                .store()
                .cursor()
                .map(|cursor| cursor.values().sum::<u64>() == events)
                .unwrap_or(false)
        };
        // Bob's event arrives at alice live.
        bob.set_step("bob", "human", StepStatus::Skipped);
        assert!(bob.sync(&server, "bob").expect("syncs").problems.is_empty());
        wait_for(&|| cursor_has(&alice, 2));
        // Alice's new own event reaches bob through the server without another sync of hers.
        alice.set_step("alice", "human", StepStatus::Open);
        wait_for(&|| {
            bob.sync(&server, "bob").expect("syncs");
            cursor_has(&bob, 3)
        });
        stop.send(()).expect("the watcher waits");
        assert_eq!(watcher.join().expect("joined"), Ok(()));
    });
    let reports = reports.into_inner().expect("lockable");
    assert!(
        reports.iter().any(|report| !report.received.is_empty()),
        "{reports:?}"
    );
    assert!(
        reports
            .iter()
            .filter(|report| !report.sent.is_empty())
            .count()
            >= 2,
        "{reports:?}"
    );
}

#[test]
fn a_configuration_is_checked_at_load() {
    let scratch = Scratch::new("config");
    let path = scratch.root.join("server.yaml");
    let base = format!(
        "listen: 127.0.0.1:0\ndata: data\nidentities:\n  alice: {{ token: \"sha256:{ALICE}\" }}\nprojects:\n  {PROJECT}: {{ workspace: local, members: [alice] }}\n"
    );
    std::fs::write(&path, &base).expect("writable");
    assert!(Config::load(&path).is_ok());
    for (broken, message) in [
        (
            base.replace("listen: 127.0.0.1:0", "listen: localhost"),
            "`listen` is not an address",
        ),
        (
            base.replace(&format!("sha256:{ALICE}"), "token-alice"),
            "the token of `alice` is not",
        ),
        (
            base.replace("members: [alice]", "members: [bob]"),
            "the member `bob`",
        ),
        (
            base.replace("workspace: local", "workspace: acme"),
            "is not a workspace and a project ID",
        ),
        (format!("{base}extra: 1\n"), "the unknown key `extra`"),
        (
            format!(
                "{base}  prj-4h6j8k2m: {{ workspace: local, members: [alice] }}\nlimits: {{ request_bytes: 0 }}\n"
            ),
            "`request_bytes` is not a positive integer",
        ),
        (
            base.replace(
                "projects:",
                &format!("  bob: {{ token: \"sha256:{ALICE}\" }}\nprojects:"),
            ),
            "belongs to another identity",
        ),
    ] {
        std::fs::write(&path, &broken).expect("writable");
        let error = Config::load(&path).expect_err(message);
        assert!(error.contains(message), "{error}");
    }
}
