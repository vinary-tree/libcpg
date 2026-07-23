//! Tree-sitter parser registry.
//!
//! Maps CPG languages to tree-sitter parser Language objects.

use std::sync::OnceLock;
use rustc_hash::FxHashMap;
use tree_sitter::Language as TsLanguage;

use crate::Language;

/// Registry mapping CPG languages to tree-sitter parsers.
///
/// This registry provides lazy initialization of tree-sitter language
/// grammars based on enabled Cargo features.
pub struct ParserRegistry {
    parsers: FxHashMap<Language, TsLanguage>,
}

impl ParserRegistry {
    /// Returns the global parser registry instance.
    ///
    /// The registry is initialized lazily on first access.
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<ParserRegistry> = OnceLock::new();
        INSTANCE.get_or_init(Self::new)
    }

    /// Creates a new parser registry with all available languages.
    fn new() -> Self {
        #[allow(unused_mut)]
        let mut parsers = FxHashMap::default();

        // Rust
        #[cfg(feature = "lang-rust")]
        {
            parsers.insert(Language::Rust, tree_sitter_rust::LANGUAGE.into());
        }

        // Python
        #[cfg(feature = "lang-python")]
        {
            parsers.insert(Language::Python, tree_sitter_python::LANGUAGE.into());
        }

        // JavaScript
        #[cfg(feature = "lang-javascript")]
        {
            parsers.insert(Language::JavaScript, tree_sitter_javascript::LANGUAGE.into());
        }

        // TypeScript
        #[cfg(feature = "lang-typescript")]
        {
            parsers.insert(
                Language::TypeScript,
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            );
        }

        // Go
        #[cfg(feature = "lang-go")]
        {
            parsers.insert(Language::Go, tree_sitter_go::LANGUAGE.into());
        }

        // Java
        #[cfg(feature = "lang-java")]
        {
            parsers.insert(Language::Java, tree_sitter_java::LANGUAGE.into());
        }

        // C
        #[cfg(feature = "lang-c")]
        {
            parsers.insert(Language::C, tree_sitter_c::LANGUAGE.into());
        }

        // C++
        #[cfg(feature = "lang-cpp")]
        {
            parsers.insert(Language::Cpp, tree_sitter_cpp::LANGUAGE.into());
        }

        // JSON
        #[cfg(feature = "lang-json")]
        {
            parsers.insert(Language::Json, tree_sitter_json::LANGUAGE.into());
        }

        // HTML
        #[cfg(feature = "lang-html")]
        {
            parsers.insert(Language::Html, tree_sitter_html::LANGUAGE.into());
        }

        // CSS
        #[cfg(feature = "lang-css")]
        {
            parsers.insert(Language::Css, tree_sitter_css::LANGUAGE.into());
        }

        // Bash
        #[cfg(feature = "lang-bash")]
        {
            parsers.insert(Language::Bash, tree_sitter_bash::LANGUAGE.into());
        }

        // TOML
        #[cfg(feature = "lang-toml")]
        {
            parsers.insert(Language::Toml, tree_sitter_toml_ng::LANGUAGE.into());
        }

        // YAML
        #[cfg(feature = "lang-yaml")]
        {
            parsers.insert(Language::Yaml, tree_sitter_yaml::LANGUAGE.into());
        }

        // Markdown
        #[cfg(feature = "lang-markdown")]
        {
            parsers.insert(Language::Markdown, tree_sitter_md::LANGUAGE.into());
        }

        // Ruby
        #[cfg(feature = "lang-ruby")]
        {
            parsers.insert(Language::Ruby, tree_sitter_ruby::LANGUAGE.into());
        }

        Self { parsers }
    }

    /// Returns the tree-sitter language for a CPG language.
    ///
    /// Returns `None` if the language grammar is not available
    /// (feature not enabled or unsupported).
    pub fn get(&self, lang: Language) -> Option<TsLanguage> {
        self.parsers.get(&lang).cloned()
    }

    /// Returns true if the given language is supported.
    pub fn supports(&self, lang: Language) -> bool {
        self.parsers.contains_key(&lang)
    }

    /// Returns an iterator over all supported languages.
    pub fn supported_languages(&self) -> impl Iterator<Item = Language> + '_ {
        self.parsers.keys().copied()
    }

    /// Returns the number of supported languages.
    pub fn language_count(&self) -> usize {
        self.parsers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = ParserRegistry::global();
        // At minimum, we should have some languages registered if features are enabled
        // This test just verifies the registry can be created
        let _ = registry.language_count();
    }

    #[test]
    #[cfg(feature = "lang-rust")]
    fn test_rust_parser_available() {
        let registry = ParserRegistry::global();
        assert!(registry.supports(Language::Rust));
        assert!(registry.get(Language::Rust).is_some());
    }

    #[test]
    #[cfg(feature = "lang-python")]
    fn test_python_parser_available() {
        let registry = ParserRegistry::global();
        assert!(registry.supports(Language::Python));
        assert!(registry.get(Language::Python).is_some());
    }

    #[test]
    fn test_unknown_language_not_supported() {
        let registry = ParserRegistry::global();
        assert!(!registry.supports(Language::Unknown));
        assert!(registry.get(Language::Unknown).is_none());
    }
}
