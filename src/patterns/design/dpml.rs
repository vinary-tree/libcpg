//! DPML (Design Pattern Markup Language) template support.
//!
//! DPML is a declarative format for defining design patterns using
//! roles and constraints. It supports both YAML and TOML syntax.
//!
//! ## Example YAML Format
//!
//! ```yaml
//! name: Singleton
//! description: Ensures a class has only one instance
//! category: Creational
//!
//! roles:
//!   - id: singleton_class
//!     type: class
//!     cardinality: "1"
//!   - id: instance_field
//!     type: field
//!     cardinality: "1"
//!   - id: get_instance
//!     type: method
//!     cardinality: "1"
//!
//! relationships:
//!   - source: singleton_class
//!     target: instance_field
//!     type: contains
//!   - source: singleton_class
//!     target: get_instance
//!     type: contains
//! ```

#[cfg(any(feature = "serde", feature = "design-patterns"))]
use serde::{Deserialize, Serialize};

use crate::pattern::{PatternTemplate, NodeConstraint, EdgeConstraint, NodeKindMatcher, NodeKindTag, EdgeKindMatcher};
use rustc_hash::FxHashMap;

/// DPML template for pattern matching.
#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "serde", feature = "design-patterns"), derive(Serialize, Deserialize))]
pub struct DpmlTemplate {
    /// Template name.
    pub name: String,
    /// Pattern description.
    #[cfg_attr(any(feature = "serde", feature = "design-patterns"), serde(default))]
    pub description: String,
    /// Pattern category (Creational, Structural, Behavioral).
    #[cfg_attr(any(feature = "serde", feature = "design-patterns"), serde(default))]
    pub category: String,
    /// Role definitions.
    #[cfg_attr(any(feature = "serde", feature = "design-patterns"), serde(default))]
    pub roles: Vec<DpmlRole>,
    /// Constraints/relationships between roles.
    #[cfg_attr(any(feature = "serde", feature = "design-patterns"), serde(default, alias = "constraints"))]
    pub relationships: Vec<DpmlConstraint>,
}

