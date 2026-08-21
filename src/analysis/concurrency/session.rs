//! Advisory binary-session sketches from typed Rholang CPG evidence.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashSet;

use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNodeKind, LiteralKind, MsgKind, NodeId, SourceRange,
    TypeInfo,
};

use super::events::{extract_events, resolve_event_channel_operand, ChannelId};

/// The communication direction of one protocol action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SessionDirection {
    /// Produce a message on the channel.
    Send,
    /// Consume or observe a message from the channel.
    Receive,
}

impl SessionDirection {
    /// Returns a stable human-readable label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Receive => "receive",
        }
    }
}

/// A deliberately shallow payload-type sketch.
///
/// Literal categories preserve frontend evidence without depending on a
/// language type checker. [`Self::Named`] records explicit CPG type metadata;
/// [`Self::Unknown`] records missing or compound evidence without guessing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SessionPayloadType {
    /// Integer literal.
    Integer,
    /// Floating-point literal.
    Float,
    /// String literal.
    String,
    /// Character literal.
    Character,
    /// Boolean literal.
    Boolean,
    /// Null, unit, or inert-process literal.
    Null,
    /// Array, list, set, tuple, or collection literal.
    Array,
    /// Object, map, or path-map literal.
    Object,
    /// Regular-expression literal.
    Regex,
    /// Explicit type name retained from [`TypeInfo`].
    Named(String),
    /// The CPG does not establish a shallow payload type.
    Unknown,
}

impl SessionPayloadType {
    /// Returns a stable human-readable label.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
            Self::String => "string",
            Self::Character => "character",
            Self::Boolean => "boolean",
            Self::Null => "null",
            Self::Array => "array",
            Self::Object => "object",
            Self::Regex => "regex",
            Self::Named(name) => name,
            Self::Unknown => "unknown",
        }
    }
}

/// One ordered action in a process endpoint's protocol sketch.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionAction {
    /// Communication direction derived from [`MsgKind`].
    pub direction: SessionDirection,
    /// Exact normalized message operation retained for auditability.
    pub message_kind: MsgKind,
    /// Shallow payload types in source order.
    pub payload_types: Vec<SessionPayloadType>,
    /// In-graph event node supporting this action.
    pub site: NodeId,
    /// Source range copied from `site`.
    pub range: SourceRange,
}

/// The ordered protocol sketch inferred for one process and channel.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionEndpoint {
    /// Top-level process/function containing the actions.
    pub process: NodeId,
    /// Actions ordered by stable event-site identity.
    pub actions: Vec<SessionAction>,
}

/// Advisory result of comparing a channel's endpoint sketches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SessionConformance {
    /// Exactly two linear endpoints are direction- and type-dual.
    Conformant,
    /// Typed evidence proves a direction, arity, type, or length mismatch.
    Nonconformant,
    /// Available evidence is insufficient for a binary linear verdict.
    Inconclusive,
}

impl SessionConformance {
    /// Returns a stable human-readable label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Conformant => "conformant",
            Self::Nonconformant => "nonconformant",
            Self::Inconclusive => "inconclusive",
        }
    }
}

/// Stable category for a session-conformance diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SessionIssueKind {
    /// Binary comparison requires exactly two participating processes.
    EndpointCardinality,
    /// Endpoint action counts differ.
    LengthMismatch,
    /// Corresponding actions have the same direction instead of opposite ones.
    DirectionMismatch,
    /// Corresponding actions carry different payload counts.
    PayloadArityMismatch,
    /// Corresponding known payload types differ.
    PayloadTypeMismatch,
    /// At least one payload type is not established by the CPG.
    UnknownPayloadType,
    /// Persistent or peeking receive semantics are outside linear duality.
    NonLinearReceive,
}

impl SessionIssueKind {
    /// Returns a stable machine- and human-readable label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EndpointCardinality => "endpoint_cardinality",
            Self::LengthMismatch => "length_mismatch",
            Self::DirectionMismatch => "direction_mismatch",
            Self::PayloadArityMismatch => "payload_arity_mismatch",
            Self::PayloadTypeMismatch => "payload_type_mismatch",
            Self::UnknownPayloadType => "unknown_payload_type",
            Self::NonLinearReceive => "non_linear_receive",
        }
    }
}

/// Evidence explaining why a channel sketch is not proven conformant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionIssue {
    /// Stable issue category.
    pub kind: SessionIssueKind,
    /// Zero-based action position, when the issue concerns paired actions.
    pub action_index: Option<usize>,
    /// Participating process nodes, sorted and confined to the input graph.
    pub processes: Vec<NodeId>,
    /// Event sites supporting the issue, sorted and confined to the input graph.
    pub sites: Vec<NodeId>,
    /// Concise explanation suitable for diagnostics.
    pub detail: String,
}

