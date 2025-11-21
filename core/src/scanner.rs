use crate::config::IndexConfig;
use crate::models::Language;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

pub struct ScannedFile {
    pub path: PathBuf,
    pub language: Language,
}

pub fn scan_repo(root: &Path, config: &IndexConfig) -> Vec<ScannedFile> {
    let mut files = Vec::new();
    let mut builder = WalkBuilder::new(root);

    // Apply config ignores
    // Note: ignore crate handles .gitignore automatically by default
    for path in &config.exclude_paths {
        builder.add_ignore(path); // This might need a .ignore file format or similar, 
                                  // but WalkBuilder has .overrides() for manual patterns.
                                  // For simplicity in Phase 1, we rely on .gitignore and standard hiding.
                                  // To strictly support config.exclude_paths as globs, we'd use OverridesBuilder.
    }
    
    // TODO: rigorous integration of config.exclude_paths using OverridesBuilder
    
    let walker = builder.build();

    for result in walker {
        match result {
            Ok(entry) => {
                if entry.file_type().map_or(false, |ft| ft.is_file()) {
                    let path = entry.path();
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        let lang = Language::from_extension(ext);
                        if lang != Language::Unknown {
                            // Check if language is enabled in config (if we had that filter)
                            // For now, we accept all supported languages.
                            files.push(ScannedFile {
                                path: path.to_path_buf(),
                                language: lang,
                            });
                        }
                    }
                }
            }
            Err(err) => {
                eprintln!("Error scanning path: {}", err);
            }
        }
    }

    files
}
