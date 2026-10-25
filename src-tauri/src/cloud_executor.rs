use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tokio::sync::Semaphore;

const MAX_CONCURRENT_CLOUD_JOBS: usize = 6;

static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static QUEUED_COUNT: AtomicU64 = AtomicU64::new(0);
static STARTED_COUNT: AtomicU64 = AtomicU64::new(0);
static COMPLETED_COUNT: AtomicU64 = AtomicU64::new(0);
static FAILED_COUNT: AtomicU64 = AtomicU64::new(0);
static QUEUE_WAIT_TOTAL_MICROS: AtomicU64 = AtomicU64::new(0);

fn semaphore() -> Arc<Semaphore> {
    Arc::clone(SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_CLOUD_JOBS))))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudExecutorMetrics {
    pub max_concurrent_jobs: usize,
    pub queued_count: u64,
    pub started_count: u64,
    pub completed_count: u64,
    pub failed_count: u64,
    pub queue_wait_total_ms: f64,
}

pub fn metrics() -> CloudExecutorMetrics {
    CloudExecutorMetrics {
        max_concurrent_jobs: MAX_CONCURRENT_CLOUD_JOBS,
        queued_count: QUEUED_COUNT.load(Ordering::Relaxed),
        started_count: STARTED_COUNT.load(Ordering::Relaxed),
        completed_count: COMPLETED_COUNT.load(Ordering::Relaxed),
        failed_count: FAILED_COUNT.load(Ordering::Relaxed),
        queue_wait_total_ms: QUEUE_WAIT_TOTAL_MICROS.load(Ordering::Relaxed) as f64 / 1_000.0,
    }
}

pub async fn run<T, F>(job: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    QUEUED_COUNT.fetch_add(1, Ordering::Relaxed);
    let queued_at = Instant::now();
    let permit = semaphore()
        .acquire_owned()
        .await
        .map_err(|_| "CLOUD_EXECUTOR_CLOSED: 云端后台执行器已关闭".to_string())?;
    QUEUE_WAIT_TOTAL_MICROS.fetch_add(
        queued_at.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
        Ordering::Relaxed,
    );
    STARTED_COUNT.fetch_add(1, Ordering::Relaxed);
    let joined = tauri::async_runtime::spawn_blocking(job).await;
    drop(permit);
    match joined {
        Ok(Ok(value)) => {
            COMPLETED_COUNT.fetch_add(1, Ordering::Relaxed);
            Ok(value)
        }
        Ok(Err(error)) => {
            FAILED_COUNT.fetch_add(1, Ordering::Relaxed);
            Err(error)
        }
        Err(error) => {
            FAILED_COUNT.fetch_add(1, Ordering::Relaxed);
            Err(format!(
                "CLOUD_EXECUTOR_JOIN_FAILED: 云端后台任务异常结束: {error}"
            ))
        }
    }
}

pub async fn run_for_storage<T, F>(database: crate::database::Database, job: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&crate::database::Database) -> Result<T, String> + Send + 'static,
{
    if crate::storage_contract::storage_backend(&database)?
        == crate::storage_contract::StorageBackend::DirectSupabase
    {
        run(move || job(&database)).await
    } else {
        job(&database)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn limits_concurrency_and_converts_panics_to_errors() {
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let mut jobs = Vec::new();
        for _ in 0..12 {
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            jobs.push(tokio::spawn(run(move || {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(15));
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            })));
        }
        for job in jobs {
            job.await.unwrap().unwrap();
        }
        assert!(maximum.load(Ordering::SeqCst) <= MAX_CONCURRENT_CLOUD_JOBS);

        let error = run::<(), _>(|| panic!("executor test panic"))
            .await
            .unwrap_err();
        assert!(error.starts_with("CLOUD_EXECUTOR_JOIN_FAILED:"));
        let snapshot = metrics();
        assert!(snapshot.queued_count >= 13);
        assert!(snapshot.completed_count >= 12);
        assert!(snapshot.failed_count >= 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slow_cloud_work_does_not_block_lightweight_async_work() {
        let slow = tokio::spawn(run(|| {
            std::thread::sleep(Duration::from_millis(150));
            Ok("cloud")
        }));
        tokio::time::sleep(Duration::from_millis(10)).await;

        let lightweight = tokio::time::timeout(Duration::from_millis(50), async { 42 })
            .await
            .expect("lightweight work should remain schedulable");
        assert_eq!(lightweight, 42);
        assert_eq!(slow.await.unwrap().unwrap(), "cloud");
    }

    #[test]
    fn metrics_are_non_sensitive_counters_only() {
        let serialized = serde_json::to_string(&metrics()).unwrap();
        for sensitive in [
            "project_url",
            "anon_key",
            "access_token",
            "task_title",
            "report_markdown",
        ] {
            assert!(!serialized.contains(sensitive));
        }
    }
}
