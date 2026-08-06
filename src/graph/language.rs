//! Programming language definitions.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Supported programming languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Language {
    // === Systems languages ===
    /// Rust programming language.
    Rust,
    /// C programming language.
    C,
    /// C++ programming language.
    Cpp,
    /// Go programming language.
    Go,
    /// Zig programming language.
    Zig,

    // === JVM languages ===
    /// Java programming language.
    Java,
    /// Kotlin programming language.
    Kotlin,
    /// Scala programming language.
    Scala,
    /// Groovy programming language.
    Groovy,

    // === Scripting languages ===
    /// Python programming language.
    Python,
    /// JavaScript programming language.
    JavaScript,
    /// TypeScript programming language.
    TypeScript,
    /// Ruby programming language.
    Ruby,
    /// PHP programming language.
    Php,
    /// Perl programming language.
    Perl,
    /// Lua programming language.
    Lua,

    // === Functional languages ===
    /// Haskell programming language.
    Haskell,
    /// OCaml programming language.
    OCaml,
    /// F# programming language.
    FSharp,
    /// Elixir programming language.
    Elixir,
    /// Erlang programming language.
    Erlang,
    /// Clojure programming language.
    Clojure,
    /// Common Lisp programming language.
    CommonLisp,
    /// Scheme programming language.
    Scheme,

    // === .NET languages ===
    /// C# programming language.
    CSharp,
    /// Visual Basic .NET.
    VbNet,

    // === Apple languages ===
    /// Swift programming language.
    Swift,
    /// Objective-C programming language.
    ObjectiveC,

    // === Shell languages ===
    /// Bash shell.
    Bash,
    /// PowerShell.
    PowerShell,

    // === Query languages ===
    /// SQL.
    Sql,
    /// GraphQL.
    GraphQL,

    // === Markup and config ===
    /// JSON.
    Json,
    /// YAML.
    Yaml,
    /// TOML.
    Toml,
    /// XML.
    Xml,
    /// HTML.
    Html,
    /// CSS.
    Css,
    /// Markdown.
    Markdown,

    // === F1R3FLY.io languages ===
    /// Rholang - concurrent process calculus language.
    Rholang,
    /// MeTTa - meta-language for hypergraph-based AI.
    MeTTa,

    // === Other ===
    /// Unknown or unsupported language.
    #[default]
    Unknown,
}

