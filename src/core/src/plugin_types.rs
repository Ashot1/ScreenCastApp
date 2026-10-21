//! User-installed plugins (ADR-021, ADR-022): remote and local MCP servers whose approved
//! read-only tools the model may call inside one assistance request. Packages, pins and results
//! are untrusted data.
use serde::{Deserialize, Serialize};

pub const MAX_PLUGINS: usize = 16;
pub const MAX_PLUGIN_SERVERS: usize = 4;
pub const MAX_LISTED_TOOLS: usize = 64;
pub const MAX_PLUGIN_KEYS: usize = 64;
pub const MAX_PLUGIN_FILES: usize = 64;
pub const MAX_PLUGIN_FILE: usize = 256 * 1024;
pub const MAX_PLUGIN_FILES_TOTAL: usize = 1024 * 1024;
pub const MAX_TOOL_DEFINITION: usize = 16 * 1024;
pub const MAX_TOOL_ARGUMENTS: usize = 2 * 1024;
pub const PLUGIN_NAMESPACE: &str = "plugin";
/// Spawn and `initialize` of a local server on its first call in a request, beyond the 20 s call.
pub const LOCAL_START: std::time::Duration = std::time::Duration::from_secs(30);
/// The call log keeps this many newest calls.
pub const MAX_CALL_LOG: usize = 256;
/// Per-request budgets of the tool loop (ADR-021).
pub const MAX_OFFERED_DEFINITIONS: usize = 64 * 1024;
pub const MAX_OFFERED_SKILLS: usize = 64 * 1024;
pub const MAX_LOOP_RESULTS: usize = 96 * 1024;
pub const MAX_FORWARDED_ITEMS: usize = 256 * 1024;
/// Output items kept from one model response; providers enforce it while decoding.
pub const MAX_RESPONSE_ITEMS: usize = 64 * 1024;
/// A request with tools may grow by every budget above over the plain limit.
pub const MAX_TOOLS_BODY: usize = crate::MAX_ASSISTANCE_BODY
    + MAX_OFFERED_DEFINITIONS
    + MAX_OFFERED_SKILLS
    + MAX_LOOP_RESULTS
    + MAX_FORWARDED_ITEMS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum PluginFileKind {
    Manifest = 1,
    Mcp = 2,
    Skill = 3,
}
/// One package file read by native; Core parses and validates everything in it.
#[derive(Clone, Copy, Debug)]
pub struct PluginFile<'a> {
    pub kind: PluginFileKind,
    pub path: &'a str,
    pub bytes: &'a [u8],
}
/// The immutable validated package stored at install (≤256 KiB JSON).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPackage {
    pub servers: Vec<PluginServerSpec>,
    pub skills: Vec<PluginSkill>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginServerSpec {
    pub key: String,
    pub kind: ServerKind,
}
/// How a server is reached. The explicit tag makes a stored package of the old flat shape
/// unreadable (`UnsupportedSchema`) instead of silently reinterpreted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerKind {
    Remote {
        url: String,
        auth_required: bool,
    },
    /// A command on this Mac (ADR-022). `env` holds variable names only: their values come from
    /// the user's Keychain secret, never from the package.
    Local {
        command: String,
        args: Vec<String>,
        env: Vec<String>,
    },
    /// The server Ambient installs and runs itself (D71); only the built-in package names it.
    Managed {
        id: ManagedServerId,
    },
}
impl ServerKind {
    /// Whether calls need a secret: sign-in for a remote server, variable values for a local one.
    pub fn needs_key(&self) -> bool {
        match self {
            Self::Remote { auth_required, .. } => *auth_required,
            Self::Local { env, .. } => !env.is_empty(),
            Self::Managed { .. } => true,
        }
    }
    /// Commands that download code on every start (D70): the review warns about them.
    pub fn downloads_code(&self) -> bool {
        let Self::Local { command, args, .. } = self else {
            return false;
        };
        let program = command.rsplit('/').next().unwrap_or(command);
        let first = args.first().map(String::as_str);
        match program {
            "npx" | "bunx" | "uvx" => true,
            "pnpm" | "yarn" => first == Some("dlx"),
            "pipx" => first == Some("run"),
            _ => false,
        }
    }
    /// What a call or discovery contacts; providers never read the package.
    pub fn target(&self) -> Target {
        match self {
            Self::Remote { url, .. } => Target::Remote { url: url.clone() },
            Self::Local { command, args, env } => Target::Local {
                command: command.clone(),
                args: args.clone(),
                env_names: env.clone(),
            },
            Self::Managed { id } => Target::Managed(*id),
        }
    }
}
/// The managed servers of this release (ADR-022). Every match on it is exhaustive, so a new id
/// does not compile until the Core allowlist, the providers recipe and the OAuth registry name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManagedServerId {
    #[serde(rename = "google-workspace")]
    GoogleWorkspace,
}
impl ManagedServerId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoogleWorkspace => "google-workspace",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "google-workspace" => Some(Self::GoogleWorkspace),
            _ => None,
        }
    }
    /// The built-in package that alone may name this server; «Add Google» reviews these bytes.
    pub fn builtin_package(self) -> &'static [PluginFile<'static>] {
        match self {
            Self::GoogleWorkspace => &GOOGLE_PACKAGE,
        }
    }
    /// The package hash of exactly those bytes; a test recomputes it, so a release whose bytes
    /// and constant disagree fails loudly instead of silently skipping the server.
    pub fn builtin_package_hash(self) -> [u8; 32] {
        match self {
            Self::GoogleWorkspace => GOOGLE_PACKAGE_HASH,
        }
    }
    /// The Core allowlist of tool names, checked at approval, discovery and every permit: a
    /// server's read-only mark alone is not enough (one listed tool writes a local file).
    pub fn allows(self, tool: &str) -> bool {
        match self {
            Self::GoogleWorkspace => matches!(
                tool,
                "search_drive_files"
                    | "get_drive_file_content"
                    | "list_drive_items"
                    | "search_docs"
                    | "get_doc_content"
                    | "get_doc_as_markdown"
                    | "list_docs_in_folder"
                    | "list_document_comments"
            ),
        }
    }
}
static GOOGLE_PACKAGE: [PluginFile<'static>; 2] = [
    PluginFile {
        kind: PluginFileKind::Manifest,
        path: "plugin.json",
        bytes: include_bytes!("../builtin/google-workspace/plugin.json"),
    },
    PluginFile {
        kind: PluginFileKind::Mcp,
        path: "mcp.json",
        bytes: include_bytes!("../builtin/google-workspace/mcp.json"),
    },
];
/// SHA-256 package hash (ADR-021 rule) of `GOOGLE_PACKAGE`.
const GOOGLE_PACKAGE_HASH: [u8; 32] = [
    0xfd, 0xfb, 0xa3, 0x4d, 0x58, 0x79, 0x3f, 0x5f, 0xee, 0xe0, 0x99, 0x04, 0x16, 0xc0, 0xaa, 0xc7,
    0x6b, 0x83, 0xff, 0x38, 0x87, 0xbc, 0x6b, 0x5f, 0x6e, 0x26, 0xde, 0x95, 0xc0, 0x34, 0xc4, 0xd6,
];
/// The server a permit or a discovery may contact. Providers build the private folder of a local
/// server as `<root>/<plugin id>`; Core knows no file layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Remote {
        url: String,
    },
    Local {
        command: String,
        args: Vec<String>,
        env_names: Vec<String>,
    },
    /// Providers build `http://127.0.0.1:<port>/mcp` after choosing the port; no URL is stored.
    Managed(ManagedServerId),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSkill {
    pub name: String,
    pub description: String,
    pub text: String,
}
/// Stateless review shown before install; `hash` is the lowercase hex package hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PluginReview {
    pub name: String,
    pub version: String,
    pub hash: String,
    pub servers: Vec<PluginServerSpec>,
    pub skills: Vec<PluginSkill>,
    pub warnings: Vec<String>,
}