impl DpmlTemplate {
    /// Creates a new DPML template.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            category: String::new(),
            roles: Vec::new(),
            relationships: Vec::new(),
        }
    }

    /// Sets the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Sets the category.
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// Adds a role.
    pub fn with_role(mut self, role: DpmlRole) -> Self {
        self.roles.push(role);
        self
    }

    /// Adds a relationship/constraint.
    pub fn with_relationship(mut self, constraint: DpmlConstraint) -> Self {
        self.relationships.push(constraint);
        self
    }

    /// Parses a DPML template from a string.
    ///
    /// Automatically detects YAML or TOML format based on content.
    #[cfg(feature = "design-patterns")]
    pub fn parse(content: &str) -> Result<Self, DpmlError> {
        // Try YAML first (more common for design patterns)
        if let Ok(template) = Self::parse_yaml(content) {
            return Ok(template);
        }

        // Try TOML
        if let Ok(template) = Self::parse_toml(content) {
            return Ok(template);
        }

        Err(DpmlError::InvalidSyntax(
            "Content is neither valid YAML nor TOML".to_string(),
        ))
    }

    /// Parses a DPML template when design-patterns feature is disabled.
    #[cfg(not(feature = "design-patterns"))]
    pub fn parse(_content: &str) -> Result<Self, DpmlError> {
        Err(DpmlError::FeatureDisabled(
            "Enable 'design-patterns' feature for DPML parsing".to_string(),
        ))
    }

    /// Parses YAML content into a DPML template.
    #[cfg(feature = "design-patterns")]
    pub fn parse_yaml(content: &str) -> Result<Self, DpmlError> {
        serde_yaml::from_str(content)
            .map_err(|e| DpmlError::YamlError(e.to_string()))
    }

    /// Parses TOML content into a DPML template.
    #[cfg(feature = "design-patterns")]
    pub fn parse_toml(content: &str) -> Result<Self, DpmlError> {
        // Parse TOML document
        let doc: toml_edit::DocumentMut = content.parse()
            .map_err(|e: toml_edit::TomlError| DpmlError::TomlError(e.to_string()))?;

        // Extract fields from the document
        let name = doc.get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DpmlError::MissingField("name".to_string()))?
            .to_string();

        let description = doc.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let category = doc.get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Parse roles
        let mut roles = Vec::new();
        if let Some(roles_arr) = doc.get("roles").and_then(|v| v.as_array_of_tables()) {
            for role_table in roles_arr {
                let id = role_table.get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let role_type = role_table.get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let cardinality = role_table.get("cardinality")
                    .and_then(|v| v.as_str())
                    .unwrap_or("1")
                    .to_string();

                roles.push(DpmlRole { id, role_type, cardinality });
            }
        }

        // Parse relationships
        let mut relationships = Vec::new();
        let rel_key = if doc.contains_key("relationships") { "relationships" } else { "constraints" };
        if let Some(rels_arr) = doc.get(rel_key).and_then(|v| v.as_array_of_tables()) {
            for rel_table in rels_arr {
                let source = rel_table.get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let target = rel_table.get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let constraint_type = rel_table.get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("contains")
                    .to_string();

                relationships.push(DpmlConstraint { source, target, constraint_type });
            }
        }

        Ok(Self {
            name,
            description,
            category,
            roles,
            relationships,
        })
    }

    /// Validates the template.
    ///
    /// Checks that:
    /// - All relationship references point to existing roles
    /// - Role IDs are unique
    /// - Required fields are present
    pub fn validate(&self) -> Result<(), DpmlError> {
        // Check name is not empty
        if self.name.is_empty() {
            return Err(DpmlError::MissingField("name".to_string()));
        }

        // Collect role IDs
        let mut role_ids: FxHashMap<&str, usize> = FxHashMap::default();
        for (idx, role) in self.roles.iter().enumerate() {
            if role.id.is_empty() {
                return Err(DpmlError::InvalidRole(format!(
                    "Role at index {} has empty id",
                    idx
                )));
            }
            if role_ids.contains_key(role.id.as_str()) {
                return Err(DpmlError::DuplicateRole(role.id.clone()));
            }
            role_ids.insert(&role.id, idx);
        }

        // Validate relationships reference existing roles
        for (idx, rel) in self.relationships.iter().enumerate() {
            if !role_ids.contains_key(rel.source.as_str()) {
                return Err(DpmlError::InvalidRelationship(format!(
                    "Relationship {} references unknown source role: {}",
                    idx, rel.source
                )));
            }
            if !role_ids.contains_key(rel.target.as_str()) {
                return Err(DpmlError::InvalidRelationship(format!(
                    "Relationship {} references unknown target role: {}",
                    idx, rel.target
                )));
            }
        }

        Ok(())
    }

    /// Converts this DPML template to a PatternTemplate for matching.
    pub fn to_pattern_template(&self) -> Result<PatternTemplate, DpmlError> {
        // Validate first
        self.validate()?;

        let mut template = PatternTemplate::new(&self.name, &self.description);

        // Map role IDs to node indices
        let mut role_to_index: FxHashMap<&str, usize> = FxHashMap::default();

        // Create node constraints from roles
        for (idx, role) in self.roles.iter().enumerate() {
            let kind_matcher = role_type_to_node_matcher(&role.role_type);
            let constraint = NodeConstraint::new(idx).with_kind(kind_matcher);
            template = template.with_node(constraint);
            role_to_index.insert(&role.id, idx);
        }

        // Create edge constraints from relationships
        for rel in &self.relationships {
            let source_idx = role_to_index[rel.source.as_str()];
            let target_idx = role_to_index[rel.target.as_str()];
            let edge_matcher = constraint_type_to_edge_matcher(&rel.constraint_type);
            let constraint = EdgeConstraint::new(source_idx, target_idx)
                .with_kind(edge_matcher);
            template = template.with_edge(constraint);
        }

        Ok(template)
    }
}

/// Converts a DPML role type string to a NodeKindMatcher.
fn role_type_to_node_matcher(role_type: &str) -> NodeKindMatcher {
    match role_type.to_lowercase().as_str() {
        "class" => NodeKindMatcher::Exact(NodeKindTag::Class),
        "struct" => NodeKindMatcher::Exact(NodeKindTag::Struct),
        "interface" | "trait" => NodeKindMatcher::Exact(NodeKindTag::Trait),
        "method" | "function" => NodeKindMatcher::Exact(NodeKindTag::Function),
        "field" => NodeKindMatcher::Exact(NodeKindTag::Field),
        "parameter" => NodeKindMatcher::Exact(NodeKindTag::Parameter),
        "variable" => NodeKindMatcher::Exact(NodeKindTag::Variable),
        "call" => NodeKindMatcher::Exact(NodeKindTag::Call),
        "block" => NodeKindMatcher::Exact(NodeKindTag::Block),
        "declaration" => NodeKindMatcher::AnyDeclaration,
        "expression" => NodeKindMatcher::AnyExpression,
        "statement" => NodeKindMatcher::AnyStatement,
        "loop" => NodeKindMatcher::AnyOf(vec![
            NodeKindTag::While,
            NodeKindTag::For,
            NodeKindTag::Loop,
        ]),
        "while" => NodeKindMatcher::Exact(NodeKindTag::While),
        "for" => NodeKindMatcher::Exact(NodeKindTag::For),
        _ => NodeKindMatcher::Any,
    }
}

