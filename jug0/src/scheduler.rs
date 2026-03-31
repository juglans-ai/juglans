// src/services/scheduler.rs
//
// Cron scheduler — loads workflows with trigger_config.type="cron" and
// executes them on schedule by POSTing to their endpoint_url.

use chrono::Utc;
use cron::Schedule;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{workflow_runs, workflows};
use crate::AppState;

pub struct CronScheduler;

impl CronScheduler {
    /// Load all active cron workflows and spawn a task for each.
    /// Re-scans when notified via `scheduler_reload` watch channel.
    pub async fn start(state: Arc<AppState>) {
        loop {
            tracing::info!("🕐 [Scheduler] Loading cron workflows...");

            let cron_workflows = match Self::load_cron_workflows(&state).await {
                Ok(wfs) => wfs,
                Err(e) => {
                    tracing::error!("[Scheduler] Failed to load workflows: {}", e);
                    vec![]
                }
            };

            tracing::info!(
                "🕐 [Scheduler] Found {} cron workflow(s)",
                cron_workflows.len()
            );

            // Spawn a task per cron workflow
            let mut handles = Vec::new();
            for wf in cron_workflows {
                let s = state.clone();
                let handle = tokio::spawn(async move {
                    Self::run_cron_loop(s, wf).await;
                });
                handles.push(handle);
            }

            // Wait for reload signal — when received, cancel all tasks and re-scan
            let mut rx = state.scheduler_reload.subscribe();
            let _ = rx.changed().await;

            tracing::info!("🕐 [Scheduler] Reload signal received, restarting...");
            for h in handles {
                h.abort();
            }
        }
    }

    /// Query DB for active workflows with trigger_config.type = "cron"
    async fn load_cron_workflows(
        state: &AppState,
    ) -> Result<Vec<workflows::Model>, sea_orm::DbErr> {
        let all_active = workflows::Entity::find()
            .filter(workflows::Column::IsActive.eq(true))
            .all(&state.db)
            .await?;

        Ok(all_active
            .into_iter()
            .filter(|wf| {
                wf.trigger_config
                    .as_ref()
                    .and_then(|tc| tc.get("type"))
                    .and_then(|t| t.as_str())
                    == Some("cron")
            })
            .collect())
    }

