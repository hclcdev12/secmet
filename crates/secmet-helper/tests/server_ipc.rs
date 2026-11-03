//! Tests d'intégration du daemon : socket réel, authentification par jeton
//! d'amorçage, séquencement des commandes, limites de trame. Sans root : les
//! disques ne sont jamais ouverts, mais chaque refus doit être le bon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use secmet_device::sysfs::SysfsPaths;
use secmet_helper::ipc::{self, Command, Message, Request};
use secmet_helper::polkit::{self, Bootstrap};
use secmet_helper::server::{Server, ServerConfig, SOCKET_NAME};

struct Harness {
    _tmp: tempfile::TempDir,
    server: Arc<Server>,
    bootstrap: String,
    thread: Option<thread::JoinHandle<()>>,
}

impl Harness {
    fn start(ttl: Duration) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let socket_path = polkit::prepare_socket_dir(&tmp.path().join("run"), SOCKET_NAME).unwrap();
        let bootstrap = Bootstrap::from_bytes([0x42; 32]);
        let server = Server::new(ServerConfig {
            socket_path,
            allowed_uid: polkit::effective_uid(),
            bootstrap: bootstrap.clone(),
            ttl,
            paths: SysfsPaths::default(),
            vault_dir: tmp.path().join("vault"),
        });
        let listener = server.bind().unwrap();
        let runner = Arc::clone(&server);
        let thread = thread::spawn(move || runner.run(listener).unwrap());
        Self {
            _tmp: tmp,
            server,
            bootstrap: bootstrap.to_hex(),
            thread: Some(thread),
        }
    }

    fn connect(&self) -> Client {
        let stream = UnixStream::connect(self.server.socket_path()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        Client {
            reader: BufReader::new(stream.try_clone().unwrap()),
            stream,
            next_id: 1,
        }
    }

    fn authenticated(&self) -> Client {
        let mut client = self.connect();
        let reply = client.call(Command::Authenticate {
            bootstrap: self.bootstrap.clone(),
        });
        assert!(matches!(reply, Message::Ok { .. }), "{reply:?}");
        client
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.request_shutdown();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Client {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl Client {
    fn call(&mut self, cmd: Command) -> Message {
        let id = self.next_id;
        self.next_id += 1;
        ipc::write_line(&mut self.stream, &Request { id, cmd }).unwrap();
        let message = ipc::read_message(&mut self.reader).unwrap();
        match &message {
            Message::Ok { id: got, .. } | Message::Error { id: got, .. } => assert_eq!(*got, id),
            Message::Event { .. } => panic!("événement inattendu: {message:?}"),
        }
        message
    }

    fn error_kind(&mut self, cmd: Command) -> String {
        match self.call(cmd) {
            Message::Error { error, .. } => error.kind,
            other => panic!("erreur attendue, obtenu {other:?}"),
        }
    }
}

#[test]
fn commands_before_authentication_are_refused() {
    let h = Harness::start(Duration::from_secs(600));
    let mut c = h.connect();
    assert_eq!(
        c.error_kind(Command::GetStatus { token: "x".into() }),
        "unauthenticated"
    );
    assert_eq!(
        c.error_kind(Command::ScanDevice {
            device: "/dev/sdb".into(),
            policy: Default::default(),
            preferences: Default::default(),
        }),
        "unauthenticated"
    );
    assert_eq!(c.error_kind(Command::Shutdown {}), "unauthenticated");
}

#[test]
fn a_wrong_bootstrap_is_refused_and_the_connection_closed() {
    let h = Harness::start(Duration::from_secs(600));
    let mut c = h.connect();
    assert_eq!(
        c.error_kind(Command::Authenticate {
            bootstrap: "00".repeat(32),
        }),
        "bad_bootstrap"
    );
    // Plus rien n'est lu sur cette connexion.
    assert!(
        ipc::write_line(
            &mut c.stream,
            &Request {
                id: 9,
                cmd: Command::Shutdown {}
            }
        )
        .is_err()
            || ipc::read_message(&mut c.reader).is_err()
    );
    // Une nouvelle connexion avec le bon secret fonctionne toujours.
    let mut ok = h.authenticated();
    assert_eq!(
        ok.error_kind(Command::GetStatus { token: "x".into() }),
        "token_unknown"
    );
}

#[test]
fn scan_refuses_names_outside_the_allowlist_and_unknown_tokens() {
    let h = Harness::start(Duration::from_secs(600));
    let mut c = h.authenticated();
    for device in ["/dev/loop0", "/etc/passwd", "../sda", "/dev/sda1"] {
        assert_eq!(
            c.error_kind(Command::ScanDevice {
                device: device.into(),
                policy: Default::default(),
                preferences: Default::default(),
            }),
            "device",
            "{device}"
        );
    }
    for cmd in [
        Command::PrepareCanaries {
            token: "nope".into(),
        },
        Command::ExecutePlan {
            token: "0".repeat(64),
            confirm_serial: "S".into(),
            policy: Default::default(),
            preferences: Default::default(),
        },
        Command::Cancel {
            token: String::new(),
        },
        Command::GetStatus {
            token: "1".repeat(64),
        },
    ] {
        assert_eq!(c.error_kind(cmd), "token_unknown");
    }
}

#[test]
fn malformed_json_and_unknown_fields_are_protocol_errors_without_closing() {
    let h = Harness::start(Duration::from_secs(600));
    let mut c = h.authenticated();
    c.stream.write_all(b"{not json\n").unwrap();
    match ipc::read_message(&mut c.reader).unwrap() {
        Message::Error { id, error } => {
            assert_eq!(id, 0);
            assert_eq!(error.kind, "protocol");
        }
        other => panic!("{other:?}"),
    }
    c.stream
        .write_all(b"{\"id\":5,\"cmd\":{\"type\":\"Shutdown\",\"force\":true}}\n")
        .unwrap();
    assert!(matches!(
        ipc::read_message(&mut c.reader).unwrap(),
        Message::Error { id: 0, .. }
    ));
    // La connexion est toujours utilisable.
    assert_eq!(
        c.error_kind(Command::GetStatus { token: "x".into() }),
        "token_unknown"
    );
}

#[test]
fn an_oversized_line_closes_the_connection() {
    let h = Harness::start(Duration::from_secs(600));
    let mut c = h.authenticated();
    let huge = vec![b'a'; ipc::MAX_MESSAGE_BYTES + 10];
    let _ = c.stream.write_all(&huge);
    let _ = c.stream.write_all(b"\n");
    assert!(ipc::read_message(&mut c.reader).is_err());
    assert!(h.authenticated().next_id == 2, "le serveur accepte encore");
}

#[test]
fn shutdown_stops_the_accept_loop_and_removes_the_socket() {
    let mut h = Harness::start(Duration::from_secs(600));
    let mut c = h.authenticated();
    match c.call(Command::Shutdown {}) {
        Message::Ok { result, .. } => {
            assert_eq!(result["stopping"], true);
            assert_eq!(result["deferred"], false, "aucun job : arrêt immédiat");
        }
        other => panic!("{other:?}"),
    }
    let thread = h.thread.take().unwrap();
    thread.join().unwrap();
    assert!(!h.server.socket_path().exists());
    assert!(UnixStream::connect(h.server.socket_path()).is_err());
}
