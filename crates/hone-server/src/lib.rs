//! Reference replication server of the hone protocol (concept Section 4, "Server transport"):
//! cursor, append, read, and the real-time subscription over HTTP, with bearer
//! authentication. The checks of an append live in `hone-core`; this crate holds the
//! transport, the authentication, and the configuration (concept Section 12, "Reference
//! server").

mod config;
mod routes;

use std::net::SocketAddr;

pub use config::Config;

/// A running server. Dropping it does not stop it; [`Handle::stop`] does.
#[derive(Debug)]
pub struct Handle {
    address: SocketAddr,
    stop: tokio::sync::watch::Sender<bool>,
    thread: std::thread::JoinHandle<std::io::Result<()>>,
}

impl Handle {
    /// The address the server listens on, with the port it was given.
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Stops the server: ends every open subscription, finishes the running requests, and
    /// waits until it has stopped.
    ///
    /// # Errors
    ///
    /// Returns the error that ended the server, if any.
    pub fn stop(self) -> std::io::Result<()> {
        let _ = self.stop.send(true);
        self.thread
            .join()
            .unwrap_or_else(|_| Err(std::io::Error::other("the server stopped unexpectedly")))
    }
}

/// Starts the server on a thread and a runtime of its own and returns once it listens.
///
/// # Errors
///
/// Returns an error if the runtime cannot start or the address cannot be bound.
pub fn start(config: Config) -> std::io::Result<Handle> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let listener = runtime.block_on(tokio::net::TcpListener::bind(config.listen))?;
    let address = listener.local_addr()?;
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let thread =
        std::thread::spawn(move || runtime.block_on(routes::serve(config, listener, stopped)));
    Ok(Handle {
        address,
        stop,
        thread,
    })
}

/// Runs the server until the process receives Ctrl-C or, on Unix, `SIGTERM`.
///
/// # Errors
///
/// Returns an error if the server cannot start or fails while running.
pub fn serve(config: Config) -> std::io::Result<()> {
    let handle = start(config)?;
    eprintln!("hone serve: listening on {}", handle.address());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(termination())?;
    handle.stop()
}

async fn termination() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}
