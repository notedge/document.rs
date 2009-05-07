//! Extracted from `src/seed_input.rs`.

use crawler::*;
#[test]
fn reads_seed_file_with_comments() {
    let dir = std::env::temp_dir().join(format!("crawler-seeds-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("seeds.txt");
    std::fs::write(&path, "# docs\nhttps://example.com/a\n\nhttps://example.com/b\n").expect("write");
    let seeds = resolve_crawl_seed_specs(path.to_str().expect("utf8")).expect("seeds");
    assert_eq!(seeds.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn single_url_input_is_not_treated_as_file() {
    let seeds = resolve_crawl_seed_specs("https://example.com/only").expect("seed");
    assert_eq!(seeds, vec!["https://example.com/only".to_string()]);
}
