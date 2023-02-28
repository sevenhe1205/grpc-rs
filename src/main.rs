

mod metrics;
mod service;
mod signals;

use std::{net::SocketAddr, sync::Arc};

use clap::Parser;
use futures::{Future, stream, FutureExt, StreamExt, TryFutureExt, TryStreamExt};
use service::{EchoService, EchoServer};
use tokio::{sync::{broadcast, mpsc}, task::JoinHandle};
use axum::{
    error_handling::HandleErrorLayer, http, Router, routing::get, Json, middleware,
};
use tower::ServiceBuilder;
use tower_http::ServiceBuilderExt;

#[allow(unused)]
use tracing::{trace, debug, info, warn, error};
use tracing::{
    info_span,
    Instrument,
};
use tracing_subscriber::{Layer, prelude::*};
use tracing_subscriber::{
    filter::LevelFilter,
    EnvFilter,
};

use anyhow::Result;

#[derive(Debug, Clone, clap::ValueEnum)]
enum LogFormat {
    Plain,
    Pretty,
    Json,
}

const DEFAULT_GRPC_LISTEN: &str = "0.0.0.0:8090";
const DEFAULT_HTTP_LISTEN: &str = "0.0.0.0:8080";

const VERSION: &str = concat!(
    env!("VERGEN_GIT_SEMVER"),
    env!("PROJECT_VERSION"),
);

#[derive(Debug, Parser)]
#[command(
    author,
    version = VERSION,
    about = "A gRPC server",
)]
pub struct Opts {

    /// Flatten fields in JSON log format, default is false
    #[arg(long, env = "LOG_FLATTEN_JSON")]
    flatten_json: bool,

    #[arg(short = 'f', long, env = "LOG_FORMAT", value_enum, default_value_t = LogFormat::Plain)]
    log_format: LogFormat,

    #[arg(long = "grpc-listen", env = "GRPC_LISTEN_ADDR", default_value = DEFAULT_GRPC_LISTEN)]
    grpc_address: SocketAddr,

    #[arg(long = "http-listen", env = "HTTP_LISTEN_ADDR", default_value = DEFAULT_HTTP_LISTEN)]
    http_address: SocketAddr,

    #[arg(long = "http-health-path", env = "HTTP_HEALTH_PATH", default_value = "/health")]
    http_health_path: String,

    #[arg(long = "prometheus-prefix", env = "PROMETHEUS_PREFIX", default_value = "/metrics")]
    prometheus_prefix: String,

    #[arg(long = "prometheus-enable", env = "PROMETHEUS_ENABLE")]
    prometheus_enable: bool,

    /// Enable log output ansi color in `pretty` or `plain` format, default is true
    #[arg(action, long, env = "LOG_ANSI")]
    without_ansi: bool,
}

pub const fn app_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

pub const fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Opts::parse();

    let opts = Arc::new(cli);
    init_tracing(opts.clone());

    let shutdown = handle_signals()?.then(|_| async {
        debug!("Received shutdown signal");
    }).shared();

    let (shutdown_complete_tx, shutdown_complete_rx) = mpsc::channel(1);

    let metrics_handler = metrics::init_recorder(opts.clone())?;

    run_server(
        opts.clone(),
        metrics_handler,
        shutdown,
        shutdown_complete_tx,
        shutdown_complete_rx
    ).instrument(info_span!("server")).await;

    Ok(())
}

/// Run gRPC server and then register service to SD
async fn run_server(
    opts: Arc<Opts>,
    metrics_handler: metrics::MetricsHandler,
    shutdown: impl Future,
    shutdown_complete_tx: mpsc::Sender<()>,
    mut shutdown_complete_rx: mpsc::Receiver<()>,
) {
    info!("Starting server");

    // Prepare the server
    //
    // Initialize Kafka producer and NSQ producer
    // Initialize rule engine

    let (notify_shutdown, _) = broadcast::channel::<()>(1);
    let mut handlers = vec![];
    handlers.push(spawn_grpc_server(opts.clone(), &notify_shutdown, shutdown_complete_tx.clone()));
    handlers.push(spawn_http_server(opts.clone(), metrics_handler, &notify_shutdown, shutdown_complete_tx.clone()));

    // Wait for shutdown signal
    shutdown.await;
    info!("Graceful shutdown server");

    // When `notify_shutdown` is dropped, all tasks which have `subscribe`d will
    // receive the shutdown signal and can exit
    let _ = notify_shutdown.send(());
    drop(notify_shutdown);
    // Drop final `Sender` so the `Receiver` below can complete
    drop(shutdown_complete_tx);

    // Wait for all active connections to finish processing. As the `Sender`
    // handle held by the listener has been dropped above, the only remaining
    // `Sender` instances are held by connection handler tasks. When those drop,
    // the `mpsc` channel will close and `recv()` will return `None`.
    while let Some(_signal) = shutdown_complete_rx.recv().await {
        // TODO unregister service before shutting down other services
    }

    futures::future::join_all(handlers).await;

    info!("Server stopped");
}