/// Canonical MCP tool definition `{name, description, inputSchema, readOnlyHint}` (sorted keys,
/// absent members omitted) and its SHA-256. Only declared read-only tools ≤16 KiB are approvable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalTool {
    pub name: String,
    pub definition: String,
    pub hash: [u8; 32],
    pub read_only: bool,
    pub approvable: bool,
}
/// The identity of a plugin key passed to dispatch; the secret itself never enters Core.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginKeyRef {
    pub plugin: u64,
    pub server: String,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum PluginCallOutcome {
    Completed = 1,
    ToolError = 2,
    Changed = 3,
    LoginRequired = 4,
    Unavailable = 5,
    Invalid = 6,
    Timeout = 7,
    Cancelled = 8,
    Interrupted = 9,
    /// A local server's variable values are missing (ADR-022 «не настроено»).
    NotConfigured = 10,
    /// Written by Core after the request: the server's process group was not confirmed gone.
    StopUnconfirmed = 11,
}
/// One stateless discovery. `stderr` is the redacted tail of a failed local server (shown only
/// to the user); `stop_confirmed` is false when its process group was not confirmed gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    pub listed: Result<Vec<serde_json::Value>, PluginCallOutcome>,
    pub stderr: Option<String>,
    pub stop_confirmed: bool,
}

