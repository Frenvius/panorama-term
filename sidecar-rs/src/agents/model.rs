use serde::{Deserialize, Serialize};

pub const NAME_MAX: usize = 32;
pub const TITLE_MAX_CHARS: usize = 60;
pub const DESCRIPTION_MAX_CHARS: usize = 280;
pub const INSTRUCTIONS_MAX_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Desired {
    Running,
    #[default]
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSession {
    pub agent: String,
    pub id: String,
    #[serde(default)]
    pub bound_at: u64,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exit {
    pub at: u64,
    pub reason: String,
    #[serde(default)]
    pub code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub profile: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    pub cwd: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub instructions: String,
    #[serde(default = "default_true")]
    pub auto_start: bool,
    #[serde(default)]
    pub desired: Desired,
    #[serde(default)]
    pub native_session: Option<NativeSession>,
    #[serde(default)]
    pub last_exit: Option<Exit>,
    #[serde(default)]
    pub restarts: Vec<u64>,
    #[serde(default)]
    pub crashed: bool,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub last_started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_by: Option<String>,
    #[serde(default)]
    pub rollover: RolloverState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rollover {
    pub n: u32,
    pub reason: String,
    pub phase: String,
    pub started_at: u64,
    #[serde(default)]
    pub note: bool,
    #[serde(default)]
    pub handoff: bool,
    #[serde(default)]
    pub noted: bool,
    #[serde(default)]
    pub auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx_before: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloverEnd {
    pub n: u32,
    pub outcome: String,
    pub at: u64,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub handoff: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx_before: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx_after: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloverState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<Rollover>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<String>,
    #[serde(default)]
    pub history: Vec<(u64, bool)>,
    #[serde(default)]
    pub strikes: u8,
    #[serde(default)]
    pub seq: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<RolloverEnd>,
}
impl Agent {
    pub fn tile_id(&self) -> String {
        tile_id(&self.id)
    }
}

pub fn tile_id(agent_id: &str) -> String {
    format!("agent-{agent_id}")
}

pub fn agent_id_from_tile(tile_id: &str) -> Option<&str> {
    tile_id.strip_prefix("agent-").filter(|id| !id.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveStatus {
    Starting,
    Idle,
    Busy,
    Waiting,
    Crashed,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatusSource {
    Hook,
    Quiet,
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLive {
    pub tile_id: String,
    pub status: LiveStatus,
    pub live: bool,
    pub status_source: StatusSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_pct: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}
