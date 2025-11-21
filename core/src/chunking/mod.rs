use crate::models::Chunk;
use anyhow::Result;

pub trait Chunker {
    fn chunk(&self, content: &str, file_path: &std::path::Path) -> Result<Vec<Chunk>>;
}

pub mod python;
pub mod typescript;
pub mod java;
pub mod cpp;

pub use python::PythonChunker;
pub use typescript::TypeScriptChunker;
pub use java::JavaChunker;
pub use cpp::CppChunker;
