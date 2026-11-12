//! Text art goldens: each `golden/*.rain` must render to its `.txt`.
//!
//! `notebook-08.txt` is copied from the Keyhive lab notebook; the rest were
//! reviewed by hand when blessed. After an intended rendering change, check
//! the diffs and re-bless with `BLESS=1 cargo test --test golden`.

use std::{fs, path::Path};

#[test]
fn text_goldens() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let bless = std::env::var_os("BLESS").is_some();

    let mut cases: Vec<_> = fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rain"))
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no goldens in {}", dir.display());

    let mut failures = Vec::new();
    for case in cases {
        let source = fs::read_to_string(&case)?;
        let text = rain_diagram_core::parse(&source)
            .map_err(|e| format!("{}: {e}", case.display()))?
            .to_text();
        let expected_path = case.with_extension("txt");

        if bless {
            fs::write(&expected_path, &text)?;
            continue;
        }

        let expected = fs::read_to_string(&expected_path).unwrap_or_default();
        if text != expected {
            failures.push(format!(
                "{}\n--- rendered ---\n{text}--- expected ---\n{expected}",
                case.display()
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
