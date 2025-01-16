#![allow(clippy::to_string_trait_impl)]

use std::convert::Infallible;
use std::str::FromStr;

use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::errors::DeserializationError;
use crate::{fromstr_deser, tostr_ser};
use std::num::NonZeroUsize;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExceptionBreakpointsFilter {
    /// The internal ID of the filter option. This value is passed to the
    /// `setExceptionBreakpoints` request.
    pub filter: String,
    /// The name of the filter option. This is shown in the UI.
    pub label: String,
    /// A help text providing additional information about the exception filter.
    /// This string is typically shown as a hover and can be translated.
    pub description: Option<String>,
    /// Initial value of the filter option. If not specified a value false is
    /// assumed.
    pub default: Option<bool>,
    /// Controls whether a condition can be specified for this filter option. If
    /// false or missing, a condition can not be set.
    pub supports_condition: Option<bool>,
    /// A help text providing information about the condition. This string is shown
    /// as the placeholder text for a text box and can be translated.
    pub condition_description: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ColumnDescriptorType {
    String,
    Number,
    Bool,
    UnixTimestampUTC,
}

impl FromStr for ColumnDescriptorType {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "string" => Ok(ColumnDescriptorType::String),
            "number" => Ok(ColumnDescriptorType::Number),
            "bool" => Ok(ColumnDescriptorType::Bool),
            "unixTimestampUTC" => Ok(ColumnDescriptorType::UnixTimestampUTC),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "ColumnDescriptorType".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

fromstr_deser! { ColumnDescriptorType }

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDescriptor {
    /// Name of the attribute rendered in this column.
    pub attribute_name: String,
    /// Header UI label of column.
    pub label: String,
    /// Format to use for the rendered values in this column. TBD how the format
    /// strings looks like.
    pub format: String,
    /// Datatype of values in this column. Defaults to `string` if not specified.
    /// Values: 'string', 'number', 'bool', 'unixTimestampUTC'
    #[serde(rename = "type")]
    pub column_descriptor_type: Option<ColumnDescriptorType>,
    /// Width of this column in characters (hint only).
    pub width: Option<usize>,
}

#[derive(Serialize, Debug, Clone)]
pub enum ChecksumAlgorithm {
    MD5,
    SHA1,
    SHA256,
    #[serde(rename = "timestamp")]
    Timestamp,
}

impl FromStr for ChecksumAlgorithm {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "MD5" => Ok(ChecksumAlgorithm::MD5),
            "SHA1" => Ok(ChecksumAlgorithm::SHA1),
            "SHA256" => Ok(ChecksumAlgorithm::SHA256),
            "timestamp" => Ok(ChecksumAlgorithm::Timestamp),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "ChecksumAlgorithm".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

fromstr_deser! {ChecksumAlgorithm}

#[derive(Serialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// The debug adapter supports the `configurationDone` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_configuration_done_request: Option<bool>,
    /// The debug adapter supports function breakpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_function_breakpoints: Option<bool>,
    /// The debug adapter supports conditional breakpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_conditional_breakpoints: Option<bool>,
    /// The debug adapter supports breakpoints that break execution after a
    /// specified number of hits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_hit_conditional_breakpoints: Option<bool>,
    /// The debug adapter supports a (side effect free) `evaluate` request for data
    /// hovers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_evaluate_for_hovers: Option<bool>,
    /// Available exception filter options for the `setExceptionBreakpoints`
    /// request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception_breakpoint_filters: Option<Vec<ExceptionBreakpointsFilter>>,
    /// The debug adapter supports stepping back via the `stepBack` and
    /// `reverseContinue` requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_step_back: Option<bool>,
    /// The debug adapter supports setting a variable to a value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_set_variable: Option<bool>,
    /// The debug adapter supports restarting a frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_restart_frame: Option<bool>,
    /// The debug adapter supports the `gotoTargets` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_goto_targets_request: Option<bool>,
    /// The debug adapter supports the `stepInTargets` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_step_in_targets_request: Option<bool>,
    /// The debug adapter supports the `completions` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_completions_request: Option<bool>,
    /// The set of characters that should trigger completion in a REPL. If not
    /// specified, the UI should assume the `.` character.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_trigger_characters: Option<Vec<String>>,
    /// The debug adapter supports the `modules` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_modules_request: Option<bool>,
    /// The set of additional module information exposed by the debug adapter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_module_columns: Option<Vec<ColumnDescriptor>>,
    /// Checksum algorithms supported by the debug adapter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_checksum_algorithms: Option<Vec<ChecksumAlgorithm>>,
    /// The debug adapter supports the `restart` request. In this case a client
    /// should not implement `restart` by terminating and relaunching the adapter
    /// but by calling the `restart` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_restart_request: Option<bool>,
    /// The debug adapter supports `exceptionOptions` on the
    /// `setExceptionBreakpoints` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_exception_options: Option<bool>,
    /// The debug adapter supports a `format` attribute on the `stackTrace`,
    /// `variables`, and `evaluate` requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_value_formatting_options: Option<bool>,
    /// The debug adapter supports the `exceptionInfo` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_exception_info_request: Option<bool>,
    /// The debug adapter supports the `terminateDebuggee` attribute on the
    /// `disconnect` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support_terminate_debuggee: Option<bool>,
    /// The debug adapter supports the `suspendDebuggee` attribute on the
    /// `disconnect` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support_suspend_debuggee: Option<bool>,
    /// The debug adapter supports the delayed loading of parts of the stack, which
    /// requires that both the `startFrame` and `levels` arguments and the
    /// `totalFrames` result of the `stackTrace` request are supported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_delayed_stack_trace_loading: Option<bool>,
    /// The debug adapter supports the `loadedSources` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_loaded_sources_request: Option<bool>,
    /// The debug adapter supports log points by interpreting the `logMessage`
    /// attribute of the `SourceBreakpoint`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_log_points: Option<bool>,
    /// The debug adapter supports the `terminateThreads` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_terminate_threads_request: Option<bool>,
    /// The debug adapter supports the `setExpression` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_set_expression: Option<bool>,
    /// The debug adapter supports the `terminate` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_terminate_request: Option<bool>,
    /// The debug adapter supports data breakpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_data_breakpoints: Option<bool>,
    /// The debug adapter supports the `readMemory` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_read_memory_request: Option<bool>,
    /// The debug adapter supports the `writeMemory` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_write_memory_request: Option<bool>,
    /// The debug adapter supports the `disassemble` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_disassemble_request: Option<bool>,
    /// The debug adapter supports the `cancel` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_cancel_request: Option<bool>,
    /// The debug adapter supports the `breakpointLocations` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_breakpoint_locations_request: Option<bool>,
    /// The debug adapter supports the `clipboard` context value in the `evaluate`
    /// request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_clipboard_context: Option<bool>,
    /// The debug adapter supports stepping granularities (argument `granularity`)
    /// for the stepping requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_stepping_granularity: Option<bool>,
    /// The debug adapter supports adding breakpoints based on instruction
    /// references.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_instruction_breakpoints: Option<bool>,
    /// The debug adapter supports `filterOptions` as an argument on the
    /// `setExceptionBreakpoints` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_exception_filter_options: Option<bool>,
    /// The debug adapter supports the `singleThread` property on the execution
    /// requests (`continue`, `next`, `stepIn`, `stepOut`, `reverseContinue`,
    /// `stepBack`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_single_thread_execution_requests: Option<bool>,
}

/// A Source is a descriptor for source code.
///
/// It is returned from the debug adapter as part of a StackFrame and it is used by clients when
/// specifying breakpoints.
///
/// Specification: [Source](https://microsoft.github.io/debug-adapter-protocol/specification#Types_Source)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Source {
    /// The short name of the source. Every source returned from the debug adapter
    /// has a name.
    /// When sending a source to the debug adapter this name is optional.
    pub name: Option<String>,
    /// The path of the source to be shown in the UI.
    /// It is only used to locate and load the content of the source if no
    /// `sourceReference` is specified (or its value is 0).
    pub path: Option<String>,
    /// If the value > 0 the contents of the source must be retrieved through the
    /// `source` request (even if a path is specified).
    /// Since a `sourceReference` is only valid for a session, it can not be used
    /// to persist a source.
    /// The value should be less than or equal to 2147483647 (2^31-1).
    pub source_reference: Option<i32>,
    /// A hint for how to present the source in the UI.
    /// A value of `deemphasize` can be used to indicate that the source is not
    /// available or that it is skipped on stepping.
    pub presentation_hint: PresentationHint,
    /// The origin of this source. For example, 'internal module', 'inlined content
    /// from source map', etc.
    pub origin: Option<String>,
    /// A list of sources that are related to this source. These may be the source
    /// that generated this source.
    pub sources: Option<Vec<Source>>,
    /// Additional data that a debug adapter might want to loop through the client.
    /// The client should leave the data intact and persist it across sessions. The
    /// client should not interpret the data.
    pub adapter_data: Option<Value>,
    /// The checksums associated with this file.
    pub checksums: Option<Vec<Checksum>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SourceBreakpoint {
    /// The source line of the breakpoint or logpoint.
    pub line: usize,
    /// Start position within source line of the breakpoint or logpoint. It is
    /// measured in UTF-16 code units and the client capability `columnsStartAt1`
    /// determines whether it is 0- or 1-based.
    pub column: Option<usize>,
    /// The expression for conditional breakpoints.
    /// It is only honored by a debug adapter if the corresponding capability
    /// `supportsConditionalBreakpoints` is true.
    pub condition: Option<String>,
    /// The expression that controls how many hits of the breakpoint are ignored.
    /// The debug adapter is expected to interpret the expression as needed.
    /// The attribute is only honored by a debug adapter if the corresponding
    /// capability `supportsHitConditionalBreakpoints` is true.
    pub hit_condition: Option<String>,
    /// If this attribute exists and is non-empty, the debug adapter must not
    /// 'break' (stop)
    /// but log the message instead. Expressions within `{}` are interpolated.
    /// The attribute is only honored by a debug adapter if the corresponding
    /// capability `supportsLogPoints` is true.
    pub log_message: Option<String>,
}

/// Information about a breakpoint created in setBreakpoints, setFunctionBreakpoints,
/// setInstructionBreakpoints, or setDataBreakpoints requests.
#[derive(Serialize, Debug, Clone)]
pub struct Breakpoint {
    /// The identifier for the breakpoint. It is needed if breakpoint events are
    /// used to update or remove breakpoints.
    pub id: Option<usize>,
    /// If true, the breakpoint could be set (but not necessarily at the desired
    /// location).
    pub verified: bool,
    /// A message about the state of the breakpoint.
    /// This is shown to the user and can be used to explain why a breakpoint could
    /// not be verified.
    pub message: Option<String>,
    /// The source where the breakpoint is located.
    pub source: Option<Source>,
    /// The start line of the actual range covered by the breakpoint.
    pub line: Option<usize>,
    /// Start position of the source range covered by the breakpoint. It is
    /// measured in UTF-16 code units and the client capability `columnsStartAt1`
    /// determines whether it is 0- or 1-based.
    pub column: Option<usize>,
    /// The end line of the actual range covered by the breakpoint.
    pub end_line: Option<usize>,
    /// End position of the source range covered by the breakpoint. It is measured
    /// in UTF-16 code units and the client capability `columnsStartAt1` determines
    /// whether it is 0- or 1-based.
    /// If no end line is given, then the end column is assumed to be in the start
    /// line.
    pub end_column: Option<usize>,
    /// A memory reference to where the breakpoint is set.
    pub instruction_reference: Option<String>,
    /// The offset from the instruction reference.
    /// This can be negative.
    pub offset: Option<isize>,
}

#[derive(Serialize, Debug, Clone)]
pub enum PresentationHint {
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "emphasize")]
    Emphasize,
    #[serde(rename = "deemphasize")]
    DeEmphasize,
}