    /// Run the cron loop for a single workflow: sleep until next fire, execute, repeat.
    async fn run_cron_loop(state: Arc<AppState>, wf: workflows::Model) {
        let cron_expr = match wf
            .trigger_config
            .as_ref()
            .and_then(|tc| tc.get("schedule"))
            .and_then(|s| s.as_str())
        {
            Some(expr) => expr.to_string(),
            None => {
                tracing::warn!(
                    "[Scheduler] Workflow {} has no schedule in trigger_config",
                    wf.slug
                );
                return;
            }
        };

        // cron crate needs 6/7 fields (with seconds); add "0" if user gave 5-field
        let full_expr = if cron_expr.split_whitespace().count() == 5 {
            format!("0 {}", cron_expr)
        } else {
            cron_expr.clone()
        };

        let schedule = match Schedule::from_str(&full_expr) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    "[Scheduler] Invalid cron '{}' for workflow {}: {}",
                    cron_expr,
                    wf.slug,
                    e
                );
                return;
            }
        };

        tracing::info!(
            "🕐 [Scheduler] Scheduled workflow '{}' with cron '{}'",
            wf.slug,
            cron_expr
        );

        loop {
            let now = Utc::now();
            let next = match schedule.upcoming(Utc).next() {
                Some(t) => t,
                None => {
                    tracing::warn!("[Scheduler] No upcoming time for workflow '{}'", wf.slug);
                    return;
                }
            };

            let wait = (next - now)
                .to_std()
                .unwrap_or(std::time::Duration::from_secs(1));

            tracing::debug!(
                "[Scheduler] Workflow '{}' next run at {} ({:.0}s)",
                wf.slug,
                next.format("%Y-%m-%d %H:%M:%S UTC"),
                wait.as_secs_f64()
            );

            tokio::time::sleep(wait).await;
            Self::trigger_workflow(&state, &wf).await;
        }
    }

    /// Execute a workflow: create a run record, POST to endpoint_url, update record.
    async fn trigger_workflow(state: &AppState, wf: &workflows::Model) {
        let run_id = Uuid::new_v4();
        let now = Utc::now().naive_utc();

        tracing::info!(
            "🚀 [Scheduler] Triggering workflow '{}' (run {})",
            wf.slug,
            run_id
        );

        // Create run record
        let run = workflow_runs::ActiveModel {
            id: Set(run_id),
            workflow_id: Set(wf.id),
            trigger: Set("cron".to_string()),
            status: Set("running".to_string()),
            started_at: Set(Some(now)),
            ..Default::default()
        };
        if let Err(e) = run.insert(&state.db).await {
            tracing::error!("[Scheduler] Failed to create run record: {}", e);
            return;
        }

        // POST to endpoint_url
        let endpoint = match &wf.endpoint_url {
            Some(url) if !url.is_empty() => url.clone(),
            _ => {
                Self::update_run_failed(state, run_id, "No endpoint_url configured").await;
                return;
            }
        };

        let payload = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": format!("[cron] Scheduled execution of workflow '{}'", wf.slug)
            }],
            "stream": false,
        });

        let result = state
            .http_client
            .post(&endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await;

        let completed_at = Utc::now().naive_utc();

        match result {
            Ok(resp) => {
                let status_code = resp.status();
                let body = resp.text().await.unwrap_or_default();

                if status_code.is_success() {
                    let result_json = serde_json::from_str::<serde_json::Value>(&body)
                        .unwrap_or(serde_json::json!({"raw": body}));
                    Self::update_run_success(state, run_id, completed_at, result_json).await;
                    tracing::info!(
                        "✅ [Scheduler] Workflow '{}' run {} completed ({})",
                        wf.slug,
                        run_id,
                        status_code
                    );
                } else {
                    Self::update_run_failed_with_time(
                        state,
                        run_id,
                        completed_at,
                        &format!("HTTP {}: {}", status_code, &body[..body.len().min(500)]),
                    )
                    .await;
                    tracing::warn!(
                        "❌ [Scheduler] Workflow '{}' run {} failed: HTTP {}",
                        wf.slug,
                        run_id,
                        status_code
                    );
                }
            }
            Err(e) => {
                Self::update_run_failed_with_time(
                    state,
                    run_id,
                    completed_at,
                    &format!("Request error: {}", e),
                )
                .await;
                tracing::error!(
                    "❌ [Scheduler] Workflow '{}' run {} error: {}",
                    wf.slug,
                    run_id,
                    e
                );
            }
        }
    }

    async fn update_run_success(
        state: &AppState,
        run_id: Uuid,
        completed_at: chrono::NaiveDateTime,
        result: serde_json::Value,
    ) {
        let update = workflow_runs::ActiveModel {
            id: Set(run_id),
            status: Set("success".to_string()),
            completed_at: Set(Some(completed_at)),
            result: Set(Some(result)),
            updated_at: Set(Some(completed_at)),
            ..Default::default()
        };
        let _ = update.update(&state.db).await;
    }

    async fn update_run_failed(state: &AppState, run_id: Uuid, error: &str) {
        let now = Utc::now().naive_utc();
        Self::update_run_failed_with_time(state, run_id, now, error).await;
    }

    async fn update_run_failed_with_time(
        state: &AppState,
        run_id: Uuid,
        completed_at: chrono::NaiveDateTime,
        error: &str,
    ) {
        let update = workflow_runs::ActiveModel {
            id: Set(run_id),
            status: Set("failed".to_string()),
            completed_at: Set(Some(completed_at)),
            error: Set(Some(error.to_string())),
            updated_at: Set(Some(completed_at)),
            ..Default::default()
        };
        let _ = update.update(&state.db).await;
    }
}