/// Per-channel behavioral sketch and advisory binary-duality verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionSketch {
    /// Stable channel identity: the resolved channel-definition node.
    pub channel: ChannelId,
    /// Participating process endpoints, sorted by process node id.
    pub endpoints: Vec<SessionEndpoint>,
    /// Advisory duality verdict.
    pub conformance: SessionConformance,
    /// Deterministically ordered evidence for non-conformance or uncertainty.
    pub issues: Vec<SessionIssue>,
}

/// Infers advisory per-channel session sketches from normalized Rholang CPG evidence.
///
/// The analysis is deliberately read-only and source-independent. It groups
/// resolved message events by channel and top-level process, retains their
/// stable event order, then compares exactly two endpoint traces for opposite
/// directions and equal known payload types. Missing types, non-linear receives,
/// and non-binary channel use produce [`SessionConformance::Inconclusive`]; a
/// concrete mismatch produces [`SessionConformance::Nonconformant`].
pub fn session_sketches(cpg: &CodePropertyGraph) -> Vec<SessionSketch> {
    let mut by_channel: BTreeMap<ChannelId, BTreeMap<NodeId, Vec<SessionAction>>> = BTreeMap::new();

    for (process, events) in extract_events(cpg) {
        for event in events {
            let Some(channel) = event.channel else {
                continue;
            };
            let direction = if event.kind.is_send() {
                SessionDirection::Send
            } else if matches!(
                event.kind,
                MsgKind::Recv | MsgKind::RecvPersistent | MsgKind::Peek
            ) {
                SessionDirection::Receive
            } else {
                continue;
            };
            by_channel
                .entry(channel)
                .or_default()
                .entry(process)
                .or_default()
                .push(SessionAction {
                    direction,
                    message_kind: event.kind,
                    payload_types: payload_types(cpg, event.site),
                    site: event.site,
                    range: event.range,
                });
        }
    }

    by_channel
        .into_iter()
        .map(|(channel, by_process)| {
            let endpoints = by_process
                .into_iter()
                .map(|(process, mut actions)| {
                    actions.sort_unstable_by_key(|action| action.site);
                    SessionEndpoint { process, actions }
                })
                .collect::<Vec<_>>();
            let (conformance, issues) = compare_endpoints(&endpoints);
            SessionSketch {
                channel,
                endpoints,
                conformance,
                issues,
            }
        })
        .collect()
}

