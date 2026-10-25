use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudReleaseMetrics {
    pub query_count: u64,
    pub query_error_count: u64,
    pub query_latency_total_ms: f64,
    pub query_latency_max_ms: f64,
    pub mutation_count: u64,
    pub mutation_error_count: u64,
    pub mutation_latency_total_ms: f64,
    pub mutation_latency_max_ms: f64,
    pub mutation_response_bytes_total: u64,
    pub mutation_response_bytes_max: u64,
    pub client_apply_latency_total_ms: f64,
    pub client_apply_count: u64,
    pub refresh_started_count: u64,
    pub refresh_reused_count: u64,
    pub refresh_suppressed_count: u64,
    pub refresh_followup_count: u64,
    pub protocol_rejection_count: u64,
    pub realtime_reconnect_count: u64,
    pub copy_failure_count: u64,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudReleaseMetricsSnapshot {
    pub workspace_id: String,
    #[serde(flatten)]
    pub metrics: DirectCloudReleaseMetrics,
    pub average_query_latency_ms: f64,
    pub average_mutation_latency_ms: f64,
    pub average_client_apply_latency_ms: f64,
    pub cloud_executor: crate::cloud_executor::CloudExecutorMetrics,
    pub http_client_creation_count: u64,
}

static METRICS: OnceLock<Mutex<HashMap<String, DirectCloudReleaseMetrics>>> = OnceLock::new();

fn metrics() -> &'static Mutex<HashMap<String, DirectCloudReleaseMetrics>> {
    METRICS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn update(workspace_id: &str, action: impl FnOnce(&mut DirectCloudReleaseMetrics)) {
    let mut metrics = metrics()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    action(metrics.entry(workspace_id.to_string()).or_default());
}

fn error_code(error: &str) -> String {
    error
        .split_once(':')
        .map(|(code, _)| code.trim())
        .filter(|code| !code.is_empty())
        .unwrap_or("UNKNOWN")
        .to_string()
}

pub fn record_query(workspace_id: &str, duration_ms: f64) {
    update(workspace_id, |metrics| {
        metrics.query_count += 1;
        metrics.query_latency_total_ms += duration_ms.max(0.0);
        metrics.query_latency_max_ms = metrics.query_latency_max_ms.max(duration_ms.max(0.0));
    });
}

pub fn record_query_error(workspace_id: &str, error: &str) {
    update(workspace_id, |metrics| {
        metrics.query_count += 1;
        metrics.query_error_count += 1;
        metrics.last_error_code = Some(error_code(error));
        if error.starts_with("CLOUD_PROTOCOL_UNSUPPORTED:")
            || error.starts_with("WORKSPACE_EPOCH_STALE:")
            || error.starts_with("EMERGENCY_READ_ONLY:")
        {
            metrics.protocol_rejection_count += 1;
        }
    });
}

pub fn record_mutation(workspace_id: &str, duration_ms: f64, response_bytes: u64) {
    update(workspace_id, |metrics| {
        metrics.mutation_count += 1;
        metrics.mutation_latency_total_ms += duration_ms.max(0.0);
        metrics.mutation_latency_max_ms = metrics.mutation_latency_max_ms.max(duration_ms.max(0.0));
        metrics.mutation_response_bytes_total += response_bytes;
        metrics.mutation_response_bytes_max =
            metrics.mutation_response_bytes_max.max(response_bytes);
    });
}

pub fn record_client_apply(workspace_id: &str, duration_ms: f64) {
    update(workspace_id, |metrics| {
        metrics.client_apply_count += 1;
        metrics.client_apply_latency_total_ms += duration_ms.max(0.0);
    });
}

pub fn record_refresh_metric(workspace_id: &str, metric: &str) {
    update(workspace_id, |metrics| match metric {
        "started" => metrics.refresh_started_count += 1,
        "reused" => metrics.refresh_reused_count += 1,
        "suppressed" => metrics.refresh_suppressed_count += 1,
        "followup" => metrics.refresh_followup_count += 1,
        _ => {}
    });
}

pub fn record_mutation_error(workspace_id: &str, error: &str) {
    update(workspace_id, |metrics| {
        metrics.mutation_count += 1;
        metrics.mutation_error_count += 1;
        metrics.last_error_code = Some(error_code(error));
        if error.starts_with("CLOUD_PROTOCOL_UNSUPPORTED:")
            || error.starts_with("WORKSPACE_EPOCH_STALE:")
            || error.starts_with("EMERGENCY_READ_ONLY:")
        {
            metrics.protocol_rejection_count += 1;
        }
    });
}

pub fn record_realtime_reconnect(workspace_id: &str) {
    update(workspace_id, |metrics| {
        metrics.realtime_reconnect_count += 1
    });
}

pub fn record_copy_failure(workspace_id: &str, error: &str) {
    update(workspace_id, |metrics| {
        metrics.copy_failure_count += 1;
        metrics.last_error_code = Some(error_code(error));
    });
}

pub fn snapshot(workspace_id: &str) -> DirectCloudReleaseMetricsSnapshot {
    let metrics = metrics()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(workspace_id)
        .cloned()
        .unwrap_or_default();
    let average_query_latency_ms = if metrics.query_count == 0 {
        0.0
    } else {
        metrics.query_latency_total_ms / metrics.query_count as f64
    };
    let average_mutation_latency_ms = if metrics.mutation_count == 0 {
        0.0
    } else {
        metrics.mutation_latency_total_ms / metrics.mutation_count as f64
    };
    let average_client_apply_latency_ms = if metrics.client_apply_count == 0 {
        0.0
    } else {
        metrics.client_apply_latency_total_ms / metrics.client_apply_count as f64
    };
    DirectCloudReleaseMetricsSnapshot {
        workspace_id: workspace_id.to_string(),
        metrics,
        average_query_latency_ms,
        average_mutation_latency_ms,
        average_client_apply_latency_ms,
        cloud_executor: crate::cloud_executor::metrics(),
        http_client_creation_count: crate::supabase::client_creation_count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_metrics_cover_queries_mutations_protocol_rejections_realtime_and_copy_failures() {
        let workspace_id = "observability-workspace";
        record_query(workspace_id, 12.5);
        record_query_error(workspace_id, "CLOUD_TIMEOUT: timeout");
        record_mutation(workspace_id, 7.5, 640);
        record_mutation_error(workspace_id, "EMERGENCY_READ_ONLY: maintenance");
        record_client_apply(workspace_id, 2.5);
        record_refresh_metric(workspace_id, "started");
        record_refresh_metric(workspace_id, "reused");
        record_refresh_metric(workspace_id, "suppressed");
        record_refresh_metric(workspace_id, "followup");
        record_realtime_reconnect(workspace_id);
        record_copy_failure(workspace_id, "DATA_COPY_HASH_MISMATCH: invalid");

        let snapshot = snapshot(workspace_id);
        assert_eq!(snapshot.metrics.query_count, 2);
        assert_eq!(snapshot.metrics.query_error_count, 1);
        assert_eq!(snapshot.metrics.mutation_count, 2);
        assert_eq!(snapshot.metrics.mutation_error_count, 1);
        assert_eq!(snapshot.metrics.mutation_response_bytes_total, 640);
        assert_eq!(snapshot.metrics.mutation_response_bytes_max, 640);
        assert_eq!(snapshot.metrics.protocol_rejection_count, 1);
        assert_eq!(snapshot.metrics.realtime_reconnect_count, 1);
        assert_eq!(snapshot.metrics.copy_failure_count, 1);
        assert_eq!(snapshot.metrics.refresh_started_count, 1);
        assert_eq!(snapshot.metrics.refresh_reused_count, 1);
        assert_eq!(snapshot.metrics.refresh_suppressed_count, 1);
        assert_eq!(snapshot.metrics.refresh_followup_count, 1);
        assert_eq!(snapshot.average_mutation_latency_ms, 3.75);
        assert_eq!(snapshot.average_client_apply_latency_ms, 2.5);
        assert_eq!(snapshot.cloud_executor.max_concurrent_jobs, 6);
        assert_eq!(
            snapshot.metrics.last_error_code.as_deref(),
            Some("DATA_COPY_HASH_MISMATCH")
        );
        assert_eq!(snapshot.average_query_latency_ms, 6.25);
    }
}
