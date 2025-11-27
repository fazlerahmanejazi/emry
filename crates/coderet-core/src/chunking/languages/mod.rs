use crate::models::Language;
use anyhow::Result;
use tree_sitter::Parser;

pub mod python;
pub mod rust;
pub mod go;
pub mod javascript;
pub mod typescript;
pub mod java;
pub mod cpp;
pub mod c;
pub mod csharp;
pub mod ruby;
pub mod php;

#[derive(Debug, Clone)]
pub struct ChunkQuery {
    pub pattern: String,
    pub priority: u8,
}

pub trait LanguageSupport: Send + Sync {
    fn language(&self) -> Language;
    fn get_queries(&self) -> Vec<ChunkQuery>;
    fn create_parser(&self) -> Result<Parser>;
}

pub fn get_language_support(lang: Language) -> Option<Box<dyn LanguageSupport>> {
    match lang {
        Language::Python => Some(Box::new(python::PythonSupport)),
        Language::Rust => Some(Box::new(rust::RustSupport)),
        Language::Go => Some(Box::new(go::GoSupport)),
        Language::JavaScript => Some(Box::new(javascript::JavaScriptSupport)),
        Language::TypeScript => Some(Box::new(typescript::TypeScriptSupport)),
        Language::Java => Some(Box::new(java::JavaSupport)),
        Language::Cpp => Some(Box::new(cpp::CppSupport)),
        Language::C => Some(Box::new(c::CSupport)),
        Language::CSharp => Some(Box::new(csharp::CSharpSupport)),
        Language::Ruby => Some(Box::new(ruby::RubySupport)),
        Language::Php => Some(Box::new(php::PhpSupport)),
        _ => None,
    }
}