impl FromStr for PresentationHint {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "normal" => Ok(PresentationHint::Normal),
            "emphasize" => Ok(PresentationHint::Emphasize),
            "deemphasize" => Ok(PresentationHint::DeEmphasize),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "PresentationHint".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

fromstr_deser! {PresentationHint}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Checksum {
    /// The algorithm used to calculate this checksum.
    pub algorithm: ChecksumAlgorithm,
    /// Value of the checksum, encoded as a hexadecimal value.
    pub checksum: String,
}

/// An ExceptionFilterOptions is used to specify an exception filter together with a condition for
/// the setExceptionBreakpoints request.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionFilterOptions {
    /// ID of an exception filter returned by the `exceptionBreakpointFilters`
    /// capability.
    pub filter_id: String,
    /// An expression for conditional exceptions.
    /// The exception breaks into the debugger if the result of the condition is
    /// true.
    pub condition: Option<String>,
}

/// This enumeration defines all possible conditions when a thrown exception should result in a
/// break.
///
/// Specification: [`ExceptionBreakMode`](https://microsoft.github.io/debug-adapter-protocol/specification#Types_ExceptionBreakMode)
#[derive(Serialize, Debug, Clone)]
pub enum ExceptionBreakMode {
    /// never breaks
    #[serde(rename = "never")]
    Never,
    /// always breaks
    #[serde(rename = "always")]
    Always,
    /// breaks when exception unhandled
    #[serde(rename = "unhandled")]
    Unhandled,
    /// breaks if the exception is not handled by user code
    #[serde(rename = "userUnhandled")]
    UserUnhandled,
}

impl FromStr for ExceptionBreakMode {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "never" => Ok(ExceptionBreakMode::Never),
            "always" => Ok(ExceptionBreakMode::Always),
            "unhandled" => Ok(ExceptionBreakMode::Unhandled),
            "userUnhandled" => Ok(ExceptionBreakMode::UserUnhandled),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "ExceptionBreakMode".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

fromstr_deser! { ExceptionBreakMode }

/// An ExceptionPathSegment represents a segment in a path that is used to match leafs or nodes in
/// a tree of exceptions.
/// If a segment consists of more than one name, it matches the names provided if negate is false
/// or missing, or it matches anything except the names provided if negate is true.
///
/// Specification: [`ExceptionPathSegment`](https://microsoft.github.io/debug-adapter-protocol/specification#Types_ExceptionPathSegment)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionPathSegment {
    /// If false or missing this segment matches the names provided, otherwise it
    /// matches anything except the names provided.
    pub negate: Option<bool>,
    /// Depending on the value of `negate` the names that should match or not
    /// match.
    pub names: Vec<String>,
}

