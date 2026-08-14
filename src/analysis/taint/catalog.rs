//! Data-driven, language-agnostic taint classification.
//!
//! Rules inspect only callee paths reconstructed from typed CPG nodes. They do
//! not inspect retained source text and deliberately avoid regular expressions,
//! keeping the security analysis available with the feature-free core.

use std::collections::BTreeMap;
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::{SinkClass, SourceClass};

/// A bounded string-matching strategy for a reconstructed callee path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Pattern {
    /// The complete callee path must equal the pattern.
    Exact(Arc<str>),
    /// The complete callee path must end with the pattern.
    Suffix(Arc<str>),
    /// The pattern may occur anywhere in the callee path.
    Substring(Arc<str>),
    /// One identifier segment must equal the pattern.
    Segment(Arc<str>),
}

impl Pattern {
    /// Returns whether this pattern recognizes `callee_path`.
    pub fn matches(&self, callee_path: &str) -> bool {
        match self {
            Self::Exact(pattern) => callee_path == pattern.as_ref(),
            Self::Suffix(pattern) => callee_path.ends_with(pattern.as_ref()),
            Self::Substring(pattern) => callee_path.contains(pattern.as_ref()),
            Self::Segment(pattern) => callee_path
                .split(|character: char| !character.is_alphanumeric() && character != '_')
                .any(|segment| segment == pattern.as_ref()),
        }
    }

    /// Stable evidence score assigned to a successful match.
    pub const fn confidence(&self) -> f32 {
        match self {
            Self::Exact(_) => 1.0,
            Self::Segment(_) => 0.95,
            Self::Suffix(_) => 0.9,
            Self::Substring(_) => 0.8,
        }
    }

    pub(crate) const fn specificity(&self) -> u8 {
        match self {
            Self::Exact(_) => 4,
            Self::Segment(_) => 3,
            Self::Suffix(_) => 2,
            Self::Substring(_) => 1,
        }
    }
}

/// One source, sink, or sanitizer classification rule.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CatalogRule {
    /// Callee-path matcher.
    pub pattern: Pattern,
    /// Open, consumer-defined classification name.
    pub class: Arc<str>,
    /// Optional Common Weakness Enumeration identifier.
    pub cwe: Option<u32>,
}

/// Evidence returned for one matching catalog class.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CatalogMatch {
    /// Open source, sink, or sanitizer class.
    pub class: Arc<str>,
    /// Optional Common Weakness Enumeration identifier.
    pub cwe: Option<u32>,
    /// Match-specificity score in the inclusive interval `[0, 1]`.
    pub confidence: f32,
}

/// All catalog roles recognized for one callee path.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CatalogClassification {
    /// Matching taint-source classes.
    pub sources: Vec<CatalogMatch>,
    /// Matching taint-sink classes.
    pub sinks: Vec<CatalogMatch>,
    /// Matching sanitizer classes.
    pub sanitizers: Vec<CatalogMatch>,
}

/// How taint crosses an opaque external or unresolved call boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExternalModel {
    /// Every tainted argument may taint the result.
    #[default]
    Passthrough,
    /// The result is modeled as a fresh untainted value.
    Havoc,
    /// The result is a fresh value in the named source class.
    Source {
        /// Open taint-source class.
        class: SourceClass,
    },
    /// Every tainted argument reaches the named sink class.
    Sink {
        /// Open taint-sink class.
        class: SinkClass,
        /// Optional Common Weakness Enumeration identifier.
        cwe: Option<u32>,
    },
    /// The result carries no taint from its arguments.
    Sanitizer,
}

/// Declarative source, sink, sanitizer, and external-call models.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TaintCatalog {
    /// Rules classifying calls that introduce attacker-controlled values.
    pub sources: Vec<CatalogRule>,
    /// Rules classifying calls that consume security-sensitive values.
    pub sinks: Vec<CatalogRule>,
    /// Rules classifying calls that remove taint.
    pub sanitizers: Vec<CatalogRule>,
    /// Ordered path patterns and their opaque-call models.
    pub externals: Vec<(Pattern, ExternalModel)>,
}

impl TaintCatalog {
    /// Creates a catalog with no classifiers or external overrides.
    pub const fn empty() -> Self {
        Self {
            sources: Vec::new(),
            sinks: Vec::new(),
            sanitizers: Vec::new(),
            externals: Vec::new(),
        }
    }

