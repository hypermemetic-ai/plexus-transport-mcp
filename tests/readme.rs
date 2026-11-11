//! README.md's example is what `scripts/example.sh` recorded.
//!
//! A fenced block under a comment that names a file,
//! `<!-- examples/beside.txt -->`, is that file whole. A block that differs
//! from its file fails this test.

use std::fs;
use std::path::Path;

#[test]
fn a_named_block_is_its_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join("README.md")).unwrap();
    let readme: Vec<&str> = text.lines().collect();

    let mut named = 0;
    for (index, line) in readme.iter().enumerate() {
        let Some(source) = line.strip_prefix("<!-- ").and_then(|rest| rest.strip_suffix(" -->")) else { continue };
        assert!(readme[index + 1].starts_with("```"), "README.md:{}: a block follows the comment", index + 2);
        let close = (index + 2..readme.len()).find(|&i| readme[i].starts_with("```")).expect("the block is closed");
        let file = fs::read_to_string(root.join(source)).unwrap_or_else(|e| panic!("{source}: {e}"));
        let file: Vec<&str> = file.trim_end().lines().collect();
        assert_eq!(&readme[index + 2..close], file.as_slice(), "README.md:{}: not what {source} says", index + 3);
        named += 1;
    }
    assert!(named > 0, "README.md names no file");
}