/// Converts a DPML constraint type string to an EdgeKindMatcher.
fn constraint_type_to_edge_matcher(constraint_type: &str) -> EdgeKindMatcher {
    match constraint_type.to_lowercase().as_str() {
        "contains" | "has" | "ast" | "child" => EdgeKindMatcher::AnyAst,
        "calls" | "invokes" => EdgeKindMatcher::AnyCall,
        "uses" | "dataflow" | "depends" => EdgeKindMatcher::AnyDfg,
        "flows" | "control" | "next" => EdgeKindMatcher::AnyCfg,
        _ => EdgeKindMatcher::Any,
    }
}

/// A role in a DPML template.
#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "serde", feature = "design-patterns"), derive(Serialize, Deserialize))]
pub struct DpmlRole {
    /// Role identifier.
    pub id: String,
    /// Role type (class, interface, method, etc.).
    #[cfg_attr(any(feature = "serde", feature = "design-patterns"), serde(rename = "type"))]
    pub role_type: String,
    /// Cardinality (e.g., "1", "1..*").
    #[cfg_attr(any(feature = "serde", feature = "design-patterns"), serde(default = "default_cardinality"))]
    pub cardinality: String,
}

fn default_cardinality() -> String {
    "1".to_string()
}

impl DpmlRole {
    /// Creates a new role.
    pub fn new(id: impl Into<String>, role_type: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role_type: role_type.into(),
            cardinality: "1".to_string(),
        }
    }

    /// Sets the cardinality.
    pub fn with_cardinality(mut self, cardinality: impl Into<String>) -> Self {
        self.cardinality = cardinality.into();
        self
    }
}

/// A constraint between roles.
#[derive(Debug, Clone)]
#[cfg_attr(any(feature = "serde", feature = "design-patterns"), derive(Serialize, Deserialize))]
pub struct DpmlConstraint {
    /// Source role ID.
    pub source: String,
    /// Target role ID.
    pub target: String,
    /// Constraint type.
    #[cfg_attr(any(feature = "serde", feature = "design-patterns"), serde(rename = "type", default = "default_constraint_type"))]
    pub constraint_type: String,
}

fn default_constraint_type() -> String {
    "contains".to_string()
}

impl DpmlConstraint {
    /// Creates a new constraint.
    pub fn new(source: impl Into<String>, target: impl Into<String>, constraint_type: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            constraint_type: constraint_type.into(),
        }
    }
}

/// DPML parsing errors.
#[derive(Debug)]
pub enum DpmlError {
    /// Feature not enabled.
    FeatureDisabled(String),
    /// YAML parsing error.
    YamlError(String),
    /// TOML parsing error.
    TomlError(String),
    /// Missing required field.
    MissingField(String),
    /// Invalid role definition.
    InvalidRole(String),
    /// Duplicate role ID.
    DuplicateRole(String),
    /// Invalid relationship.
    InvalidRelationship(String),
    /// Invalid syntax.
    InvalidSyntax(String),
}

impl std::fmt::Display for DpmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FeatureDisabled(msg) => write!(f, "Feature disabled: {}", msg),
            Self::YamlError(msg) => write!(f, "YAML error: {}", msg),
            Self::TomlError(msg) => write!(f, "TOML error: {}", msg),
            Self::MissingField(field) => write!(f, "Missing required field: {}", field),
            Self::InvalidRole(msg) => write!(f, "Invalid role: {}", msg),
            Self::DuplicateRole(id) => write!(f, "Duplicate role ID: {}", id),
            Self::InvalidRelationship(msg) => write!(f, "Invalid relationship: {}", msg),
            Self::InvalidSyntax(msg) => write!(f, "Invalid syntax: {}", msg),
        }
    }
}