/// One item of the request history after the user context: model output returned unchanged or
/// a Core-built result envelope for one call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryItem {
    Forwarded(serde_json::Value),
    Output { call_id: String, output: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepMode {
    /// No plugin tool was offered: the exact pre-plugin request.
    Plain,
    /// Tools offered in this POST.
    Tools,
    /// Tools were offered earlier in the request; this POST has none and must answer.
    Final,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepFunction {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepSkill {
    pub plugin: String,
    pub name: String,
    pub description: String,
    pub text: String,
}
/// One admitted POST whose possible-send row is committed; borrowed from its loop.
pub struct ModelStep<'a> {
    pub id: crate::AttemptId,
    pub mode: StepMode,
    pub context: &'a str,
    pub functions: &'a [StepFunction],
    pub skills: &'a [StepSkill],
    pub history: &'a [HistoryItem],
    pub timeout: std::time::Duration,
    pub cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
pub enum StepNext {
    Done(Option<crate::WorkspaceVersion>),
    Tools,
}
/// One call admitted by Core; its stage-1 call-log row is committed.
pub struct ToolCallPermit {
    pub target: Target,
    pub plugin: u64,
    pub server: String,
    pub key: Option<PluginKeyRef>,
    pub tool: String,
    pub arguments: String,
    pub list_first: bool,
    pub pinned: [u8; 32],
    pub timeout: std::time::Duration,
    pub cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) tool_index: usize,
    pub(crate) call_id: String,
    pub(crate) generation: u64,
    pub(crate) log: i64,
    pub(crate) at_utc: i64,
}
/// The transport outcome of one call. `text` is the bounded read; `listed` holds the canonical
/// hashes of the server's live tools when this call listed them first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCallResult {
    pub outcome: PluginCallOutcome,
    pub text: String,
    pub listed: Option<std::collections::BTreeMap<String, [u8; 32]>>,
}
/// One model call in this request; `citable` was decided at receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ToolResult {
    pub tool: usize,
    pub arguments: String,
    pub called_at_utc: i64,
    pub text: String,
    pub complete: bool,
    pub citable: bool,
}
/// Loop state the bridge owns between Store locks. It holds the admission by value; every POST
/// borrows it, so the admission is never cloned (ADR-021).
pub struct ModelLoop {
    pub(crate) admission: crate::CheckAdmission,
    pub(crate) request: u64,
    pub(crate) context: String,
    pub(crate) started: std::time::Instant,
    pub(crate) keys: std::collections::BTreeSet<PluginKeyRef>,
    pub(crate) functions: Vec<StepFunction>,
    pub(crate) skills: Vec<StepSkill>,
    pub(crate) history: Vec<HistoryItem>,
    pub(crate) posts: u32,
    pub(crate) mode: StepMode,
    pub(crate) last: Option<crate::AttemptId>,
    pub(crate) pending: std::collections::VecDeque<crate::ToolCall>,
    pub(crate) results: Vec<ToolResult>,
    pub(crate) listed:
        std::collections::BTreeMap<(u64, String), std::collections::BTreeMap<String, [u8; 32]>>,
    pub(crate) results_bytes: usize,
    pub(crate) forwarded_bytes: usize,
    pub(crate) done: bool,
}
impl ModelLoop {
    pub fn cancellation(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.admission.cancellation()
    }
}