impl Language {
    /// Returns the language name as a string.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Go => "Go",
            Self::Zig => "Zig",
            Self::Java => "Java",
            Self::Kotlin => "Kotlin",
            Self::Scala => "Scala",
            Self::Groovy => "Groovy",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Ruby => "Ruby",
            Self::Php => "PHP",
            Self::Perl => "Perl",
            Self::Lua => "Lua",
            Self::Haskell => "Haskell",
            Self::OCaml => "OCaml",
            Self::FSharp => "F#",
            Self::Elixir => "Elixir",
            Self::Erlang => "Erlang",
            Self::Clojure => "Clojure",
            Self::CommonLisp => "Common Lisp",
            Self::Scheme => "Scheme",
            Self::CSharp => "C#",
            Self::VbNet => "VB.NET",
            Self::Swift => "Swift",
            Self::ObjectiveC => "Objective-C",
            Self::Bash => "Bash",
            Self::PowerShell => "PowerShell",
            Self::Sql => "SQL",
            Self::GraphQL => "GraphQL",
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
            Self::Xml => "XML",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Markdown => "Markdown",
            Self::Rholang => "Rholang",
            Self::MeTTa => "MeTTa",
            Self::Unknown => "Unknown",
        }
    }

    /// Returns common file extensions for this language.
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["rs"],
            Self::C => &["c", "h"],
            Self::Cpp => &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "c++", "h++"],
            Self::Go => &["go"],
            Self::Zig => &["zig"],
            Self::Java => &["java"],
            Self::Kotlin => &["kt", "kts"],
            Self::Scala => &["scala", "sc"],
            Self::Groovy => &["groovy", "gvy", "gy", "gsh"],
            Self::Python => &["py", "pyi", "pyw"],
            Self::JavaScript => &["js", "mjs", "cjs"],
            Self::TypeScript => &["ts", "mts", "cts", "tsx"],
            Self::Ruby => &["rb", "rake", "gemspec"],
            Self::Php => &["php", "phtml", "php3", "php4", "php5", "phps"],
            Self::Perl => &["pl", "pm", "pod", "t"],
            Self::Lua => &["lua"],
            Self::Haskell => &["hs", "lhs"],
            Self::OCaml => &["ml", "mli"],
            Self::FSharp => &["fs", "fsi", "fsx"],
            Self::Elixir => &["ex", "exs"],
            Self::Erlang => &["erl", "hrl"],
            Self::Clojure => &["clj", "cljs", "cljc", "edn"],
            Self::CommonLisp => &["lisp", "lsp", "cl"],
            Self::Scheme => &["scm", "ss"],
            Self::CSharp => &["cs"],
            Self::VbNet => &["vb"],
            Self::Swift => &["swift"],
            Self::ObjectiveC => &["m", "mm"],
            Self::Bash => &["sh", "bash"],
            Self::PowerShell => &["ps1", "psm1", "psd1"],
            Self::Sql => &["sql"],
            Self::GraphQL => &["graphql", "gql"],
            Self::Json => &["json"],
            Self::Yaml => &["yaml", "yml"],
            Self::Toml => &["toml"],
            Self::Xml => &["xml"],
            Self::Html => &["html", "htm"],
            Self::Css => &["css"],
            Self::Markdown => &["md", "markdown"],
            Self::Rholang => &["rho"],
            Self::MeTTa => &["metta"],
            Self::Unknown => &[],
        }
    }

    /// Detects language from file extension.
    pub fn from_extension(ext: &str) -> Self {
        let ext = ext.to_lowercase();
        let ext = ext.trim_start_matches('.');

        match ext {
            "rs" => Self::Rust,
            "c" | "h" => Self::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "c++" | "h++" => Self::Cpp,
            "go" => Self::Go,
            "zig" => Self::Zig,
            "java" => Self::Java,
            "kt" | "kts" => Self::Kotlin,
            "scala" | "sc" => Self::Scala,
            "groovy" | "gvy" | "gy" | "gsh" => Self::Groovy,
            "py" | "pyi" | "pyw" => Self::Python,
            "js" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "mts" | "cts" | "tsx" => Self::TypeScript,
            "rb" | "rake" | "gemspec" => Self::Ruby,
            "php" | "phtml" | "php3" | "php4" | "php5" | "phps" => Self::Php,
            "pl" | "pm" | "pod" | "t" => Self::Perl,
            "lua" => Self::Lua,
            "hs" | "lhs" => Self::Haskell,
            "ml" | "mli" => Self::OCaml,
            "fs" | "fsi" | "fsx" => Self::FSharp,
            "ex" | "exs" => Self::Elixir,
            "erl" | "hrl" => Self::Erlang,
            "clj" | "cljs" | "cljc" | "edn" => Self::Clojure,
            "lisp" | "lsp" | "cl" => Self::CommonLisp,
            "scm" | "ss" => Self::Scheme,
            "cs" => Self::CSharp,
            "vb" => Self::VbNet,
            "swift" => Self::Swift,
            "m" | "mm" => Self::ObjectiveC,
            "sh" | "bash" => Self::Bash,
            "ps1" | "psm1" | "psd1" => Self::PowerShell,
            "sql" => Self::Sql,
            "graphql" | "gql" => Self::GraphQL,
            "json" => Self::Json,
            "yaml" | "yml" => Self::Yaml,
            "toml" => Self::Toml,
            "xml" => Self::Xml,
            "html" | "htm" => Self::Html,
            "css" => Self::Css,
            "md" | "markdown" => Self::Markdown,
            "rho" => Self::Rholang,
            "metta" => Self::MeTTa,
            _ => Self::Unknown,
        }
    }

    /// Returns true if this is a systems programming language.
    pub fn is_systems(&self) -> bool {
        matches!(
            self,
            Self::Rust | Self::C | Self::Cpp | Self::Go | Self::Zig
        )
    }

    /// Returns true if this is a JVM language.
    pub fn is_jvm(&self) -> bool {
        matches!(
            self,
            Self::Java | Self::Kotlin | Self::Scala | Self::Groovy | Self::Clojure
        )
    }

    /// Returns true if this is a scripting language.
    pub fn is_scripting(&self) -> bool {
        matches!(
            self,
            Self::Python
                | Self::JavaScript
                | Self::TypeScript
                | Self::Ruby
                | Self::Php
                | Self::Perl
                | Self::Lua
        )
    }

    /// Returns true if this is a functional programming language.
    pub fn is_functional(&self) -> bool {
        matches!(
            self,
            Self::Haskell
                | Self::OCaml
                | Self::FSharp
                | Self::Elixir
                | Self::Erlang
                | Self::Clojure
                | Self::CommonLisp
                | Self::Scheme
        )
    }

    /// Returns true if this is a markup or config language.
    pub fn is_markup(&self) -> bool {
        matches!(
            self,
            Self::Json
                | Self::Yaml
                | Self::Toml
                | Self::Xml
                | Self::Html
                | Self::Css
                | Self::Markdown
        )
    }

    /// Returns true if this is a F1R3FLY.io language.
    pub fn is_f1r3fly(&self) -> bool {
        matches!(self, Self::Rholang | Self::MeTTa)
    }

    /// Returns the paradigm(s) this language primarily supports.
    pub fn paradigms(&self) -> &'static [Paradigm] {
        match self {
            Self::Rust => &[
                Paradigm::Imperative,
                Paradigm::Functional,
                Paradigm::Concurrent,
            ],
            Self::C => &[Paradigm::Imperative, Paradigm::Procedural],
            Self::Cpp => &[
                Paradigm::Imperative,
                Paradigm::ObjectOriented,
                Paradigm::Functional,
            ],
            Self::Go => &[Paradigm::Imperative, Paradigm::Concurrent],
            Self::Java => &[Paradigm::ObjectOriented, Paradigm::Imperative],
            Self::Python => &[
                Paradigm::ObjectOriented,
                Paradigm::Imperative,
                Paradigm::Functional,
            ],
            Self::JavaScript | Self::TypeScript => &[
                Paradigm::ObjectOriented,
                Paradigm::Functional,
                Paradigm::Reactive,
            ],
            Self::Haskell => &[Paradigm::Functional],
            Self::Elixir | Self::Erlang => &[Paradigm::Functional, Paradigm::Concurrent],
            Self::Rholang => &[Paradigm::Concurrent, Paradigm::ProcessCalculus],
            Self::MeTTa => &[Paradigm::Logic, Paradigm::Functional],
            _ => &[],
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Programming paradigms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Paradigm {
    /// Imperative programming.
    Imperative,
    /// Procedural programming.
    Procedural,
    /// Object-oriented programming.
    ObjectOriented,
    /// Functional programming.
    Functional,
    /// Logic programming.
    Logic,
    /// Concurrent/parallel programming.
    Concurrent,
    /// Reactive programming.
    Reactive,
    /// Process calculus (pi-calculus, rho-calculus).
    ProcessCalculus,
    /// Declarative programming.
    Declarative,
    /// Event-driven programming.
    EventDriven,
}

impl Paradigm {
    /// Returns the paradigm name as a string.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Imperative => "Imperative",
            Self::Procedural => "Procedural",
            Self::ObjectOriented => "Object-Oriented",
            Self::Functional => "Functional",
            Self::Logic => "Logic",
            Self::Concurrent => "Concurrent",
            Self::Reactive => "Reactive",
            Self::ProcessCalculus => "Process Calculus",
            Self::Declarative => "Declarative",
            Self::EventDriven => "Event-Driven",
        }
    }
}

