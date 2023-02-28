//! Handle metrics stuff.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use anyhow::Result;
use axum::{
    extract::MatchedPath,
    http::Request,
    response::IntoResponse,
    middleware::Next,
};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use crate::Opts;

#[allow(unused)]
use tracing::{trace, debug, info, warn, error};



/// The metrics handler.
///
/// Combines the Prometheus metrics handle and the process metrics collector.
#[derive(Clone)]
pub struct MetricsHandler {
    prometheus: PrometheusHandle,
    process_collector: metrics_process::Collector,
}

impl MetricsHandler {

    /// Takes a snapshot of the metrics held by the recorder and generates a payload conforming to
    /// the Prometheus exposition format.
    ///
    /// This function is delegated to the inner [`PrometheusHandle`].
    pub fn render(&self) -> String {
        self.process_collector.collect();
        self.prometheus.render()
    }
}

/// Initialize all metrics and setup the global recorder for the application
pub fn init_recorder(_opts: Arc<Opts>) -> Result<MetricsHandler> {
    // The global recorder MUST be set before any metrics are created
    let recorder_handle = setup_metrics_recorder(&Default::default())?;

    describe_metrics();

    register_metrics();

    let process_collector = metrics_process::Collector::default();
    process_collector.describe();

    Ok(MetricsHandler {
        prometheus: recorder_handle,
        process_collector,
    })
}


const GRPC_REQUESTS_DURATION: &str = "grpc_requests_duration_seconds";
const GRPC_REQUESTS_COUNTER: &str = "grpc_requests_total";

const GRPC_REQUESTS_DURATION_SECONDS_BUCKET: &[f64] = &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0];

fn setup_metrics_recorder(buckets: &HashMap<String, Vec<f64>>) -> Result<PrometheusHandle> {

    let http_requests_bucket = buckets
        .get(GRPC_REQUESTS_DURATION)
        .map(Vec::as_slice)
        .unwrap_or(GRPC_REQUESTS_DURATION_SECONDS_BUCKET);

    let builder = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full(GRPC_REQUESTS_DURATION.to_string()),
            http_requests_bucket,
        )?;

    let handle = builder.install_recorder()?;
    Ok(handle)
}

/// Track HTTP metrics
pub async fn track_metrics<B>(req: Request<B>, next: Next<B>) -> impl IntoResponse {
    let start = Instant::now();
    let path = if let Some(matched_path) = req.extensions().get::<MatchedPath>() {
        matched_path.as_str().to_owned()
    } else {
        req.uri().path().to_owned()
    };
    let method = req.method().clone();

    let response = next.run(req).await;

    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    let labels = [
        ("method", method.to_string()),
        ("path", path),
        ("status", status),
    ];

    metrics::increment_counter!(GRPC_REQUESTS_COUNTER, &labels);
    metrics::histogram!(GRPC_REQUESTS_DURATION, latency, &labels);

    response
}

const RESULT_KEY: &str = "result";
const RESULT_OK: &str = "succeed";
const RESULT_ERR: &str = "failed";

/// A trait for describing a result like type as key-value pairs for metrics.
pub trait AsResultLabel {
    fn as_label(&self) -> (&'static str, &'static str);
}

impl<T, E> AsResultLabel for Result<T, E> {
    fn as_label(&self) -> (&'static str, &'static str) {
        match self {
            Ok(_) => (RESULT_KEY, RESULT_OK),
            Err(_) => (RESULT_KEY, RESULT_ERR),
        }
    }
}

fn describe_metrics() {
    metrics::describe_counter!(GRPC_REQUESTS_COUNTER, "The total number of grpc server receive requests.");
    metrics::describe_histogram!(GRPC_REQUESTS_DURATION, "The latency duration of grpc response");
}

fn register_metrics() {
    metrics::register_counter!(GRPC_REQUESTS_COUNTER);
    metrics::register_histogram!(GRPC_REQUESTS_DURATION);

}
