//! System signals handling
#[allow(unused)]
use tracing::{debug, info, warn, error};
use tokio::signal::unix::{signal, SignalKind};

/// Receive system signals as a `Future`.
/// If the signal is `SIGINT` or `SIGTERM` or `SIGQUIT`, the future will resolve to `Ok(())`.
/// Else, the future will not resolve.
#[cfg(unix)]
pub async fn signals() -> std::io::Result<()> {
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut term = signal(SignalKind::terminate())?;
    let mut quit = signal(SignalKind::quit())?;

    tokio::select! {
        _ = interrupt.recv() => {
            warn!("Interrupt signal received");
            Ok(())
        },
        _ = term.recv() => {
            warn!("Terminate signal received");
            Ok(())
        },
        _ = quit.recv() => {
            warn!("Quit signal received");
            Ok(())
        }
    }
}

/// Don't support Windows platform.
#[cfg(windows)]
pub async fn signals() -> Result<()> {
    unimplemented!("Signal handling didn't implemented on Windows");
}
