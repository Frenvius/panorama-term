use std::time::Duration;

pub(crate) struct RunToExitInput {
    pub cwd: String,
    pub cmd: String,
    pub timeout: Duration,
    pub dispatch_id: String,
    pub purpose: String,
}

pub(crate) struct RunOutcome {
    pub exit_code: Option<i64>,
    pub timed_out: bool,
    pub tail: Vec<String>,
}

pub(crate) trait ExecBackend: Send + Sync {
    fn start(&self, tile: &str, cwd: &str, cmd: &str) -> Result<(), String>;
    fn is_exited(&self, tile: &str) -> Option<i64>;
    fn stop_tile(&self, tile: &str);
    fn tail_lines(&self, tile: &str, n: usize) -> Vec<String>;
}

pub(crate) async fn run_to_exit(input: RunToExitInput) -> Result<RunOutcome, String> {
    run_to_exit_with(input, &LiveRunner).await
}

pub(crate) async fn run_to_exit_with(input: RunToExitInput, backend: &dyn ExecBackend) -> Result<RunOutcome, String> {
    let tile = format!("orch-{}-{}", input.purpose, input.dispatch_id);
    backend.start(&tile, &input.cwd, &input.cmd)?;

    let deadline = tokio::time::Instant::now() + input.timeout;
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Some(code) = backend.is_exited(&tile) {
            let tail = backend.tail_lines(&tile, 50);
            return Ok(RunOutcome { exit_code: Some(code), timed_out: false, tail });
        }
        if tokio::time::Instant::now() >= deadline {
            backend.stop_tile(&tile);
            tokio::time::sleep(Duration::from_secs(3)).await;
            if backend.is_exited(&tile).is_none() {
                backend.stop_tile(&tile);
            }
            let tail = backend.tail_lines(&tile, 50);
            return Ok(RunOutcome { exit_code: None, timed_out: true, tail });
        }
    }
}

struct LiveRunner;

impl ExecBackend for LiveRunner {
    fn start(&self, tile: &str, cwd: &str, cmd: &str) -> Result<(), String> {
        let tile = tile.to_string();
        let cwd = cwd.to_string();
        let cmd = cmd.to_string();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let res = crate::run_start_json("build", &tile, &cwd, &cmd).await;
                let v: serde_json::Value = serde_json::from_str(&res).unwrap_or_default();
                if let Some(e) = v.get("error").and_then(|v| v.as_str()) {
                    return Err(e.to_string());
                }
                Ok(())
            })
        })
    }

    fn is_exited(&self, tile: &str) -> Option<i64> {
        let status = crate::run_status_json(crate::run_lookup_tile("build", tile));
        let v: serde_json::Value = serde_json::from_str(&status).ok()?;
        if v["state"].as_str() == Some("exited") {
            Some(v["exitCode"].as_i64().unwrap_or(1))
        } else {
            None
        }
    }

    fn stop_tile(&self, tile: &str) {
        if let Some(s) = crate::run_lookup_tile("build", tile) {
            crate::run_stop(&s);
        }
    }

    fn tail_lines(&self, tile: &str, n: usize) -> Vec<String> {
        crate::run_tail_lines("build", tile, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeRunner {
        exit_code: Option<i64>,
        exit_after_polls: usize,
        poll_count: AtomicUsize,
    }

    impl FakeRunner {
        fn exits_after(code: i64, polls: usize) -> Self {
            Self { exit_code: Some(code), exit_after_polls: polls, poll_count: AtomicUsize::new(0) }
        }

        fn never_exits() -> Self {
            Self { exit_code: None, exit_after_polls: usize::MAX, poll_count: AtomicUsize::new(0) }
        }
    }

    impl ExecBackend for FakeRunner {
        fn start(&self, _tile: &str, _cwd: &str, _cmd: &str) -> Result<(), String> {
            Ok(())
        }
        fn is_exited(&self, _tile: &str) -> Option<i64> {
            let n = self.poll_count.fetch_add(1, Ordering::Relaxed);
            if n >= self.exit_after_polls { self.exit_code } else { None }
        }
        fn stop_tile(&self, _tile: &str) {}
        fn tail_lines(&self, _tile: &str, _n: usize) -> Vec<String> { vec![] }
    }

    fn input(purpose: &str, timeout: Duration) -> RunToExitInput {
        RunToExitInput {
            cwd: "/tmp".into(),
            cmd: "true".into(),
            timeout,
            dispatch_id: "d1".into(),
            purpose: purpose.into(),
        }
    }

    #[tokio::test]
    async fn run_to_exit_reports_code_0() {
        tokio::time::pause();
        let runner = FakeRunner::exits_after(0, 1);
        let out = run_to_exit_with(input("setup", Duration::from_secs(10)), &runner).await.unwrap();
        assert_eq!(out.exit_code, Some(0));
        assert!(!out.timed_out);
    }

    #[tokio::test]
    async fn run_to_exit_reports_code_3() {
        tokio::time::pause();
        let runner = FakeRunner::exits_after(3, 1);
        let out = run_to_exit_with(input("setup", Duration::from_secs(10)), &runner).await.unwrap();
        assert_eq!(out.exit_code, Some(3));
        assert!(!out.timed_out);
    }

    #[tokio::test]
    async fn run_to_exit_timeout_sets_timed_out() {
        tokio::time::pause();
        let runner = FakeRunner::never_exits();
        let out = run_to_exit_with(input("setup", Duration::from_millis(300)), &runner).await.unwrap();
        assert!(out.timed_out);
        assert_eq!(out.exit_code, None);
    }
}