impl std::error::Error for DpmlError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpml_template_builder() {
        let template = DpmlTemplate::new("Singleton")
            .with_description("Ensures single instance")
            .with_category("Creational")
            .with_role(DpmlRole::new("singleton_class", "class"))
            .with_role(DpmlRole::new("instance_field", "field"))
            .with_relationship(DpmlConstraint::new("singleton_class", "instance_field", "contains"));

        assert_eq!(template.name, "Singleton");
        assert_eq!(template.roles.len(), 2);
        assert_eq!(template.relationships.len(), 1);
    }

    #[test]
    fn test_validate_empty_name() {
        let template = DpmlTemplate::new("");
        assert!(matches!(template.validate(), Err(DpmlError::MissingField(_))));
    }

    #[test]
    fn test_validate_duplicate_role() {
        let template = DpmlTemplate::new("Test")
            .with_role(DpmlRole::new("role1", "class"))
            .with_role(DpmlRole::new("role1", "method"));
        assert!(matches!(template.validate(), Err(DpmlError::DuplicateRole(_))));
    }

    #[test]
    fn test_validate_invalid_relationship() {
        let template = DpmlTemplate::new("Test")
            .with_role(DpmlRole::new("role1", "class"))
            .with_relationship(DpmlConstraint::new("role1", "nonexistent", "contains"));
        assert!(matches!(template.validate(), Err(DpmlError::InvalidRelationship(_))));
    }

    #[test]
    fn test_to_pattern_template() {
        let dpml = DpmlTemplate::new("Observer")
            .with_description("Observer pattern")
            .with_role(DpmlRole::new("subject", "class"))
            .with_role(DpmlRole::new("observer", "trait"))
            .with_role(DpmlRole::new("notify_method", "method"))
            .with_relationship(DpmlConstraint::new("subject", "notify_method", "contains"))
            .with_relationship(DpmlConstraint::new("subject", "observer", "uses"));

        let pattern = dpml.to_pattern_template().expect("Should convert successfully");

        assert_eq!(pattern.name, "Observer");
        assert_eq!(pattern.node_constraints.len(), 3);
        assert_eq!(pattern.edge_constraints.len(), 2);
    }

    #[cfg(feature = "design-patterns")]
    #[test]
    fn test_parse_yaml() {
        let yaml = r#"
name: Factory
description: Factory method pattern
category: Creational

roles:
  - id: factory
    type: class
  - id: create_method
    type: method
    cardinality: "1"
  - id: product
    type: interface

relationships:
  - source: factory
    target: create_method
    type: contains
  - source: create_method
    target: product
    type: calls
"#;

        let template = DpmlTemplate::parse_yaml(yaml).expect("Should parse YAML");
        assert_eq!(template.name, "Factory");
        assert_eq!(template.category, "Creational");
        assert_eq!(template.roles.len(), 3);
        assert_eq!(template.relationships.len(), 2);

        // Verify it validates
        template.validate().expect("Should validate");

        // Verify conversion to pattern template
        let pattern = template.to_pattern_template().expect("Should convert");
        assert_eq!(pattern.node_constraints.len(), 3);
    }

    #[test]
    fn test_dpml_error_display() {
        let err = DpmlError::MissingField("name".to_string());
        assert!(err.to_string().contains("name"));
    }

    #[test]
    fn test_role_and_constraint_builders() {
        let role = DpmlRole::new("subject", "class").with_cardinality("1..*");
        assert_eq!(role.id, "subject");
        assert_eq!(role.role_type, "class");
        assert_eq!(role.cardinality, "1..*");

        // Default cardinality is "1".
        let default_role = DpmlRole::new("observer", "trait");
        assert_eq!(default_role.cardinality, "1");

        let constraint = DpmlConstraint::new("subject", "observer", "uses");
        assert_eq!(constraint.source, "subject");
        assert_eq!(constraint.target, "observer");
        assert_eq!(constraint.constraint_type, "uses");
    }

    #[cfg(feature = "design-patterns")]
    #[test]
    fn test_parse_toml() {
        let toml = r#"
name = "Singleton"
description = "One instance"
category = "Creational"

[[roles]]
id = "cls"
type = "class"

[[roles]]
id = "field"
type = "field"
cardinality = "1"

[[relationships]]
source = "cls"
target = "field"
type = "contains"
"#;

        let template = DpmlTemplate::parse_toml(toml).expect("Should parse TOML");
        assert_eq!(template.name, "Singleton");
        assert_eq!(template.description, "One instance");
        assert_eq!(template.category, "Creational");
        assert_eq!(template.roles.len(), 2);
        assert_eq!(template.relationships.len(), 1);
        assert_eq!(template.relationships[0].constraint_type, "contains");

        template.validate().expect("Should validate");
        let pattern = template.to_pattern_template().expect("Should convert");
        assert_eq!(pattern.node_constraints.len(), 2);
        assert_eq!(pattern.edge_constraints.len(), 1);
    }

    /// TOML also accepts `[[constraints]]` as an alias for `[[relationships]]`.
    #[cfg(feature = "design-patterns")]
    #[test]
    fn test_parse_toml_constraints_alias() {
        let toml = r#"
name = "Aliased"

[[roles]]
id = "a"
type = "class"

[[roles]]
id = "b"
type = "field"

[[constraints]]
source = "a"
target = "b"
type = "contains"
"#;
        let template = DpmlTemplate::parse_toml(toml).expect("Should parse TOML");
        assert_eq!(template.relationships.len(), 1);
    }

    #[cfg(feature = "design-patterns")]
    #[test]
    fn test_parse_autodetect_yaml_and_toml() {
        // YAML content -> YAML branch.
        let yaml = "name: FromYaml\nroles:\n  - id: a\n    type: class\n";
        let from_yaml = DpmlTemplate::parse(yaml).expect("auto-detect YAML");
        assert_eq!(from_yaml.name, "FromYaml");
        assert_eq!(from_yaml.roles.len(), 1);

        // TOML content -> falls through to the TOML branch.
        let toml = "name = \"FromToml\"\n\n[[roles]]\nid = \"a\"\ntype = \"class\"\n";
        let from_toml = DpmlTemplate::parse(toml).expect("auto-detect TOML");
        assert_eq!(from_toml.name, "FromToml");
        assert_eq!(from_toml.roles.len(), 1);
    }

    #[cfg(feature = "design-patterns")]
    #[test]
    fn test_parse_rejects_garbage() {
        // `:` makes serde_yaml try (and fail on the wrong shape); TOML also fails.
        let result = DpmlTemplate::parse("%%% not : valid = anything [[[");
        assert!(result.is_err());
    }
}

