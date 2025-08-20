use std::fs;
use std::path::Path;

fn main() {
    // Check for banned macros in src/ and tests/
    let src_dir = Path::new("src");
    let tests_dir = Path::new("tests");
    
    let mut violations = Vec::new();
    
    if src_dir.exists() {
        check_directory(&src_dir, &mut violations);
    }
    
    if tests_dir.exists() {
        check_directory(&tests_dir, &mut violations);
    }
    
    if !violations.is_empty() {
        println!("cargo:warning=❌ Found banned macro usage (use tracing macros instead):");
        for (file, line_num, line) in &violations {
            println!("cargo:warning=  {}:{} - {}", file.display(), line_num, line.trim());
        }
        panic!("Found {} banned macro violations", violations.len());
    }
}

fn check_directory(dir: &Path, violations: &mut Vec<(std::path::PathBuf, usize, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        
        if path.is_dir() {
            check_directory(&path, violations);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let content = fs::read_to_string(&path).unwrap();
            
            for (line_num, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                
                // Skip comments
                if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                    continue;
                }
                
                // Check for banned macros (using concat to avoid self-detection)
                let banned = [
                    concat!("print", "ln!"),
                    concat!("eprint", "ln!"),
                    concat!("print", "!"),
                    concat!("eprint", "!"),
                    concat!("dbg", "!"),
                ];
                
                if banned.iter().any(|pattern| line.contains(pattern)) {
                    violations.push((path.clone(), line_num + 1, line.to_string()));
                }
            }
        }
    }
}