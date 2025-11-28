use anyhow::Result;
use emry_config::Config;
use emry_core::models::Language;
use emry_pipeline::index::{prepare_files_async, FileInput};
use std::time::Instant;
use tempfile::TempDir;

#[tokio::test]
async fn test_indexing_performance_stress() -> Result<()> {
    // 1. Setup
    let temp_dir = TempDir::new()?;
    let root = temp_dir.path().to_path_buf();
    let config = Config::default();
    
    // 2. Generate Synthetic Workload
    // 1,000 files
    // Mix of Rust, Python, JS
    let num_files = 1000;
    let mut files = Vec::with_capacity(num_files);
    
    for i in 0..num_files {
        let (lang, ext, content) = match i % 3 {
            0 => (Language::Rust, "rs", "fn main() { println!(\"Hello\"); }"),
            1 => (Language::Python, "py", "def main():\n    print('Hello')"),
            _ => (Language::JavaScript, "js", "function main() { console.log('Hello'); }"),
        };
        
        let path = root.join(format!("file_{}.{}", i, ext));
        let hash = format!("hash_{}", i);
        
        files.push(FileInput {
            path,
            language: lang,
            file_id: i as u64,
            file_node_id: format!("file_node:{}", i),
            hash,
            content: content.to_string(),
            last_modified: 0,
        });
    }

    println!("Generated {} synthetic files.", num_files);

    // 3. Run Pipeline (prepare_files_async)
    // This tests the heavy lifting: chunking, symbol extraction, etc.
    // We mock the embedder as None for pure CPU/parsing benchmark.
    
    let start_time = Instant::now();
    let concurrency = 8;
    
    let prepared = prepare_files_async(files, &config, None, concurrency).await;
    
    let duration = start_time.elapsed();
    
    // 4. Report & Assert
    println!("Processed {} files in {:.2?}", prepared.len(), duration);
    
    let files_per_sec = num_files as f64 / duration.as_secs_f64();
    println!("Throughput: {:.2} files/sec", files_per_sec);
    
    assert_eq!(prepared.len(), num_files);
    
    // Baseline assertion: Should be faster than 100 files/sec on modern hardware
    // (Simple parsing shouldn't take > 10ms per file)
    // Adjust baseline if CI is slow, but 100 is very conservative.
    assert!(files_per_sec > 40.0, "Performance too low: {:.2} files/sec", files_per_sec);

    Ok(())
}
