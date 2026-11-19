//! Service registry and `SOCKET_SLOTS` invariants (F4 Task 1).
//!
//! Ensures every `.rs` service module placed in `src/services/` (other than
//! `mod.rs`) is wired inside `fn register_all` in `src/services/mod.rs`, and
//! that `main.rs` allocates 16 smoltcp socket slots.

use std::fs;
use std::path::PathBuf;

fn extract_fn_body<'a>(src: &'a str, signature_prefix: &str) -> &'a str {
    let start = src
        .find(signature_prefix)
        .expect("fn register_all must exist in src/services/mod.rs");
    let rest = &src[start..];
    let open_brace = rest
        .find('{')
        .expect("fn register_all must have an opening brace");
    let mut depth = 0usize;
    for (idx, ch) in rest[open_brace..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[open_brace..open_brace + idx + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed brace in fn register_all");
}

#[test]
fn every_service_file_is_referenced_in_register_all() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let services_dir = manifest_dir.join("src").join("services");
    let mod_path = services_dir.join("mod.rs");
    let mod_src = fs::read_to_string(&mod_path).expect("src/services/mod.rs must be readable");
    let register_body = extract_fn_body(&mod_src, "pub fn register_all");

    let entries = fs::read_dir(&services_dir).expect("src/services directory must exist");
    for entry in entries {
        let entry = entry.expect("dir entry must be readable");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name == "mod.rs" {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("service file must have UTF-8 stem");
        assert!(
            register_body.contains(stem),
            "service file `src/services/{file_name}` is not referenced inside `register_all`"
        );
    }
}

#[test]
fn main_socket_slots_is_sixteen() {
    let main_src = include_str!("../src/main.rs");
    let line = main_src
        .lines()
        .find(|l| l.contains("const SOCKET_SLOTS: usize ="))
        .expect("main.rs must define `const SOCKET_SLOTS: usize =`");
    assert!(
        line.contains("const SOCKET_SLOTS: usize = 16;"),
        "expected SOCKET_SLOTS == 16, found: `{}`",
        line.trim()
    );
}