fn compare_endpoints(endpoints: &[SessionEndpoint]) -> (SessionConformance, Vec<SessionIssue>) {
    let processes = endpoints
        .iter()
        .map(|endpoint| endpoint.process)
        .collect::<Vec<_>>();
    if endpoints.len() != 2 {
        return (
            SessionConformance::Inconclusive,
            vec![SessionIssue {
                kind: SessionIssueKind::EndpointCardinality,
                action_index: None,
                processes,
                sites: endpoint_sites(endpoints),
                detail: format!(
                    "binary session duality requires exactly two process endpoints; observed {}",
                    endpoints.len()
                ),
            }],
        );
    }

    let left = &endpoints[0];
    let right = &endpoints[1];
    let mut issues = Vec::new();
    let mut hard_mismatch = false;
    let mut uncertain = false;

    if left.actions.len() != right.actions.len() {
        hard_mismatch = true;
        issues.push(SessionIssue {
            kind: SessionIssueKind::LengthMismatch,
            action_index: None,
            processes: processes.clone(),
            sites: endpoint_sites(endpoints),
            detail: format!(
                "endpoint traces contain {} and {} actions",
                left.actions.len(),
                right.actions.len()
            ),
        });
    }

    for (index, (left_action, right_action)) in left.actions.iter().zip(&right.actions).enumerate()
    {
        let sites = sorted_pair(left_action.site, right_action.site);
        if left_action.direction == right_action.direction {
            hard_mismatch = true;
            issues.push(SessionIssue {
                kind: SessionIssueKind::DirectionMismatch,
                action_index: Some(index),
                processes: processes.clone(),
                sites: sites.clone(),
                detail: format!(
                    "action {index} is {} at both endpoints",
                    left_action.direction.as_str()
                ),
            });
        }

        if !left_action.message_kind.is_linear_receive()
            && left_action.direction == SessionDirection::Receive
            || !right_action.message_kind.is_linear_receive()
                && right_action.direction == SessionDirection::Receive
        {
            uncertain = true;
            issues.push(SessionIssue {
                kind: SessionIssueKind::NonLinearReceive,
                action_index: Some(index),
                processes: processes.clone(),
                sites: sites.clone(),
                detail: format!(
                    "action {index} includes {:?}/{:?}; persistent and peeking receives are outside linear binary duality",
                    left_action.message_kind, right_action.message_kind
                ),
            });
        }

        if left_action.payload_types.len() != right_action.payload_types.len() {
            hard_mismatch = true;
            issues.push(SessionIssue {
                kind: SessionIssueKind::PayloadArityMismatch,
                action_index: Some(index),
                processes: processes.clone(),
                sites: sites.clone(),
                detail: format!(
                    "action {index} carries {} and {} payload values",
                    left_action.payload_types.len(),
                    right_action.payload_types.len()
                ),
            });
        }

        for (position, (left_type, right_type)) in left_action
            .payload_types
            .iter()
            .zip(&right_action.payload_types)
            .enumerate()
        {
            if matches!(left_type, SessionPayloadType::Unknown)
                || matches!(right_type, SessionPayloadType::Unknown)
            {
                uncertain = true;
                issues.push(SessionIssue {
                    kind: SessionIssueKind::UnknownPayloadType,
                    action_index: Some(index),
                    processes: processes.clone(),
                    sites: sites.clone(),
                    detail: format!(
                        "payload {position} of action {index} is not typed at both endpoints"
                    ),
                });
            } else if left_type != right_type {
                hard_mismatch = true;
                issues.push(SessionIssue {
                    kind: SessionIssueKind::PayloadTypeMismatch,
                    action_index: Some(index),
                    processes: processes.clone(),
                    sites: sites.clone(),
                    detail: format!(
                        "payload {position} of action {index} has types {} and {}",
                        left_type.as_str(),
                        right_type.as_str()
                    ),
                });
            }
        }
    }

    let conformance = if hard_mismatch {
        SessionConformance::Nonconformant
    } else if uncertain {
        SessionConformance::Inconclusive
    } else {
        SessionConformance::Conformant
    };
    (conformance, issues)
}

fn endpoint_sites(endpoints: &[SessionEndpoint]) -> Vec<NodeId> {
    let mut sites = endpoints
        .iter()
        .flat_map(|endpoint| endpoint.actions.iter().map(|action| action.site))
        .collect::<Vec<_>>();
    sites.sort_unstable();
    sites.dedup();
    sites
}

fn sorted_pair(first: NodeId, second: NodeId) -> Vec<NodeId> {
    if first <= second {
        vec![first, second]
    } else {
        vec![second, first]
    }
}

fn payload_types(cpg: &CodePropertyGraph, site: NodeId) -> Vec<SessionPayloadType> {
    let channel_operand = resolve_event_channel_operand(cpg, site).map(|(_, operand)| operand);
    let mut visited = FxHashSet::default();
    visited.insert(site);
    let mut stack = cpg.ast_children(site).to_vec();
    let mut candidates = Vec::new();

    while let Some(candidate) = stack.pop() {
        if !visited.insert(candidate) || Some(candidate) == channel_operand {
            continue;
        }
        let Some(node) = cpg.node(candidate) else {
            continue;
        };
        if node.message_kind.is_some()
            || node.lock_kind.is_some()
            || matches!(node.kind, CpgNodeKind::Function { .. })
        {
            continue;
        }

        if let Some(payload_type) = payload_type(cpg, candidate) {
            candidates.push((node.range.start, node.range.end, candidate, payload_type));
            continue;
        }

        let mut children = cpg.ast_children(candidate).to_vec();
        children.reverse();
        stack.extend(children);
    }

    candidates.sort_unstable_by_key(|candidate| (candidate.0, candidate.1, candidate.2));
    candidates
        .into_iter()
        .map(|(_, _, _, payload_type)| payload_type)
        .collect()
}

fn payload_type(cpg: &CodePropertyGraph, node: NodeId) -> Option<SessionPayloadType> {
    let kind = &cpg.node(node)?.kind;
    match kind {
        CpgNodeKind::Literal { kind } => Some(literal_payload_type(kind)),
        CpgNodeKind::Variable { var_type, .. } => Some(named_or_unknown(var_type.as_deref())),
        CpgNodeKind::Parameter { param_type, .. } => Some(named_or_unknown(param_type.as_deref())),
        CpgNodeKind::Field { field_type, .. } => Some(named_or_unknown(field_type.as_deref())),
        CpgNodeKind::TypeAnnotation { type_info } => {
            Some(type_info_payload_type(type_info.as_ref()))
        }
        CpgNodeKind::Identifier { .. } => Some(
            referenced_type(cpg, node)
                .map(type_info_payload_type)
                .unwrap_or(SessionPayloadType::Unknown),
        ),
        CpgNodeKind::BinaryOp { .. }
        | CpgNodeKind::UnaryOp { .. }
        | CpgNodeKind::Assignment { .. }
        | CpgNodeKind::Call { .. }
        | CpgNodeKind::MemberAccess { .. }
        | CpgNodeKind::IndexAccess
        | CpgNodeKind::Lambda { .. }
        | CpgNodeKind::Await
        | CpgNodeKind::Yield => Some(SessionPayloadType::Unknown),
        _ => None,
    }
}