#[cfg(all(test, feature = "design-patterns"))]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    fn arb_role_type() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("class"),
            Just("struct"),
            Just("trait"),
            Just("interface"),
            Just("method"),
            Just("function"),
            Just("field"),
            Just("parameter"),
            Just("variable"),
            Just("block"),
        ]
        .prop_map(|s| s.to_string())
    }

    fn arb_constraint_type() -> impl Strategy<Value = String> {
        prop_oneof![Just("contains"), Just("calls"), Just("uses"), Just("flows")]
            .prop_map(|s| s.to_string())
    }

    /// A valid DPML template: non-empty name, roles with unique ids, and
    /// relationships that only reference existing roles.
    fn arb_valid_dpml() -> impl Strategy<Value = DpmlTemplate> {
        (
            arb_name(),
            prop::collection::vec((arb_name(), arb_role_type()), 1..=5),
        )
            .prop_flat_map(|(name, raw_roles)| {
                // Keep the first occurrence of each role id (ids must be unique).
                let mut seen = std::collections::HashSet::new();
                let roles: Vec<(String, String)> = raw_roles
                    .into_iter()
                    .filter(|(id, _)| seen.insert(id.clone()))
                    .collect();
                let n = roles.len();
                let rels = prop::collection::vec((0..n, 0..n, arb_constraint_type()), 0..=4);
                (Just(name), Just(roles), rels)
            })
            .prop_map(|(name, roles, rels)| {
                let mut template = DpmlTemplate::new(name);
                for (id, role_type) in &roles {
                    template = template.with_role(DpmlRole::new(id.clone(), role_type.clone()));
                }
                for (s, t, ct) in rels {
                    template = template.with_relationship(DpmlConstraint::new(
                        roles[s].0.clone(),
                        roles[t].0.clone(),
                        ct,
                    ));
                }
                template
            })
    }

    fn to_yaml(t: &DpmlTemplate) -> String {
        let mut s = format!("name: {}\n", t.name);
        if !t.roles.is_empty() {
            s.push_str("roles:\n");
            for r in &t.roles {
                s.push_str(&format!(
                    "  - id: {}\n    type: {}\n    cardinality: \"{}\"\n",
                    r.id, r.role_type, r.cardinality
                ));
            }
        }
        if !t.relationships.is_empty() {
            s.push_str("relationships:\n");
            for rel in &t.relationships {
                s.push_str(&format!(
                    "  - source: {}\n    target: {}\n    type: {}\n",
                    rel.source, rel.target, rel.constraint_type
                ));
            }
        }
        s
    }

    fn to_toml(t: &DpmlTemplate) -> String {
        let mut s = format!("name = \"{}\"\n", t.name);
        for r in &t.roles {
            s.push_str(&format!(
                "\n[[roles]]\nid = \"{}\"\ntype = \"{}\"\ncardinality = \"{}\"\n",
                r.id, r.role_type, r.cardinality
            ));
        }
        for rel in &t.relationships {
            s.push_str(&format!(
                "\n[[relationships]]\nsource = \"{}\"\ntarget = \"{}\"\ntype = \"{}\"\n",
                rel.source, rel.target, rel.constraint_type
            ));
        }
        s
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// A template that validates converts to a PatternTemplate whose node
        /// constraints match its roles and edge constraints match its
        /// relationships.
        #[test]
        fn prop_valid_to_pattern_template(t in arb_valid_dpml()) {
            prop_assert!(t.validate().is_ok(), "expected valid: {:?}", t.validate().err());
            let pattern = t.to_pattern_template();
            prop_assert!(pattern.is_ok());
            let pattern = pattern.expect("checked ok");
            prop_assert_eq!(pattern.node_constraints.len(), t.roles.len());
            prop_assert_eq!(pattern.edge_constraints.len(), t.relationships.len());
        }

        /// An empty name always fails validation.
        #[test]
        fn prop_empty_name_errs(t in arb_valid_dpml()) {
            let mut t = t;
            t.name = String::new();
            prop_assert!(matches!(t.validate(), Err(DpmlError::MissingField(_))));
        }

        /// A duplicated role id always fails validation.
        #[test]
        fn prop_duplicate_role_errs(t in arb_valid_dpml()) {
            let mut t = t;
            let dup = t.roles[0].id.clone();
            t = t.with_role(DpmlRole::new(dup, "class"));
            prop_assert!(matches!(t.validate(), Err(DpmlError::DuplicateRole(_))));
        }

        /// An empty role id always fails validation.
        #[test]
        fn prop_empty_role_id_errs(t in arb_valid_dpml()) {
            let mut t = t;
            t = t.with_role(DpmlRole::new("", "class"));
            prop_assert!(matches!(t.validate(), Err(DpmlError::InvalidRole(_))));
        }

        /// A relationship referencing a non-existent role always fails
        /// validation. `arb_name` never starts with `_`, so the bogus id cannot
        /// collide with a generated role id.
        #[test]
        fn prop_dangling_relationship_errs(t in arb_valid_dpml()) {
            let mut t = t;
            let existing = t.roles[0].id.clone();
            t = t.with_relationship(DpmlConstraint::new(existing, "__nonexistent__", "contains"));
            prop_assert!(matches!(t.validate(), Err(DpmlError::InvalidRelationship(_))));
        }

        /// YAML and TOML renderings of the same template parse back to equal
        /// `(name, roles.len(), relationships.len())`.
        #[test]
        fn prop_yaml_toml_roundtrip_equal(t in arb_valid_dpml()) {
            let yaml = to_yaml(&t);
            let toml = to_toml(&t);

            let from_yaml = DpmlTemplate::parse(&yaml);
            prop_assert!(from_yaml.is_ok(), "YAML parse failed: {:?}\n{}", from_yaml.err(), yaml);
            let from_yaml = from_yaml.expect("checked ok");

            let from_toml = DpmlTemplate::parse_toml(&toml);
            prop_assert!(from_toml.is_ok(), "TOML parse failed: {:?}\n{}", from_toml.err(), toml);
            let from_toml = from_toml.expect("checked ok");

            // Each parse matches the source template.
            prop_assert_eq!(&from_yaml.name, &t.name);
            prop_assert_eq!(from_yaml.roles.len(), t.roles.len());
            prop_assert_eq!(from_yaml.relationships.len(), t.relationships.len());

            // And the two formats agree with each other.
            prop_assert_eq!(&from_yaml.name, &from_toml.name);
            prop_assert_eq!(from_yaml.roles.len(), from_toml.roles.len());
            prop_assert_eq!(from_yaml.relationships.len(), from_toml.relationships.len());
        }
    }
}
