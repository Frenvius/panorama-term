use serde::{Deserialize, Serialize};
use crate::orch::candidates::{CandidateKey, CandidateStatus};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Explore,
    Implement,
    Test,
    Review,
    Mechanical,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Explore => "explore",
            Role::Implement => "implement",
            Role::Test => "test",
            Role::Review => "review",
            Role::Mechanical => "mechanical",
        }
    }

    pub fn all() -> &'static [&'static str] {
        &["explore", "implement", "test", "review", "mechanical"]
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn default_verify_timeout_min() -> u32 { 15 }
fn default_stall_quiet_min() -> u32 { 5 }
fn default_max_operational_attempts() -> u32 { 6 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_workers: u32,
    pub max_attempts: u32,
    pub dispatch_timeout_min: u32,
    #[serde(default = "default_verify_timeout_min")]
    pub verify_timeout_min: u32,
    #[serde(default = "default_stall_quiet_min")]
    pub stall_quiet_min: u32,
    #[serde(default = "default_max_operational_attempts")]
    pub max_operational_attempts: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_workers: 4,
            max_attempts: 3,
            dispatch_timeout_min: 60,
            verify_timeout_min: 15,
            stall_quiet_min: 5,
            max_operational_attempts: 6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Active,
    Paused,
    Completed,
    Cancelled,
}

impl RunStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, RunStatus::Completed | RunStatus::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Autonomy {
    #[serde(default)]
    pub operational: bool,
    #[serde(default)]
    pub execution: bool,
    #[serde(default)]
    pub integration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorWait {
    #[serde(default)]
    pub reset_at: Option<i64>,
    pub since: i64,
    #[serde(default)]
    pub reason: String,
    pub resume_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    QuotaExhausted,
    RateLimited,
    AuthFailed,
    ModelUnavailable,
    LaunchFailed,
    ExecutionFailed,
    SetupFailed,
    QualityFailed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCounter {
    Operational,
    Quality,
    None,
}

impl FailureClass {
    pub fn counter(self) -> FailureCounter {
        match self {
            FailureClass::QuotaExhausted
            | FailureClass::RateLimited
            | FailureClass::AuthFailed
            | FailureClass::ModelUnavailable
            | FailureClass::LaunchFailed
            | FailureClass::ExecutionFailed => FailureCounter::Operational,
            FailureClass::SetupFailed => FailureCounter::None,
            FailureClass::QualityFailed | FailureClass::Unknown => FailureCounter::Quality,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Initial,
    Fallback,
    Escalation,
    Resume,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchBy {
    Coordinator,
    Brain,
    User,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaitKind {
    Quota,
    Availability,
    Intervention,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskWait {
    pub kind: WaitKind,
    #[serde(default)]
    pub resume_at: Option<i64>,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub candidate: Option<CandidateKey>,
    #[serde(default)]
    pub same_session: bool,
    pub resume_id: String,
    pub since: i64,
}

pub fn resume_id(task_id: &str, attempts: u32, resume_at: Option<i64>) -> String {
    match resume_at {
        Some(at) => format!("{task_id}:{attempts}:{at}"),
        None => format!("{task_id}:{attempts}:none"),
    }
}

pub fn coordinator_resume_id(run_id: &str, reset_at: Option<i64>) -> String {
    match reset_at {
        Some(at) => format!("{run_id}:coordinator:{at}"),
        None => format!("{run_id}:coordinator:none"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkippedCandidate {
    pub candidate: CandidateKey,
    pub state: CandidateStatus,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoutingDecision {
    #[serde(default)]
    pub requested: Option<CandidateKey>,
    pub effective: CandidateKey,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub skipped: Vec<SkippedCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub title: String,
    pub repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator_tile: Option<String>,
    pub status: RunStatus,
    pub limits: Limits,
    #[serde(default)]
    pub autonomy: Autonomy,
    #[serde(default)]
    pub detached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator_wait: Option<CoordinatorWait>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Pending,
    Dispatched,
    Verifying,
    Review,
    Completed,
    Failed,
    Blocked,
    Cancelled,
    Waiting,
}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Completed | TaskStatus::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: String,
    pub spec: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
    pub deps: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub escalation: Vec<String>,
    pub verify: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub review: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_reason: Option<String>,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrated_sha: Option<String>,
    pub attempts: u32,
    #[serde(default)]
    pub routing: u32,
    #[serde(default)]
    pub quality_floor: u32,
    #[serde(default)]
    pub quality_attempts: u32,
    #[serde(default)]
    pub operational_attempts: u32,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<FailureClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<TaskWait>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    pub timed_out: bool,
    pub tail: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    pub at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum DispatchStatus {
    Running,
    Reported,
    Failed,
    Stopped,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dispatch {
    pub id: String,
    pub task_id: String,
    pub attempt: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_of: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub status: DispatchStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    pub files_modified: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    pub started_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verifications: Vec<VerificationResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<Trigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<RoutingDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<FailureClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<DispatchBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ref: Option<String>,
}

impl Dispatch {
    pub fn accepts_reports(&self) -> bool {
        self.status == DispatchStatus::Running
            || (self.status == DispatchStatus::Failed && self.outcome.is_none())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub seq: u32,
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub at: u64,
    pub acked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gate {
    pub id: String,
    pub task_id: String,
    pub question: String,
    pub options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<u64>,
}