fn literal_payload_type(kind: &LiteralKind) -> SessionPayloadType {
    match kind {
        LiteralKind::Integer(_) => SessionPayloadType::Integer,
        LiteralKind::Float(_) => SessionPayloadType::Float,
        LiteralKind::String(_) => SessionPayloadType::String,
        LiteralKind::Char(_) => SessionPayloadType::Character,
        LiteralKind::Bool(_) => SessionPayloadType::Boolean,
        LiteralKind::Null => SessionPayloadType::Null,
        LiteralKind::Array => SessionPayloadType::Array,
        LiteralKind::Object => SessionPayloadType::Object,
        LiteralKind::Regex(_) => SessionPayloadType::Regex,
    }
}

fn named_or_unknown(type_info: Option<&TypeInfo>) -> SessionPayloadType {
    type_info
        .map(type_info_payload_type)
        .unwrap_or(SessionPayloadType::Unknown)
}

fn type_info_payload_type(type_info: &TypeInfo) -> SessionPayloadType {
    if type_info.is_reference || type_info.is_mutable || !type_info.generics.is_empty() {
        return SessionPayloadType::Named(render_type_info(type_info));
    }
    match type_info.name.trim().to_ascii_lowercase().as_str() {
        "int" | "integer" | "bigint" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8"
        | "u16" | "u32" | "u64" | "u128" | "usize" => SessionPayloadType::Integer,
        "float" | "double" | "bigrat" | "fixedpoint" | "f32" | "f64" => SessionPayloadType::Float,
        "string" | "str" => SessionPayloadType::String,
        "char" | "character" => SessionPayloadType::Character,
        "bool" | "boolean" => SessionPayloadType::Boolean,
        "null" | "nil" | "unit" => SessionPayloadType::Null,
        "array" | "list" | "set" | "tuple" | "collection" => SessionPayloadType::Array,
        "object" | "map" | "pathmap" => SessionPayloadType::Object,
        "regex" | "regexp" => SessionPayloadType::Regex,
        _ => SessionPayloadType::Named(type_info.name.to_string()),
    }
}

fn render_type_info(type_info: &TypeInfo) -> String {
    let mut rendered = if type_info.is_reference {
        if type_info.is_mutable {
            "&mut ".to_owned()
        } else {
            "&".to_owned()
        }
    } else if type_info.is_mutable {
        "mut ".to_owned()
    } else {
        String::new()
    };
    rendered.push_str(&type_info.name);
    if !type_info.generics.is_empty() {
        rendered.push('<');
        for (index, generic) in type_info.generics.iter().enumerate() {
            if index > 0 {
                rendered.push_str(", ");
            }
            rendered.push_str(generic);
        }
        rendered.push('>');
    }
    rendered
}

fn referenced_type(cpg: &CodePropertyGraph, identifier: NodeId) -> Option<&TypeInfo> {
    let mut definitions = BTreeSet::new();
    if let Some(CpgNodeKind::Identifier {
        definition: Some(definition),
        ..
    }) = cpg.node(identifier).map(|node| &node.kind)
    {
        definitions.insert(*definition);
    }
    definitions.extend(cpg.reaching_definitions(identifier));
    for edge in cpg.outgoing_edges(identifier) {
        if edge.kind == CpgEdgeKind::Reference {
            definitions.insert(edge.target);
        }
    }
    for edge in cpg.incoming_edges(identifier) {
        if edge.kind == CpgEdgeKind::Reference {
            definitions.insert(edge.source);
        }
    }

    definitions.into_iter().find_map(|definition| {
        let definition = cpg.node(definition)?;
        match &definition.kind {
            CpgNodeKind::Variable { var_type, .. } => var_type.as_deref(),
            CpgNodeKind::Parameter { param_type, .. } => param_type.as_deref(),
            CpgNodeKind::Field { field_type, .. } => field_type.as_deref(),
            CpgNodeKind::TypeAnnotation { type_info } => Some(type_info.as_ref()),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests;