impl std::fmt::Display for Paradigm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language variant that declares at least one file extension
    /// (i.e. everything except `Unknown`).
    const WITH_EXT: &[Language] = &[
        Language::Rust,
        Language::C,
        Language::Cpp,
        Language::Go,
        Language::Zig,
        Language::Java,
        Language::Kotlin,
        Language::Scala,
        Language::Groovy,
        Language::Python,
        Language::JavaScript,
        Language::TypeScript,
        Language::Ruby,
        Language::Php,
        Language::Perl,
        Language::Lua,
        Language::Haskell,
        Language::OCaml,
        Language::FSharp,
        Language::Elixir,
        Language::Erlang,
        Language::Clojure,
        Language::CommonLisp,
        Language::Scheme,
        Language::CSharp,
        Language::VbNet,
        Language::Swift,
        Language::ObjectiveC,
        Language::Bash,
        Language::PowerShell,
        Language::Sql,
        Language::GraphQL,
        Language::Json,
        Language::Yaml,
        Language::Toml,
        Language::Xml,
        Language::Html,
        Language::Css,
        Language::Markdown,
        Language::Rholang,
        Language::MeTTa,
    ];

    #[test]
    fn names_are_non_empty() {
        for l in WITH_EXT {
            assert!(!l.name().is_empty(), "{l:?} has an empty name");
        }
        assert_eq!(Language::Rust.name(), "Rust");
        assert_eq!(Language::Cpp.name(), "C++");
        assert_eq!(Language::CSharp.name(), "C#");
        assert_eq!(Language::Unknown.name(), "Unknown");
    }

    #[test]
    fn extension_round_trip() {
        for &l in WITH_EXT {
            let exts = l.extensions();
            assert!(!exts.is_empty(), "{l:?} declares no extensions");
            for &ext in exts {
                assert_eq!(Language::from_extension(ext), l, "ext {ext:?}");
                // A leading dot and upper-casing are both normalized away.
                assert_eq!(Language::from_extension(&format!(".{ext}")), l);
                assert_eq!(Language::from_extension(&ext.to_uppercase()), l);
            }
        }
        assert_eq!(Language::Unknown.extensions().len(), 0);
        assert_eq!(Language::from_extension("nonsense_xyz"), Language::Unknown);
    }

    #[test]
    fn category_predicates() {
        for l in [
            Language::Rust,
            Language::C,
            Language::Cpp,
            Language::Go,
            Language::Zig,
        ] {
            assert!(l.is_systems(), "{l:?} should be a systems language");
        }
        for l in [
            Language::Java,
            Language::Kotlin,
            Language::Scala,
            Language::Groovy,
            Language::Clojure,
        ] {
            assert!(l.is_jvm(), "{l:?} should be a JVM language");
        }
        for l in [
            Language::Python,
            Language::JavaScript,
            Language::TypeScript,
            Language::Ruby,
            Language::Php,
            Language::Perl,
            Language::Lua,
        ] {
            assert!(l.is_scripting(), "{l:?} should be a scripting language");
        }
        for l in [
            Language::Haskell,
            Language::OCaml,
            Language::FSharp,
            Language::Elixir,
            Language::Erlang,
            Language::Clojure,
            Language::CommonLisp,
            Language::Scheme,
        ] {
            assert!(l.is_functional(), "{l:?} should be a functional language");
        }
        for l in [
            Language::Json,
            Language::Yaml,
            Language::Toml,
            Language::Xml,
            Language::Html,
            Language::Css,
            Language::Markdown,
        ] {
            assert!(l.is_markup(), "{l:?} should be a markup language");
        }
        for l in [Language::Rholang, Language::MeTTa] {
            assert!(l.is_f1r3fly(), "{l:?} should be an F1R3FLY.io language");
        }

        // Representative negatives.
        assert!(!Language::Python.is_systems());
        assert!(!Language::Rust.is_jvm());
        assert!(!Language::Rust.is_scripting());
        assert!(!Language::Rust.is_functional());
        assert!(!Language::Rust.is_markup());
        assert!(!Language::Rust.is_f1r3fly());
    }

    #[test]
    fn paradigms_representative() {
        assert!(Language::Rust.paradigms().contains(&Paradigm::Concurrent));
        assert!(Language::Rust.paradigms().contains(&Paradigm::Functional));
        assert!(Language::Rholang
            .paradigms()
            .contains(&Paradigm::ProcessCalculus));
        assert!(Language::MeTTa.paradigms().contains(&Paradigm::Logic));
        assert_eq!(Language::Haskell.paradigms().len(), 1);
        assert!(Language::Haskell
            .paradigms()
            .contains(&Paradigm::Functional));
        // Languages that fall through to the `_ => &[]` arm have no paradigms.
        assert!(Language::Unknown.paradigms().is_empty());
        assert!(Language::Zig.paradigms().is_empty());
    }

    #[test]
    fn display_matches_name() {
        assert_eq!(format!("{}", Language::Rust), "Rust");
        assert_eq!(format!("{}", Language::Cpp), "C++");
        for l in WITH_EXT {
            assert_eq!(format!("{l}"), l.name());
        }
        assert_eq!(format!("{}", Paradigm::ProcessCalculus), "Process Calculus");
        assert_eq!(format!("{}", Paradigm::ObjectOriented), "Object-Oriented");
    }

    #[test]
    fn default_is_unknown() {
        assert_eq!(Language::default(), Language::Unknown);
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;

    #[test]
    fn serde_language_round_trip() {
        for l in [
            Language::Rust,
            Language::Cpp,
            Language::Rholang,
            Language::MeTTa,
            Language::Python,
            Language::Unknown,
        ] {
            let s = serde_json::to_string(&l).expect("serialize Language");
            let back: Language = serde_json::from_str(&s).expect("deserialize Language");
            assert_eq!(l, back);
        }
    }
}