/// An ExceptionOptions assigns configuration options to a set of exceptions.
///
/// Specification: [`ExceptionOptions`](https://microsoft.github.io/debug-adapter-protocol/specification#Types_ExceptionOptions)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionOptions {
    /// A path that selects a single or multiple exceptions in a tree. If `path` is
    /// missing, the whole tree is selected.
    /// By convention the first segment of the path is a category that is used to
    /// group exceptions in the UI.
    pub path: Option<Vec<ExceptionPathSegment>>,
    /// Condition when a thrown exception should result in a break.
    pub break_mode: ExceptionBreakMode,
}

/// Properties of a breakpoint passed to the setFunctionBreakpoints request.
///
/// Specification: [FunctionBreakpoint](https://microsoft.github.io/debug-adapter-protocol/specification#Types_FunctionBreakpoint)
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionBreakpoint {
    /// The name of the function.
    pub name: String,
    /// An expression for conditional breakpoints.
    /// It is only honored by a debug adapter if the corresponding capability
    /// `supportsConditionalBreakpoints` is true.
    pub condition: Option<String>,
    /// An expression that controls how many hits of the breakpoint are ignored.
    /// The debug adapter is expected to interpret the expression as needed.
    /// The attribute is only honored by a debug adapter if the corresponding
    /// capability `supportsHitConditionalBreakpoints` is true.
    pub hit_condition: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub enum StopReason {
    #[serde(rename = "step")]
    Step,
    #[serde(rename = "breakpoint")]
    Breakpoint,
    #[serde(rename = "exception")]
    Exception,
    #[serde(rename = "pause")]
    Pause,
    #[serde(rename = "entry")]
    Entry,
    #[serde(rename = "goto")]
    Goto,
    #[serde(rename = "function breakpoint")]
    FunctionBreakpoint,
    #[serde(rename = "data breakpoint")]
    DataBreakpoint,
    #[serde(rename = "instruction breakpoint")]
    InstructionBreakpoint,
}

impl FromStr for StopReason {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "step" => Ok(StopReason::Step),
            "breakpoint" => Ok(StopReason::Breakpoint),
            "exception" => Ok(StopReason::Exception),
            "pause" => Ok(StopReason::Pause),
            "entry" => Ok(StopReason::Entry),
            "goto" => Ok(StopReason::Goto),
            "function breakpoint" => Ok(StopReason::FunctionBreakpoint),
            "data breakpoint" => Ok(StopReason::DataBreakpoint),
            "instruction breakpoint" => Ok(StopReason::InstructionBreakpoint),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "StopReason".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

fromstr_deser! { StopReason }

#[derive(Serialize, Debug, Clone)]
pub enum BreakpointEventReason {
    #[serde(rename = "changed")]
    Changed,
    #[serde(rename = "new")]
    New,
    #[serde(rename = "removed")]
    Removed,
    String(String),
}

impl FromStr for BreakpointEventReason {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "changed" => Ok(BreakpointEventReason::Changed),
            "new" => Ok(BreakpointEventReason::New),
            "removed" => Ok(BreakpointEventReason::Removed),
            other => Ok(BreakpointEventReason::String(other.to_string())),
        }
    }
}

fromstr_deser! { BreakpointEventReason }

#[derive(Debug, Clone)]
pub enum InvalidatedAreas {
    All,
    Stacks,
    Threads,
    Variables,
    String(String),
}

impl FromStr for InvalidatedAreas {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "all" => Ok(InvalidatedAreas::All),
            "stacks" => Ok(InvalidatedAreas::Stacks),
            "threads" => Ok(InvalidatedAreas::Threads),
            "variables" => Ok(InvalidatedAreas::Variables),
            other => Ok(InvalidatedAreas::String(other.to_string())),
        }
    }
}

