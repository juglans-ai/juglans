// src/runtime/python/worker.rs
//
// Python worker process management

use super::protocol::{PythonRequest, PythonResponse};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_request_id() -> String {
    let id = REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("req-{:06}", id)
}

/// A single Python worker process
pub struct PythonWorker {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    worker_id: u32,
    refs_held: Vec<String>,
}

impl PythonWorker {
    /// Spawn a new Python worker process
    pub fn spawn(worker_id: u32) -> Result<Self> {
        // Find the worker script path relative to the executable
        let worker_script = Self::find_worker_script()?;

        debug!("Spawning Python worker {} with script: {:?}", worker_id, worker_script);

        let mut process = Command::new("python3")
            .arg(&worker_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // Let Python errors go to stderr
            .spawn()
            .with_context(|| format!("Failed to spawn Python worker {}", worker_id))?;

        let stdin = process.stdin.take().ok_or_else(|| anyhow!("Failed to get stdin"))?;
        let stdout = process.stdout.take().ok_or_else(|| anyhow!("Failed to get stdout"))?;

        let mut worker = Self {
            process,
            stdin,
            stdout: BufReader::new(stdout),
            worker_id,
            refs_held: Vec::new(),
        };

        // Verify worker is alive with a ping
        let ping_resp = worker.send_request(&PythonRequest::ping(next_request_id()))?;
        if ping_resp.is_error() {
            return Err(anyhow!("Worker {} failed health check", worker_id));
        }

        info!("Python worker {} ready", worker_id);
        Ok(worker)
    }

    /// Find the worker script path
    fn find_worker_script() -> Result<std::path::PathBuf> {
        // Try multiple locations
        let mut candidates: Vec<std::path::PathBuf> = vec![
            // Relative to current directory
            "src/workers/python_worker.py".into(),
            // Relative to executable
            "../src/workers/python_worker.py".into(),
        ];

        // Try ~/.juglans/workers/python_worker.py
        if let Some(home) = std::env::var_os("HOME") {
            let home_path: std::path::PathBuf = home.into();
            candidates.push(home_path.join(".juglans/workers/python_worker.py"));
        }

        // Try relative to the executable's directory
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                candidates.push(exe_dir.join("python_worker.py"));
                candidates.push(exe_dir.join("workers/python_worker.py"));
            }
        }

        for candidate in &candidates {
            if candidate.exists() {
                return Ok(candidate.clone());
            }
        }

        // As a fallback, embed the worker script and write it to a temp file
        Err(anyhow!(
            "Python worker script not found. Searched: {:?}",
            candidates
        ))
    }

    /// Send a request and wait for response
    pub fn send_request(&mut self, request: &PythonRequest) -> Result<PythonResponse> {
        let json = serde_json::to_string(request)?;
        debug!("[Worker {}] <- {}", self.worker_id, json);

        writeln!(self.stdin, "{}", json)?;
        self.stdin.flush()?;

        let mut response_line = String::new();
        self.stdout.read_line(&mut response_line)?;

        debug!("[Worker {}] -> {}", self.worker_id, response_line.trim());

        let response: PythonResponse = serde_json::from_str(&response_line)
            .with_context(|| format!("Failed to parse worker response: {}", response_line))?;

        // Track any new references
        if let Some(ref_id) = &response.reference {
            self.refs_held.push(ref_id.clone());
        }

        Ok(response)
    }

    /// Call a function on a module or reference
    pub fn call(
        &mut self,
        target: &str,
        method: &str,
        args: Vec<serde_json::Value>,
        kwargs: HashMap<String, serde_json::Value>,
    ) -> Result<PythonResponse> {
        let request = PythonRequest::call(next_request_id(), target, method, args, kwargs);
        self.send_request(&request)
    }

    /// Check if the worker process is still alive
    pub fn is_alive(&mut self) -> bool {
        match self.process.try_wait() {
            Ok(None) => true,        // Still running
            Ok(Some(_)) => false,    // Exited
            Err(_) => false,         // Error checking
        }
    }

    /// Get the worker ID
    pub fn id(&self) -> u32 {
        self.worker_id
    }

    /// Release all held references
    pub fn release_refs(&mut self) -> Result<()> {
        if self.refs_held.is_empty() {
            return Ok(());
        }

        let refs = std::mem::take(&mut self.refs_held);
        let request = PythonRequest::Del {
            id: next_request_id(),
            refs,
        };
        let _ = self.send_request(&request)?;
        Ok(())
    }
}

impl Drop for PythonWorker {
    fn drop(&mut self) {
        // Try to cleanly release refs before killing
        let _ = self.release_refs();

        // Kill the process
        if let Err(e) = self.process.kill() {
            warn!("Failed to kill Python worker {}: {}", self.worker_id, e);
        }
    }
}

/// A pool of Python workers for parallel execution
pub struct PythonWorkerPool {
    workers: Vec<Arc<Mutex<PythonWorker>>>,
    next_worker: AtomicU64,
    max_workers: usize,
}

impl PythonWorkerPool {
    /// Create a new worker pool
    pub fn new(max_workers: usize) -> Result<Self> {
        let mut workers = Vec::with_capacity(max_workers);

        // Start with one worker, scale up as needed
        let worker = PythonWorker::spawn(0)?;
        workers.push(Arc::new(Mutex::new(worker)));

        Ok(Self {
            workers,
            next_worker: AtomicU64::new(0),
            max_workers,
        })
    }

    /// Get a worker for executing a request
    pub fn get_worker(&self) -> Arc<Mutex<PythonWorker>> {
        let idx = self.next_worker.fetch_add(1, Ordering::SeqCst) as usize;
        let worker_idx = idx % self.workers.len();
        Arc::clone(&self.workers[worker_idx])
    }

    /// Execute a call on any available worker
    pub fn call(
        &self,
        target: &str,
        method: &str,
        args: Vec<serde_json::Value>,
        kwargs: HashMap<String, serde_json::Value>,
    ) -> Result<PythonResponse> {
        let worker_lock = self.get_worker();
        let mut worker = worker_lock.lock().map_err(|e| anyhow!("Worker lock poisoned: {}", e))?;

        // Check if worker is still alive
        if !worker.is_alive() {
            error!("Worker {} died, restarting...", worker.id());
            // For now, just return an error; proper restart logic would be more complex
            return Err(anyhow!("Python worker died unexpectedly"));
        }

        worker.call(target, method, args, kwargs)
    }

    /// Get the number of active workers
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires Python worker script to be present
    fn test_worker_spawn() {
        let worker = PythonWorker::spawn(0);
        assert!(worker.is_ok());
    }

    #[test]
    #[ignore] // Requires Python worker script to be present
    fn test_worker_call() {
        let mut worker = PythonWorker::spawn(0).unwrap();
        let resp = worker.call("json", "dumps", vec![serde_json::json!({"a": 1})], HashMap::new());
        assert!(resp.is_ok());
        let resp = resp.unwrap();
        assert!(!resp.is_error());
        assert_eq!(resp.value, Some(serde_json::json!("{\"a\": 1}")));
    }
}
