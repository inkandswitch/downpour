//! Goldens: each `golden/*.rain` and `golden/*.certflow` must render to its
//! `.txt` (and say the notation its extension does), and the README's
//! picture must match the SVG renderer.
//!
//! `notebook-08.txt` is copied from [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/); the rest were
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
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "rain" || ext == "certflow")
        })
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no goldens in {}", dir.display());

    let mut failures = Vec::new();
    for case in cases {
        let source = fs::read_to_string(&case)?;
        let diagram =
            rain_diagram_core::parse(&source).map_err(|e| format!("{}: {e}", case.display()))?;
        let kind = match diagram.kind() {
            rain_diagram_core::ast::Kind::Rain => "rain",
            rain_diagram_core::ast::Kind::Certflow => "certflow",
        };
        assert!(
            case.extension().is_some_and(|ext| ext == kind),
            "{} is a {kind} diagram; name it .{kind}",
            case.display()
        );
        let text = diagram.to_text();
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

/// The README's picture (`docs/notebook-08.svg`) is what `to_svg` draws
/// today. Re-bless with `BLESS=1 cargo test --test golden` after an
/// intended SVG change.
#[test]
fn readme_svg_is_current() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read_to_string(root.join("tests/golden/notebook-08.rain"))?;
    let svg = rain_diagram_core::parse(&source)?.to_svg();
    let path = root.join("../../docs/notebook-08.svg");

    if std::env::var_os("BLESS").is_some() {
        fs::write(&path, format!("{svg}\n"))?;
    }
    assert!(
        fs::read_to_string(&path)?.trim_end() == svg,
        "{} is stale; re-bless with BLESS=1 cargo test --test golden",
        path.display()
    );
    Ok(())
}
