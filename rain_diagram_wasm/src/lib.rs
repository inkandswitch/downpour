//! Wasm bindings for certflow and rain diagrams.
//!
//! This crate only moves data across the JS boundary; all logic lives in
//! [`rain_diagram_core`]. It will grow SVG rendering and the queries the
//! interactive embed needs (reach, revocation fallout).

use wasm_bindgen::prelude::{JsError, wasm_bindgen};

/// Check that `source` parses and resolves.
///
/// # Errors
///
/// Throws with the problem and its `line:column` if it doesn't.
#[wasm_bindgen]
pub fn validate(source: &str) -> Result<(), JsError> {
    parse(source).map(drop)
}

/// Convert `source` to the lab notebook renderer's rain diagram JSON.
///
/// # Errors
///
/// Throws with the problem and its `line:column` if `source` doesn't parse.
#[wasm_bindgen(js_name = toSiteJson)]
pub fn to_site_json(source: &str) -> Result<String, JsError> {
    Ok(parse(source)?.to_site_json()?)
}

fn parse(source: &str) -> Result<rain_diagram_core::diagram::Diagram, JsError> {
    rain_diagram_core::parse(source).map_err(|e| {
        let (line, column) = e.span().line_col(source);
        JsError::new(&format!("{line}:{column}: {e}"))
    })
}