fn spawn_http_server(
    opts: Arc<Opts>,
    metrics_handler: metrics::MetricsHandler,
    shutdown: &broadcast::Sender<()>,
    shutdown_complete_tx: mpsc::Sender<()>,
) -> JoinHandle<()> {
    let mut shutdown = shutdown.subscribe();
    tokio::spawn(async move {
        let shutdown = shutdown.recv().map(|_| ());
        let _ = start_http(opts, metrics_handler, shutdown).await;
        let _ = shutdown_complete_tx.send(()).await;
        warn!("HTTP server stopped");
    }.instrument(info_span!("http")))
}

async fn start_http(opts: Arc<Opts>, metrics_handler: metrics::MetricsHandler, shutdown: impl Future<Output = ()>) -> Result<()> {
    let health_path = &opts.http_health_path;
    let addr = &opts.http_address;
    let health_path = format!("/{}", health_path.trim_start_matches('/'));
    let server_name = format!("{}/{}", app_name(), app_version());

    let body =  "ok!".to_string();
    let health_checking = Router::new()
        .route("/", get(|| async { body }))
        .layer(
            ServiceBuilder::new()
                .append_response_header(
                    http::header::SERVER,
                    http::header::HeaderValue::from_str(&server_name).ok()
                )
                .append_response_header(
                    http::header::HeaderName::from_static("oops"),
                    http::header::HeaderValue::from_static("DONT'T PANIC!")
                )
        );

    info!(path = %health_path, addr = %addr, "Started HTTP health checking");
    let router = Router::new()
        .route("/info", get(server_info))
        .nest(&health_path, health_checking)
    ;

    let router = if opts.prometheus_enable {
        let metrics_path = opts.prometheus_prefix.as_str();
        let metrics_path = format!("/{}", metrics_path.trim_start_matches('/'));
        info!(path = %metrics_path, addr = %addr, "Started Prometheus metrics");
        router.route(&metrics_path, get(move || std::future::ready(metrics_handler.render())))
    } else {
        router
    };

    let router = router.route_layer(middleware::from_fn(metrics::track_metrics));

    axum::Server::bind(addr)
        .serve(router.into_make_service())
        .with_graceful_shutdown(shutdown)
        .await?;

    Ok(())
}

fn spawn_grpc_server(
    opts: Arc<Opts>,
    shutdown: &broadcast::Sender<()>,
    shutdown_complete_tx: mpsc::Sender<()>,
) -> JoinHandle<()> {
    let mut shutdown = shutdown.subscribe();
    let listen_addresses = [opts.grpc_address].to_vec();
    tokio::spawn(async move {
        let shutdown = shutdown.recv().map(|_| ());
        let _ = start_grpc(listen_addresses, shutdown).await;
        let _ = shutdown_complete_tx.send(()).await;
        warn!("gRPC server stopped");
    }.instrument(info_span!("grpc")))
}

