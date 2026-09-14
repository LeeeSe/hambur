//! Tool layer shared by the runtime: registry of tool specs, invocation types, result
//! normalisation, and a scheduler for builtin tools.

mod invocation;
mod normalizer;
mod scheduler;
mod spec;

pub use invocation::{RawToolOutput, ToolCallBatch, ToolExecutionRecord, ToolInvocation, ToolResult};
pub use normalizer::{DEFAULT_LARGE_RESULT_THRESHOLD_BYTES, ToolResultNormalizer};
pub use scheduler::{MAX_PARALLEL_TOOL_CALLS, ToolScheduler};
pub use spec::{RiskLevel, ToolAudience, ToolHost, ToolKind, ToolRegistry, ToolSpec};

pub const MAX_TOOL_ITERATIONS_PER_TURN: u32 = 64;
