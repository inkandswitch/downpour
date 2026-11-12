//! Render a diagram from stdin, with any diagnostics on stderr.
//!
//! ```console
//! $ cargo run --example render < diagram.rain          # Unicode text art
//! $ cargo run --example render -- --svg < diagram.rain # SVG
//! $ cargo run --example render -- --json < diagram.rain # notebook JSON
//! ```

use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = String::new();
    std::io::stdin().read_to_string(&mut source)?;

    let diagram = rain_diagram_core::parse(&source).map_err(|e| {
        let (line, column) = e.span().line_col(&source);
        format!("{line}:{column}: {e}")
    })?;

    match std::env::args().nth(1).as_deref() {
        Some("--svg") => println!("{}", diagram.to_svg()),
        Some("--json") => println!("{}", diagram.to_site_json()?),
        _ => print!("{}", diagram.to_text()),
    }
    for diagnostic in diagram.diagnostics() {
        let (line, column) = diagnostic.span().line_col(&source);
        eprintln!("warning: {line}:{column}: {diagnostic}");
    }
    Ok(())
}