impl ToString for InvalidatedAreas {
    fn to_string(&self) -> String {
        match &self {
            InvalidatedAreas::All => "all",
            InvalidatedAreas::Stacks => "stacks",
            InvalidatedAreas::Threads => "threads",
            InvalidatedAreas::Variables => "variables",
            InvalidatedAreas::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { InvalidatedAreas }
tostr_ser! { InvalidatedAreas }

#[derive(Debug, Clone)]
pub enum LoadedSourceEventReason {
    New,
    Changed,
    Removed,
}

impl FromStr for LoadedSourceEventReason {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "new" => Ok(LoadedSourceEventReason::New),
            "changed" => Ok(LoadedSourceEventReason::Changed),
            "removed" => Ok(LoadedSourceEventReason::Removed),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "LoadedSourceEventReason".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for LoadedSourceEventReason {
    fn to_string(&self) -> String {
        match &self {
            LoadedSourceEventReason::New => "new",
            LoadedSourceEventReason::Changed => "changed",
            LoadedSourceEventReason::Removed => "removed",
        }
        .to_string()
    }
}

fromstr_deser! { LoadedSourceEventReason }
tostr_ser! { LoadedSourceEventReason }

#[derive(Debug, Clone)]
pub enum ModuleEventReason {
    New,
    Changed,
    Removed,
}

impl FromStr for ModuleEventReason {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "new" => Ok(ModuleEventReason::New),
            "changed" => Ok(ModuleEventReason::Changed),
            "removed" => Ok(ModuleEventReason::Removed),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "ModuleEventReason".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for ModuleEventReason {
    fn to_string(&self) -> String {
        match &self {
            ModuleEventReason::New => "new",
            ModuleEventReason::Changed => "changed",
            ModuleEventReason::Removed => "removed",
        }
        .to_string()
    }
}

fromstr_deser! { ModuleEventReason }
tostr_ser! { ModuleEventReason }

#[derive(Serialize, Debug, Clone)]
pub struct Module {
    /// Unique identifier for the module.
    pub id: ModuleId,
    /// A name of the module.
    pub name: String,
    /// Logical full path to the module. The exact definition is implementation
    /// defined, but usually this would be a full path to the on-disk file for the
    /// module.
    pub path: Option<String>,
    /// True if the module is optimized.
    pub is_optimized: Option<bool>,
    /// True if the module is considered 'user code' by a debugger that supports
    /// 'Just My Code'.
    pub is_user_code: Option<bool>,
    /// Version of Module.
    pub version: Option<String>,
    /// User-understandable description of if symbols were found for the module
    /// (ex: 'Symbols Loaded', 'Symbols not found', etc.)
    pub symbol_status: Option<String>,
    /// Logical full path to the symbol file. The exact definition is
    /// implementation defined.
    pub symbol_file_path: Option<String>,
    /// Module created or modified, encoded as a RFC 3339 timestamp.
    pub date_time_stamp: Option<String>,
    /// Address range covered by this module.
    pub address_range: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ModuleId {
    Number,
    String(String),
}

impl FromStr for ModuleId {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "number" => Ok(ModuleId::Number),
            other => Ok(ModuleId::String(other.to_string())),
        }
    }
}

impl ToString for ModuleId {
    fn to_string(&self) -> String {
        match &self {
            ModuleId::Number => "number",
            ModuleId::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { ModuleId }
tostr_ser! { ModuleId }

#[derive(Debug, Clone)]
pub enum OutputEventCategory {
    Console,
    Important,
    Stdout,
    Stderr,
    Telemetry,
    String(String),
}

impl FromStr for OutputEventCategory {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "console" => Ok(OutputEventCategory::Console),
            "important" => Ok(OutputEventCategory::Important),
            "stdout" => Ok(OutputEventCategory::Stdout),
            "stderr" => Ok(OutputEventCategory::Stderr),
            "telemetry" => Ok(OutputEventCategory::Telemetry),
            other => Ok(OutputEventCategory::String(other.to_string())),
        }
    }
}

impl ToString for OutputEventCategory {
    fn to_string(&self) -> String {
        match &self {
            OutputEventCategory::Console => "console",
            OutputEventCategory::Important => "important",
            OutputEventCategory::Stdout => "stdout",
            OutputEventCategory::Stderr => "stderr",
            OutputEventCategory::Telemetry => "telemetry",
            OutputEventCategory::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { OutputEventCategory }
tostr_ser! { OutputEventCategory }

#[derive(Debug, Clone)]
pub enum OutputEventGroup {
    Start,
    StartCollapsed,
    End,
}

impl FromStr for OutputEventGroup {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "start" => Ok(OutputEventGroup::Start),
            "startCollapsed" => Ok(OutputEventGroup::StartCollapsed),
            "end" => Ok(OutputEventGroup::End),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "OutputEventGroup".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for OutputEventGroup {
    fn to_string(&self) -> String {
        match &self {
            OutputEventGroup::Start => "start",
            OutputEventGroup::StartCollapsed => "startCollapsed",
            OutputEventGroup::End => "end",
        }
        .to_string()
    }
}

fromstr_deser! { OutputEventGroup }
tostr_ser! { OutputEventGroup }

#[derive(Debug, Clone)]
pub enum ProcessEventStartMethod {
    Launch,
    Attach,
    AttachForSuspendedLaunch,
}

impl FromStr for ProcessEventStartMethod {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "launch" => Ok(ProcessEventStartMethod::Launch),
            "attach" => Ok(ProcessEventStartMethod::Attach),
            "attachForSuspendedLaunch" => Ok(ProcessEventStartMethod::AttachForSuspendedLaunch),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "ProcessEventStartmethod".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for ProcessEventStartMethod {
    fn to_string(&self) -> String {
        match &self {
            ProcessEventStartMethod::Launch => "launch",
            ProcessEventStartMethod::Attach => "attach",
            ProcessEventStartMethod::AttachForSuspendedLaunch => "attachForSuspendedLaunch",
        }
        .to_string()
    }
}

fromstr_deser! { ProcessEventStartMethod }
tostr_ser! { ProcessEventStartMethod }

#[derive(Debug, Clone)]
pub enum StoppedEventReason {
    Step,
    Breakpoint,
    Exception,
    Pause,
    Entry,
    Goto,
    Function,
    Data,
    Instruction,
    String(String),
}

impl FromStr for StoppedEventReason {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "step" => Ok(StoppedEventReason::Step),
            "breakpoint" => Ok(StoppedEventReason::Breakpoint),
            "exception" => Ok(StoppedEventReason::Exception),
            "pause" => Ok(StoppedEventReason::Pause),
            "entry" => Ok(StoppedEventReason::Entry),
            "goto" => Ok(StoppedEventReason::Goto),
            "function" => Ok(StoppedEventReason::Function),
            "data" => Ok(StoppedEventReason::Data),
            "instruction" => Ok(StoppedEventReason::Instruction),
            other => Ok(StoppedEventReason::String(other.to_string())),
        }
    }
}

impl ToString for StoppedEventReason {
    fn to_string(&self) -> String {
        match &self {
            StoppedEventReason::Step => "step",
            StoppedEventReason::Breakpoint => "breakpoint",
            StoppedEventReason::Exception => "exception",
            StoppedEventReason::Pause => "pause",
            StoppedEventReason::Entry => "entry",
            StoppedEventReason::Goto => "goto",
            StoppedEventReason::Function => "function",
            StoppedEventReason::Data => "data",
            StoppedEventReason::Instruction => "instruction",
            StoppedEventReason::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { StoppedEventReason }
tostr_ser! { StoppedEventReason }

#[derive(Debug, Clone)]
pub enum ThreadEventReason {
    Started,
    Exited,
    String(String),
}

impl FromStr for ThreadEventReason {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "started" => Ok(ThreadEventReason::Started),
            "exited" => Ok(ThreadEventReason::Exited),
            other => Ok(ThreadEventReason::String(other.to_string())),
        }
    }
}

impl ToString for ThreadEventReason {
    fn to_string(&self) -> String {
        match &self {
            ThreadEventReason::Started => "started",
            ThreadEventReason::Exited => "exited",
            ThreadEventReason::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { ThreadEventReason }
tostr_ser! { ThreadEventReason }

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ValueFormat {
    /// Display the value in hex.
    pub hex: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StackFrameFormat {
    /// Display the value in hex.
    pub hex: Option<bool>,
    /// Displays parameters for the stack frame.
    pub parameters: Option<bool>,
    /// Displays the types of parameters for the stack frame.
    pub parameter_types: Option<bool>,
    /// Displays the names of parameters for the stack frame.
    pub parameter_names: Option<bool>,
    /// Displays the values of parameters for the stack frame.
    pub parameter_values: Option<bool>,
    /// Displays the line usize of the stack frame.
    pub line: Option<bool>,
    /// Displays the module of the stack frame.
    pub module: Option<bool>,
    /// Includes all stack frames, including those the debug adapter might
    /// otherwise hide.
    pub include_all: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum EvaluateArgumentsContext {
    Variables,
    Watch,
    Repl,
    Hover,
    Clipboard,
    String(String),
}

impl FromStr for EvaluateArgumentsContext {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "variables" => Ok(EvaluateArgumentsContext::Variables),
            "watch" => Ok(EvaluateArgumentsContext::Watch),
            "repl" => Ok(EvaluateArgumentsContext::Repl),
            "hover" => Ok(EvaluateArgumentsContext::Hover),
            "clipboard" => Ok(EvaluateArgumentsContext::Clipboard),
            other => Ok(EvaluateArgumentsContext::String(other.to_string())),
        }
    }
}

impl ToString for EvaluateArgumentsContext {
    fn to_string(&self) -> String {
        match &self {
            EvaluateArgumentsContext::Variables => "variables",
            EvaluateArgumentsContext::Watch => "watch",
            EvaluateArgumentsContext::Repl => "repl",
            EvaluateArgumentsContext::Hover => "hover",
            EvaluateArgumentsContext::Clipboard => "clipboard",
            EvaluateArgumentsContext::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { EvaluateArgumentsContext }
tostr_ser! { EvaluateArgumentsContext }

#[derive(Debug, Clone)]
pub enum SteppingGranularity {
    Statement,
    Line,
    Instruction,
}

impl FromStr for SteppingGranularity {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "statement" => Ok(SteppingGranularity::Statement),
            "line" => Ok(SteppingGranularity::Line),
            "instruction" => Ok(SteppingGranularity::Instruction),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "SteppingGranularity".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for SteppingGranularity {
    fn to_string(&self) -> String {
        match &self {
            SteppingGranularity::Statement => "statement",
            SteppingGranularity::Line => "line",
            SteppingGranularity::Instruction => "instruction",
        }
        .to_string()
    }
}

fromstr_deser! { SteppingGranularity }
tostr_ser! { SteppingGranularity }

#[derive(Debug, Clone)]
pub enum DataBreakpointAccessType {
    Read,
    Write,
    ReadWrite,
}

impl FromStr for DataBreakpointAccessType {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read" => Ok(DataBreakpointAccessType::Read),
            "write" => Ok(DataBreakpointAccessType::Write),
            "readWrite" => Ok(DataBreakpointAccessType::ReadWrite),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "DataBreakpointAccessType".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for DataBreakpointAccessType {
    fn to_string(&self) -> String {
        match &self {
            DataBreakpointAccessType::Read => "read",
            DataBreakpointAccessType::Write => "write",
            DataBreakpointAccessType::ReadWrite => "readWrite",
        }
        .to_string()
    }
}

fromstr_deser! { DataBreakpointAccessType }
tostr_ser! { DataBreakpointAccessType }

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DataBreakpoint {
    /// An id representing the data. This id is returned from the
    /// `dataBreakpointInfo` request.
    pub data_id: String,
    /// The access type of the data.
    pub access_type: Option<DataBreakpointAccessType>,
    /// An expression for conditional breakpoints.
    pub condition: Option<String>,
    /// An expression that controls how many hits of the breakpoint are ignored.
    /// The debug adapter is expected to interpret the expression as needed.
    pub hit_condition: Option<String>,
}

/// Properties of a breakpoint passed to the setInstructionBreakpoints request
///
/// Specfication: [InstructionBreakpoint](https://microsoft.github.io/debug-adapter-protocol/specification#Types_InstructionBreakpoint)
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstructionBreakpoint {
    /// The instruction reference of the breakpoint.
    /// This should be a memory or instruction pointer reference from an
    /// `EvaluateResponse`, `Variable`, `StackFrame`, `GotoTarget`, or
    /// `Breakpoint`.
    pub instruction_reference: String,
    /// The offset from the instruction reference.
    /// This can be negative.
    pub offset: Option<isize>,
    /// An expression for conditional breakpoints.
    /// It is only honored by a debug adapter if the corresponding capability
    /// `supportsConditionalBreakpoints` is true.
    pub condition: Option<String>,
    /// An expression that controls how many hits of the breakpoint are ignored.
    /// The debug adapter is expected to interpret the expression as needed.
    /// The attribute is only honored by a debug adapter if the corresponding
    /// capability `supportsHitConditionalBreakpoints` is true.
    pub hit_condition: Option<String>,
}

#[derive(Debug, Clone)]
pub enum VariablesArgumentsFilter {
    Indexed,
    Named,
}

impl FromStr for VariablesArgumentsFilter {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "indexed" => Ok(VariablesArgumentsFilter::Indexed),
            "named" => Ok(VariablesArgumentsFilter::Named),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "VariablesArgumentsFilter".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for VariablesArgumentsFilter {
    fn to_string(&self) -> String {
        match &self {
            VariablesArgumentsFilter::Indexed => "indexed",
            VariablesArgumentsFilter::Named => "named",
        }
        .to_string()
    }
}

fromstr_deser! { VariablesArgumentsFilter }
tostr_ser! { VariablesArgumentsFilter }

/// Properties of a breakpoint location returned from the breakpointLocations request.
///
/// Specfication: [BreakpointLocation](https://microsoft.github.io/debug-adapter-protocol/specification#Types_BreakpointLocation)
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointLocation {
    /// Start line of breakpoint location.
    pub line: usize,
    /// The start position of a breakpoint location. Position is measured in UTF-16
    /// code units and the client capability `columnsStartAt1` determines whether
    /// it is 0- or 1-based.
    pub column: Option<usize>,
    /// The end line of breakpoint location if the location covers a range.
    pub end_line: Option<usize>,
    /// The end position of a breakpoint location (if the location covers a range).
    /// Position is measured in UTF-16 code units and the client capability
    /// `columnsStartAt1` determines whether it is 0- or 1-based.
    pub end_column: Option<usize>,
}

/// Some predefined types for the CompletionItem. Please note that not all clients have specific
/// icons for all of them
///
/// Specification: [CompletionItemType](https://microsoft.github.io/debug-adapter-protocol/specification#Types_CompletionItemType)
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum CompletionItemType {
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Interface,
    Module,
    Property,
    Unit,
    Value,
    Enum,
    Keyword,
    Snippet,
    Text,
    Color,
    File,
    Reference,
    CustomColor,
}

impl FromStr for CompletionItemType {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "method" => Ok(CompletionItemType::Method),
            "function" => Ok(CompletionItemType::Function),
            "constructor" => Ok(CompletionItemType::Constructor),
            "field" => Ok(CompletionItemType::Field),
            "variable" => Ok(CompletionItemType::Variable),
            "class" => Ok(CompletionItemType::Class),
            "interface" => Ok(CompletionItemType::Interface),
            "module" => Ok(CompletionItemType::Module),
            "property" => Ok(CompletionItemType::Property),
            "unit" => Ok(CompletionItemType::Unit),
            "value" => Ok(CompletionItemType::Value),
            "enum" => Ok(CompletionItemType::Enum),
            "keyword" => Ok(CompletionItemType::Keyword),
            "snippet" => Ok(CompletionItemType::Snippet),
            "text" => Ok(CompletionItemType::Text),
            "color" => Ok(CompletionItemType::Color),
            "file" => Ok(CompletionItemType::File),
            "reference" => Ok(CompletionItemType::Reference),
            "customcolor" => Ok(CompletionItemType::CustomColor),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "CompletionItemType".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for CompletionItemType {
    fn to_string(&self) -> String {
        match &self {
            CompletionItemType::Method => "method",
            CompletionItemType::Function => "function",
            CompletionItemType::Constructor => "constructor",
            CompletionItemType::Field => "field",
            CompletionItemType::Variable => "variable",
            CompletionItemType::Class => "class",
            CompletionItemType::Interface => "interface",
            CompletionItemType::Module => "module",
            CompletionItemType::Property => "property",
            CompletionItemType::Unit => "unit",
            CompletionItemType::Value => "value",
            CompletionItemType::Enum => "enum",
            CompletionItemType::Keyword => "keyword",
            CompletionItemType::Snippet => "snippet",
            CompletionItemType::Text => "text",
            CompletionItemType::Color => "color",
            CompletionItemType::File => "file",
            CompletionItemType::Reference => "reference",
            CompletionItemType::CustomColor => "customcolor",
        }
        .to_string()
    }
}

fromstr_deser! { CompletionItemType }

/// `CompletionItems` are the suggestions returned from the `completions` request.
///
/// Specification: [CompletionItem](https://microsoft.github.io/debug-adapter-protocol/specification#Types_CompletionItem)
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    /// The label of this completion item. By default this is also the text that is
    /// inserted when selecting this completion.
    pub label: String,
    /// If text is returned and not an empty String, then it is inserted instead of
    /// the label.
    pub text: Option<String>,
    /// A String that should be used when comparing this item with other items. If
    /// not returned or an empty String, the `label` is used instead.
    pub sort_text: Option<String>,
    /// A human-readable String with additional information about this item, like
    /// type or symbol information.
    pub detail: Option<String>,
    /// The item's type. Typically the client uses this information to render the
    /// item in the UI with an icon.
    #[serde(rename = "type")]
    pub type_field: Option<CompletionItemType>,
    /// Start position (within the `text` attribute of the `completions` request)
    /// where the completion text is added. The position is measured in UTF-16 code
    /// units and the client capability `columnsStartAt1` determines whether it is
    /// 0- or 1-based. If the start position is omitted the text is added at the
    /// location specified by the `column` attribute of the `completions` request.
    pub start: Option<usize>,
    /// Length determines how many characters are overwritten by the completion
    /// text and it is measured in UTF-16 code units. If missing the value 0 is
    /// assumed which results in the completion text being inserted.
    pub length: Option<usize>,
    /// Determines the start of the new selection after the text has been inserted
    /// (or replaced). `selectionStart` is measured in UTF-16 code units and must
    /// be in the range 0 and length of the completion text. If omitted the
    /// selection starts at the end of the completion text.
    pub selection_start: Option<usize>,
    /// Determines the length of the new selection after the text has been inserted
    /// (or replaced) and it is measured in UTF-16 code units. The selection can
    /// not extend beyond the bounds of the completion text. If omitted the length
    /// is assumed to be 0.
    pub selection_length: Option<usize>,
}

/// Represents a single disassembled instruction.
///
/// Specification: [DisassembledInstruction](https://microsoft.github.io/debug-adapter-protocol/specification#Types_DisassembledInstruction)
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DisassembledInstruction {
    /// The address of the instruction. Treated as a hex value if prefixed with
    /// `0x`, or as a decimal value otherwise.
    pub address: String,
    /// Raw bytes representing the instruction and its operands, in an
    /// implementation-defined format.
    pub instruction_bytes: Option<String>,
    /// Text representing the instruction and its operands, in an
    /// implementation-defined format.
    pub instruction: String,
    /// Name of the symbol that corresponds with the location of this instruction,
    /// if any.
    pub symbol: Option<String>,
    /// Source location that corresponds to this instruction, if any.
    /// Should always be set (if available) on the first instruction returned,
    /// but can be omitted afterwards if this instruction maps to the same source
    /// file as the previous instruction.
    pub location: Option<Source>,
    /// The line within the source location that corresponds to this instruction,
    /// if any.
    pub line: Option<usize>,
    /// The column within the line that corresponds to this instruction, if any.
    pub column: Option<usize>,
    /// The end line of the range that corresponds to this instruction, if any.
    pub end_line: Option<usize>,
    /// The end column of the range that corresponds to this instruction, if any.
    pub end_column: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum VariablePresentationHintKind {
    /// Indicates that the object is a property.
    Property,
    /// Indicates that the object is a method.
    Method,
    /// Indicates that the object is a class.
    Class,
    /// Indicates that the object is data.
    Data,
    /// Indicates that the object is an event.
    Event,
    /// Indicates that the object is a base class.
    BaseClass,
    /// Indicates that the object is an inner class.
    InnerClass,
    /// Indicates that the object is an interface.
    Interface,
    /// Indicates that the object is the most derived class.
    MostDerivedClass,
    /// Indicates that the object is virtual, that means it is a
    /// synthetic object introduced by the adapter for rendering purposes, e.g. an
    /// index range for large arrays.
    Virtual,
    /// Deprecated: Indicates that a data breakpoint is
    /// registered for the object. The `hasDataBreakpoint` attribute should
    /// generally be used instead.
    DataBreakpoint,
    String(String),
}

impl FromStr for VariablePresentationHintKind {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "property" => Ok(VariablePresentationHintKind::Property),
            "method" => Ok(VariablePresentationHintKind::Method),
            "class" => Ok(VariablePresentationHintKind::Class),
            "data" => Ok(VariablePresentationHintKind::Data),
            "event" => Ok(VariablePresentationHintKind::Event),
            "baseClass" => Ok(VariablePresentationHintKind::BaseClass),
            "innerClass" => Ok(VariablePresentationHintKind::InnerClass),
            "interface" => Ok(VariablePresentationHintKind::Interface),
            "mostDerivedClass" => Ok(VariablePresentationHintKind::MostDerivedClass),
            "virtual" => Ok(VariablePresentationHintKind::Virtual),
            "dataBreakpoint" => Ok(VariablePresentationHintKind::DataBreakpoint),
            other => Ok(VariablePresentationHintKind::String(other.to_string())),
        }
    }
}

impl ToString for VariablePresentationHintKind {
    fn to_string(&self) -> String {
        match &self {
            VariablePresentationHintKind::Property => "property",
            VariablePresentationHintKind::Method => "method",
            VariablePresentationHintKind::Class => "class",
            VariablePresentationHintKind::Data => "data",
            VariablePresentationHintKind::Event => "event",
            VariablePresentationHintKind::BaseClass => "baseClass",
            VariablePresentationHintKind::InnerClass => "innerClass",
            VariablePresentationHintKind::Interface => "interface",
            VariablePresentationHintKind::MostDerivedClass => "mostDerivedClass",
            VariablePresentationHintKind::Virtual => "virtual",
            VariablePresentationHintKind::DataBreakpoint => "dataBreakpoint",
            VariablePresentationHintKind::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { VariablePresentationHintKind }
tostr_ser! { VariablePresentationHintKind }

/// Set of attributes represented as an array of Strings. Before introducing
/// additional values, try to use the listed values.
#[derive(Debug, Clone)]
pub enum VariablePresentationHintAttributes {
    /// Indicates that the object is static.
    Static,
    /// Indicates that the object is a constant.
    Constant,
    /// Indicates that the object is read only.
    ReadOnly,
    /// Indicates that the object is a raw String.
    RawString,
    /// Indicates that the object can have an Object ID created for it.
    HasObjectId,
    /// Indicates that the object has an Object ID associated with it.
    CanHaveObjectId,
    /// Indicates that the evaluation had side effects.
    HasSideEffects,
    /// Indicates that the object has its value tracked by a data breakpoint.
    HasDataBreakpoint,
    String(String),
}

impl FromStr for VariablePresentationHintAttributes {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "static" => Ok(VariablePresentationHintAttributes::Static),
            "constant" => Ok(VariablePresentationHintAttributes::Constant),
            "readOnly" => Ok(VariablePresentationHintAttributes::ReadOnly),
            "rawString" => Ok(VariablePresentationHintAttributes::RawString),
            "hasObjectId" => Ok(VariablePresentationHintAttributes::HasObjectId),
            "canHaveObjectId" => Ok(VariablePresentationHintAttributes::CanHaveObjectId),
            "hasSideEffects" => Ok(VariablePresentationHintAttributes::HasSideEffects),
            "hasDataBreakpoint" => Ok(VariablePresentationHintAttributes::HasDataBreakpoint),
            other => Ok(VariablePresentationHintAttributes::String(
                other.to_string(),
            )),
        }
    }
}

impl ToString for VariablePresentationHintAttributes {
    fn to_string(&self) -> String {
        match &self {
            VariablePresentationHintAttributes::Static => "static",
            VariablePresentationHintAttributes::Constant => "constant",
            VariablePresentationHintAttributes::ReadOnly => "readOnly",
            VariablePresentationHintAttributes::RawString => "rawString",
            VariablePresentationHintAttributes::HasObjectId => "hasObjectId",
            VariablePresentationHintAttributes::CanHaveObjectId => "canHaveObjectId",
            VariablePresentationHintAttributes::HasSideEffects => "hasSideEffects",
            VariablePresentationHintAttributes::HasDataBreakpoint => "hasDataBreakpoint",
            VariablePresentationHintAttributes::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { VariablePresentationHintAttributes }
tostr_ser! { VariablePresentationHintAttributes }

#[derive(Debug, Clone)]
pub enum VariablePresentationHintVisibility {
    Public,
    Private,
    Protected,
    Internal,
    Final,
    String(String),
}

impl FromStr for VariablePresentationHintVisibility {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "public" => Ok(VariablePresentationHintVisibility::Public),
            "private" => Ok(VariablePresentationHintVisibility::Private),
            "protected" => Ok(VariablePresentationHintVisibility::Protected),
            "internal" => Ok(VariablePresentationHintVisibility::Internal),
            "final" => Ok(VariablePresentationHintVisibility::Final),
            other => Ok(VariablePresentationHintVisibility::String(
                other.to_string(),
            )),
        }
    }
}

impl ToString for VariablePresentationHintVisibility {
    fn to_string(&self) -> String {
        match &self {
            VariablePresentationHintVisibility::Public => "public",
            VariablePresentationHintVisibility::Private => "private",
            VariablePresentationHintVisibility::Protected => "protected",
            VariablePresentationHintVisibility::Internal => "internal",
            VariablePresentationHintVisibility::Final => "final",
            VariablePresentationHintVisibility::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { VariablePresentationHintVisibility }
tostr_ser! { VariablePresentationHintVisibility }

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VariablePresentationHint {
    /// The kind of variable. Before introducing additional values, try to use the
    /// listed values.
    pub kind: Option<VariablePresentationHintKind>,
    /// Set of attributes represented as an array of Strings. Before introducing
    /// additional values, try to use the listed values.
    pub attributes: Option<Vec<VariablePresentationHintAttributes>>,
    /// Visibility of variable. Before introducing additional values, try to use
    /// the listed values.
    pub visibility: Option<VariablePresentationHintVisibility>,
    /// If true, clients can present the variable with a UI that supports a
    /// specific gesture to trigger its evaluation.
    /// This mechanism can be used for properties that require executing code when
    /// retrieving their value and where the code execution can be expensive and/or
    /// produce side-effects. A typical example are properties based on a getter
    /// function.
    /// Please note that in addition to the `lazy` flag, the variable's
    /// `variablesReference` is expected to refer to a variable that will provide
    /// the value through another `variable` request.
    pub lazy: Option<bool>,
}

/// Detailed information about an exception that has occurred.
///
/// Specification: [ExceptionDetails](https://microsoft.github.io/debug-adapter-protocol/specification#Types_ExceptionDetails)
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionDetails {
    /// Message contained in the exception.
    pub message: Option<String>,
    /// Short type name of the exception object.
    pub type_name: Option<String>,
    /// Fully-qualified type name of the exception object.
    pub full_type_name: Option<String>,
    /// An expression that can be evaluated in the current scope to obtain the
    /// exception object.
    pub evaluate_name: Option<String>,
    /// Stack trace at the time the exception was thrown.
    pub stack_trace: Option<String>,
    /// Details of the exception contained by this exception, if any.
    pub inner_exception: Option<Vec<ExceptionDetails>>,
}

/// A `GotoTarget` describes a code location that can be used as a target in the
/// goto request.
/// The possible goto targets can be determined via the gotoTargets request.
///
/// Specification: [GotoTarget](https://microsoft.github.io/debug-adapter-protocol/specification#Types_GotoTarget)
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GotoTarget {
    /// Unique identifier for a goto target. This is used in the `goto` request.
    pub id: usize,
    /// The name of the goto target (shown in the UI).
    pub label: String,
    /// The line of the goto target.
    pub line: usize,
    /// The column of the goto target.
    pub column: Option<usize>,
    /// The end line of the range covered by the goto target.
    pub end_line: Option<usize>,
    /// The end column of the range covered by the goto target.
    pub end_column: Option<usize>,
    /// A memory reference for the instruction pointer value represented by this
    /// target.
    pub instruction_pointer_reference: Option<String>,
}

/// A hint for how to present this scope in the UI. If this attribute is
/// missing, the scope is shown with a generic UI.
///
/// Specification: [Scope](https://microsoft.github.io/debug-adapter-protocol/specification#Types_Scope)
#[derive(Debug, Clone)]
pub enum ScopePresentationhint {
    /// Scope contains method arguments.
    Arguments,
    /// Scope contains local variables.
    Locals,
    /// Scope contains registers. Only a single `registers` scope
    /// should be returned from a `scopes` request.
    Registers,
    String(String),
}

impl FromStr for ScopePresentationhint {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "arguments" => Ok(ScopePresentationhint::Arguments),
            "locals" => Ok(ScopePresentationhint::Locals),
            "registers" => Ok(ScopePresentationhint::Registers),
            other => Ok(ScopePresentationhint::String(other.to_string())),
        }
    }
}

impl ToString for ScopePresentationhint {
    fn to_string(&self) -> String {
        match &self {
            ScopePresentationhint::Arguments => "arguments",
            ScopePresentationhint::Locals => "locals",
            ScopePresentationhint::Registers => "registers",
            ScopePresentationhint::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { ScopePresentationhint }
tostr_ser! { ScopePresentationhint }

/// A Scope is a named container for variables. Optionally a scope can map to a source or a range
/// within a source.
///
/// Specification: [Scope](https://microsoft.github.io/debug-adapter-protocol/specification#Types_Scope)
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    /// Name of the scope such as 'Arguments', 'Locals', or 'Registers'. This
    /// String is shown in the UI as is and can be translated.
    pub name: String,
    /// A hint for how to present this scope in the UI. If this attribute is
    /// missing, the scope is shown with a generic UI.
    /// Values:
    /// 'arguments': Scope contains method arguments.
    /// 'locals': Scope contains local variables.
    /// 'registers': Scope contains registers. Only a single `registers` scope
    /// should be returned from a `scopes` request.
    /// etc.
    pub presentation_hint: Option<ScopePresentationhint>,
    /// The variables of this scope can be retrieved by passing the value of
    /// `variablesReference` to the `variables` request.
    pub variables_reference: NonZeroUsize,
    /// The usize of named variables in this scope.
    /// The client can use this information to present the variables in a paged UI
    /// and fetch them in chunks.
    pub named_variables: Option<usize>,
    /// The usize of indexed variables in this scope.
    /// The client can use this information to present the variables in a paged UI
    /// and fetch them in chunks.
    pub indexed_variables: Option<usize>,
    /// If true, the usize of variables in this scope is large or expensive to
    /// retrieve.
    pub expensive: bool,
    /// The source for this scope.
    pub source: Option<Source>,
    /// The start line of the range covered by this scope.
    pub line: Option<usize>,
    /// Start position of the range covered by the scope. It is measured in UTF-16
    /// code units and the client capability `columnsStartAt1` determines whether
    /// it is 0- or 1-based.
    pub column: Option<usize>,
    /// The end line of the range covered by this scope.
    pub end_line: Option<usize>,
    /// End position of the range covered by the scope. It is measured in UTF-16
    /// code units and the client capability `columnsStartAt1` determines whether
    /// it is 0- or 1-based.
    pub end_column: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum StackFrameModuleid {
    Number,
    String(String),
}

impl FromStr for StackFrameModuleid {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "number" => Ok(StackFrameModuleid::Number),
            other => Ok(StackFrameModuleid::String(other.to_string())),
        }
    }
}

impl ToString for StackFrameModuleid {
    fn to_string(&self) -> String {
        match &self {
            StackFrameModuleid::Number => "number",
            StackFrameModuleid::String(other) => other,
        }
        .to_string()
    }
}

fromstr_deser! { StackFrameModuleid }
tostr_ser! { StackFrameModuleid }

#[derive(Debug, Clone)]
pub enum StackFramePresentationhint {
    Normal,
    Label,
    Subtle,
}

impl FromStr for StackFramePresentationhint {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "normal" => Ok(StackFramePresentationhint::Normal),
            "label" => Ok(StackFramePresentationhint::Label),
            "subtle" => Ok(StackFramePresentationhint::Subtle),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "StackFramePresentationhint".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for StackFramePresentationhint {
    fn to_string(&self) -> String {
        match &self {
            StackFramePresentationhint::Normal => "normal",
            StackFramePresentationhint::Label => "label",
            StackFramePresentationhint::Subtle => "subtle",
        }
        .to_string()
    }
}

fromstr_deser! { StackFramePresentationhint }
tostr_ser! { StackFramePresentationhint }

/// A Stackframe contains the source location.
///
/// Specification: [StackFrame](https://microsoft.github.io/debug-adapter-protocol/specification#Types_StackFrame)
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    /// An identifier for the stack frame. It must be unique across all threads.
    /// This id can be used to retrieve the scopes of the frame with the `scopes`
    /// request or to restart the execution of a stackframe.
    pub id: usize,
    /// The name of the stack frame, typically a method name.
    pub name: String,
    /// The source of the frame.
    pub source: Option<Source>,
    /// The line within the source of the frame. If the source attribute is missing
    /// or doesn't exist, `line` is 0 and should be ignored by the client.
    pub line: usize,
    /// Start position of the range covered by the stack frame. It is measured in
    /// UTF-16 code units and the client capability `columnsStartAt1` determines
    /// whether it is 0- or 1-based. If attribute `source` is missing or doesn't
    /// exist, `column` is 0 and should be ignored by the client.
    pub column: usize,
    /// The end line of the range covered by the stack frame.
    pub end_line: Option<usize>,
    /// End position of the range covered by the stack frame. It is measured in
    /// UTF-16 code units and the client capability `columnsStartAt1` determines
    /// whether it is 0- or 1-based.
    pub end_column: Option<usize>,
    /// Indicates whether this frame can be restarted with the `restart` request.
    /// Clients should only use this if the debug adapter supports the `restart`
    /// request and the corresponding capability `supportsRestartRequest` is true.
    pub can_restart: Option<bool>,
    /// A memory reference for the current instruction pointer in this frame.
    pub instruction_pointer_reference: Option<String>,
    /// The module associated with this frame, if any.
    pub module_id: Option<StackFrameModuleid>,
    /// A hint for how to present this frame in the UI.
    /// A value of `label` can be used to indicate that the frame is an artificial
    /// frame that is used as a visual label or separator. A value of `subtle` can
    /// be used to change the appearance of a frame in a 'subtle' way.
    /// Values: 'normal', 'label', 'subtle'
    pub presentation_hint: Option<StackFramePresentationhint>,
}

/// A thread.
///
/// Specification: [Thread](https://microsoft.github.io/debug-adapter-protocol/specification#Types_Thread)
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// Unique identifier for the thread.
    pub id: usize,
    /// The name of the thread.
    pub name: String,
}

/// A Variable is a name/value pair.
///
/// The `type` attribute is shown if space permits or when hovering over the variable’s name.
///
/// The `kind` attribute is used to render additional properties of the variable, e.g. different
/// icons can be used to indicate that a variable is public or private.
///
/// If the value is structured (has children), a handle is provided to retrieve the children with
/// the `variables` request.
///
/// If the number of named or indexed children is large, the numbers should be returned via the
/// `namedVariables` and `indexedVariables` attributes.
///
/// The client can use this information to present the children in a paged UI and fetch them in
/// chunks.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    /// The variable's name.
    pub name: String,
    /// The variable's value.
    /// This can be a multi-line text, e.g. for a function the body of a function.
    /// For structured variables (which do not have a simple value), it is
    /// recommended to provide a one-line representation of the structured object.
    /// This helps to identify the structured object in the collapsed state when
    /// its children are not yet visible.
    /// An empty String can be used if no value should be shown in the UI.
    pub value: String,
    /// The type of the variable's value. Typically shown in the UI when hovering
    /// over the value.
    /// This attribute should only be returned by a debug adapter if the
    /// corresponding capability `supportsVariableType` is true.
    #[serde(rename = "type")]
    pub type_field: Option<String>,
    /// Properties of a variable that can be used to determine how to render the
    /// variable in the UI.
    pub presentation_hint: Option<VariablePresentationHint>,
    /// The evaluatable name of this variable which can be passed to the `evaluate`
    /// request to fetch the variable's value.
    pub evaluate_name: Option<String>,
    /// If `variablesReference` is > 0, the variable is structured and its children
    /// can be retrieved by passing `variablesReference` to the `variables`
    /// request.
    pub variables_reference: usize,
    /// The usize of named child variables.
    /// The client can use this information to present the children in a paged UI
    /// and fetch them in chunks.
    pub named_variables: Option<usize>,
    /// The usize of indexed child variables.
    /// The client can use this information to present the children in a paged UI
    /// and fetch them in chunks.
    pub indexed_variables: Option<usize>,
    /// The memory reference for the variable if the variable represents executable
    /// code, such as a function pointer.
    /// This attribute is only required if the corresponding capability
    /// `supportsMemoryReferences` is true.
    pub memory_reference: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RunInTerminalRequestArgumentsKind {
    Integrated,
    External,
}

impl FromStr for RunInTerminalRequestArgumentsKind {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "integrated" => Ok(RunInTerminalRequestArgumentsKind::Integrated),
            "external" => Ok(RunInTerminalRequestArgumentsKind::External),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "RunInTerminalRequestArgumentsKind".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for RunInTerminalRequestArgumentsKind {
    fn to_string(&self) -> String {
        match &self {
            RunInTerminalRequestArgumentsKind::Integrated => "integrated",
            RunInTerminalRequestArgumentsKind::External => "external",
        }
        .to_string()
    }
}

tostr_ser! { RunInTerminalRequestArgumentsKind }

#[derive(Debug, Clone)]
pub enum StartDebuggingRequestKind {
    Launch,
    Attach,
}

impl FromStr for StartDebuggingRequestKind {
    type Err = DeserializationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "launch" => Ok(StartDebuggingRequestKind::Launch),
            "attach" => Ok(StartDebuggingRequestKind::Attach),
            other => Err(DeserializationError::StringToEnumParseError {
                enum_name: "StartDebuggingRequestArgumentsRequest".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

impl ToString for StartDebuggingRequestKind {
    fn to_string(&self) -> String {
        match &self {
            StartDebuggingRequestKind::Launch => "launch",
            StartDebuggingRequestKind::Attach => "attach",
        }
        .to_string()
    }
}

fromstr_deser! { StartDebuggingRequestKind }
tostr_ser! { StartDebuggingRequestKind }