    /// Creates the cross-language generic catalog shipped with libcpg.
    ///
    /// Its open classes preserve pgmcp's source families (`env`, `argv`,
    /// `stdin`, and `request`) and injection sinks (`command`, `sql`, `eval`,
    /// `deserialize`, and `path`) without tying the core to a language grammar.
    pub fn builtin_generic() -> Self {
        let mut catalog = Self::empty();

        for value in ["env::var", "getenv", "os.environ", "process.env"] {
            catalog
                .sources
                .push(rule(Pattern::Substring(value.into()), "env", None));
        }
        for value in ["env::args", "sys.argv", "process.argv", "os.Args"] {
            catalog
                .sources
                .push(rule(Pattern::Substring(value.into()), "argv", None));
        }
        for value in ["io::stdin", "sys.stdin", "read_line", "readLine"] {
            catalog
                .sources
                .push(rule(Pattern::Substring(value.into()), "stdin", None));
        }
        for value in [
            "req.body",
            "req.query",
            "req.params",
            "request.body",
            "request.form",
            "request.args",
            "query_string",
            "form_data",
            "FormData",
        ] {
            catalog
                .sources
                .push(rule(Pattern::Substring(value.into()), "request", None));
        }

        for value in [
            "Command::new",
            "process::Command",
            "os.system",
            "subprocess.",
            "Popen",
            "Runtime.exec",
            "child_process",
        ] {
            catalog
                .sinks
                .push(rule(Pattern::Substring(value.into()), "command", Some(78)));
        }
        for value in ["system", "execvp", "execve"] {
            catalog
                .sinks
                .push(rule(Pattern::Exact(value.into()), "command", Some(78)));
        }
        catalog
            .sinks
            .push(rule(Pattern::Suffix("::exec".into()), "command", Some(78)));

        for value in [
            "sqlx::query",
            "cursor.execute",
            "executeQuery",
            "rawQuery",
            "db.query",
        ] {
            catalog
                .sinks
                .push(rule(Pattern::Substring(value.into()), "sql", Some(89)));
        }
        for value in [".execute", ".query"] {
            catalog
                .sinks
                .push(rule(Pattern::Suffix(value.into()), "sql", Some(89)));
        }

        catalog
            .sinks
            .push(rule(Pattern::Segment("eval".into()), "eval", Some(95)));
        for value in ["vm.run", "Function"] {
            catalog
                .sinks
                .push(rule(Pattern::Substring(value.into()), "eval", Some(95)));
        }

        for value in [
            "pickle.load",
            "yaml.load",
            "ObjectInputStream",
            "Marshal.load",
            "unserialize",
        ] {
            catalog.sinks.push(rule(
                Pattern::Substring(value.into()),
                "deserialize",
                Some(502),
            ));
        }

        for value in [
            "File::open",
            "fs::read",
            "fs::write",
            "fopen",
            "readFile",
            "sendfile",
        ] {
            catalog
                .sinks
                .push(rule(Pattern::Substring(value.into()), "path", Some(22)));
        }

        for value in [
            "escape",
            "sanitize",
            "quote",
            "htmlspecialchars",
            "parameterize",
            "canonicalize",
            "validate",
            "parseInt",
            "parseFloat",
            "to_int",
            ".bind",
        ] {
            catalog
                .sanitizers
                .push(rule(Pattern::Substring(value.into()), "sanitizer", None));
        }

        catalog.externals.extend([
            (
                Pattern::Substring("tokio::process::Command".into()),
                ExternalModel::Sink {
                    class: "command".into(),
                    cwe: Some(78),
                },
            ),
            (
                Pattern::Substring("duct::cmd".into()),
                ExternalModel::Sink {
                    class: "command".into(),
                    cwe: Some(78),
                },
            ),
            (
                Pattern::Substring("diesel::sql_query".into()),
                ExternalModel::Sink {
                    class: "sql".into(),
                    cwe: Some(89),
                },
            ),
            (
                Pattern::Substring("std::fs::".into()),
                ExternalModel::Sink {
                    class: "path".into(),
                    cwe: Some(22),
                },
            ),
            (
                Pattern::Substring("tokio::fs::".into()),
                ExternalModel::Sink {
                    class: "path".into(),
                    cwe: Some(22),
                },
            ),
        ]);

        catalog
    }

    /// Classifies a reconstructed callee path under every catalog role.
    pub fn classify(&self, callee_path: &str) -> CatalogClassification {
        CatalogClassification {
            sources: matching_rules(&self.sources, callee_path),
            sinks: matching_rules(&self.sinks, callee_path),
            sanitizers: matching_rules(&self.sanitizers, callee_path),
        }
    }

    /// Returns the most-specific opaque-call model, defaulting to passthrough.
    pub fn external_model(&self, callee_path: &str) -> ExternalModel {
        self.external_match(callee_path)
            .map_or_else(ExternalModel::default, |(model, _)| model)
    }

    pub(crate) fn external_match(&self, callee_path: &str) -> Option<(ExternalModel, f32)> {
        self.externals
            .iter()
            .enumerate()
            .filter(|(_, (pattern, _))| pattern.matches(callee_path))
            .max_by_key(|(index, (pattern, _))| (pattern.specificity(), usize::MAX - index))
            .map(|(_, (pattern, model))| (model.clone(), pattern.confidence()))
    }
}

fn rule(pattern: Pattern, class: &str, cwe: Option<u32>) -> CatalogRule {
    CatalogRule {
        pattern,
        class: class.into(),
        cwe,
    }
}

fn matching_rules(rules: &[CatalogRule], callee_path: &str) -> Vec<CatalogMatch> {
    let mut best = BTreeMap::<(Arc<str>, Option<u32>), f32>::new();
    for rule in rules
        .iter()
        .filter(|rule| rule.pattern.matches(callee_path))
    {
        let confidence = rule.pattern.confidence();
        best.entry((rule.class.clone(), rule.cwe))
            .and_modify(|current| *current = current.max(confidence))
            .or_insert(confidence);
    }
    best.into_iter()
        .map(|((class, cwe), confidence)| CatalogMatch {
            class,
            cwe,
            confidence,
        })
        .collect()
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