/// Start gRPC server for handling requests
async fn start_grpc(
    listen_addresses: Vec<SocketAddr>,
    shutdown: impl Future,
) -> Result<()> {
    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<EchoServer<EchoService>>()
        .await;

    // Support listening on multiple ports for gRPC
    let listeners = stream::iter(listen_addresses)
        .then(|addr| async move {
            tokio::net::TcpListener::bind(addr)
                .inspect_ok(|_| info!(%addr, "Started gRPC listening"))
                .await
        })
        .map_ok(|listener| {
            stream::try_unfold(listener, |listener| async {
                let socket = listener.accept()
                    .inspect_ok(|(_, addr)| debug!(%addr, "accepted new connection"))
                    .map_ok(|(socket, _)| socket).await?;
                Ok::<_, std::io::Error>(Some((socket, listener)))
            }).boxed()
        })
        .try_collect::<Vec<_>>().await?;

    let incoming = stream::select_all(listeners);
    let shutdown = shutdown.map(|_| warn!("gRPC server received shutdown signal"));

    let grpc_reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(crate::service::echo::FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(tonic_health::proto::GRPC_HEALTH_V1_FILE_DESCRIPTOR_SET)
        .build()
        .expect("failed to build reflection turbine service");

    tonic::transport::Server::builder()
        .max_concurrent_streams(65_535)
        .concurrency_limit_per_connection(2_000)
        .timeout(std::time::Duration::from_secs(5))
        .layer(
            ServiceBuilder::new()
            .layer(HandleErrorLayer::new(|error: tower::BoxError| async move {
                (
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Unhandled internal error: {}", error),
                )
            }))
        )
        .add_service(health_service)
        .add_service(grpc_reflection)
        .add_service(service::EchoServer::new(EchoService::new()))
        .serve_with_incoming_shutdown(incoming, shutdown)
        .map_err(From::from)
        .await
}

/// Handle system signals
fn handle_signals() -> std::io::Result<tokio::sync::oneshot::Receiver<()>> {
    info!("Starting signal handling");

    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        match signals::signals().await {
            Ok(_) => {}
            Err(error) => {
                error!(?error, "System signals error");
            }
        }
        let _ = tx.send(());
    }.instrument(info_span!("signals")));
    Ok(rx)
}

fn init_tracing(opt: Arc<Opts>) {
    #[cfg(tokio_unstable)]
    let reg = {
        use tracing::Level;
        use tracing_subscriber::filter;

        let console_layer = if opt.with_tokio_console {
            let env_filter_for_console = filter::Targets::new()
                .with_target("tokio", Level::TRACE)
                .with_target("runtime", Level::TRACE)
                .with_default(LevelFilter::OFF);

            // spawn the console server in the background
            let console_layer = console_subscriber::spawn();
            let console_layer = console_layer.with_filter(env_filter_for_console);
            Some(console_layer)
        } else {
            None
        };

        tracing_subscriber::registry().with(console_layer)
    };

    #[cfg(not(tokio_unstable))]
    let reg = tracing_subscriber::registry();

    let env_filter_for_log = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_ansi(!opt.without_ansi)
        .with_target(true)
        .with_thread_names(false)
        .with_writer(std::io::stdout);

    match opt.log_format {
        LogFormat::Json => {
            let fmt_layer = fmt_layer.json().flatten_event(opt.flatten_json)
                .with_filter(env_filter_for_log);
            reg.with(fmt_layer).init();
        }
        LogFormat::Pretty => {
            let fmt_layer = fmt_layer.pretty()
                .with_filter(env_filter_for_log);
            reg.with(fmt_layer).init();
        }
        _ => {
            let fmt_layer = fmt_layer.with_filter(env_filter_for_log);
            reg.with(fmt_layer).init();
        }
    }

    #[cfg(tokio_unstable)]
    if opt.with_tokio_console {
        warn!("tokio-console enabled with default environment variables, see <https://docs.rs/console-subscriber> for more details");
    }
}

async fn server_info() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "build_timestamp":     env!("VERGEN_BUILD_TIMESTAMP"),
        "git_semver":          env!("VERGEN_GIT_SEMVER"),
        "git_sha":             env!("VERGEN_GIT_SHA"),
        "git_commit_date":     env!("VERGEN_GIT_COMMIT_TIMESTAMP"),
        "git_branch":          env!("VERGEN_GIT_BRANCH"),
        "rustc_semver":        env!("VERGEN_RUSTC_SEMVER"),
        "rustc_channel":       env!("VERGEN_RUSTC_CHANNEL"),
        "rustc_host_triple":   env!("VERGEN_RUSTC_HOST_TRIPLE"),
        "rustc_commit_sha":    env!("VERGEN_RUSTC_COMMIT_HASH"),
        "cargo_target_triple": env!("VERGEN_CARGO_TARGET_TRIPLE"),
        "cargo_profile":       env!("VERGEN_CARGO_PROFILE"),
    }))

}
