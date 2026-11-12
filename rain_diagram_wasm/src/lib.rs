//! Wasm bindings for certflow and rain diagrams.
//!
//! This crate only moves data across the JS boundary; all logic lives in
//! [`rain_diagram_core`]. It will grow the queries the interactive embed
//! needs (reach, revocation fallout).

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

/// Warnings for things that resolve but don't do what they look like they
/// do, such as delegating without authority, each as `line:column: message`.
///
/// # Errors
///
/// Throws with the problem and its `line:column` if `source` doesn't parse.
#[wasm_bindgen]
pub fn diagnostics(source: &str) -> Result<Vec<String>, JsError> {
    Ok(parse(source)?
        .diagnostics()
        .iter()
        .map(|d| {
            let (line, column) = d.span().line_col(source);
            format!("{line}:{column}: {d}")
        })
        .collect())
}

/// Draw `source` as a standalone SVG document.
///
/// # Errors
///
/// Throws with the problem and its `line:column` if `source` doesn't parse.
#[wasm_bindgen(js_name = renderSvg)]
pub fn render_svg(source: &str) -> Result<String, JsError> {
    Ok(parse(source)?.to_svg())
}

/// Draw `source` as Unicode text art.
///
/// # Errors
///
/// Throws with the problem and its `line:column` if `source` doesn't parse.
#[wasm_bindgen(js_name = renderText)]
pub fn render_text(source: &str) -> Result<String, JsError> {
    Ok(parse(source)?.to_text())
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

/// A parsed diagram, kept for interactive queries without re-parsing.
///
/// ```js
/// const diagram = new RainDiagram(source);
/// host.innerHTML = diagram.svg();
/// diagram.reach("bob");      // what Bob can act on, and through which certificates
/// diagram.dependents(1);     // what revoking event 1 would void
/// ```
#[derive(Debug)]
#[wasm_bindgen]
pub struct RainDiagram {
    diagram: rain_diagram_core::diagram::Diagram,
}

#[wasm_bindgen]
impl RainDiagram {
    /// Parse and resolve `source`.
    ///
    /// # Errors
    ///
    /// Throws with the problem and its `line:column` if it doesn't parse.
    #[wasm_bindgen(constructor)]
    pub fn new(source: &str) -> Result<Self, JsError> {
        Ok(Self {
            diagram: parse(source)?,
        })
    }

    /// The SVG document, in the notation the header names.
    #[must_use]
    pub fn svg(&self) -> String {
        self.diagram.to_svg()
    }

    /// The SVG as a rain diagram, whatever the header.
    #[must_use]
    #[wasm_bindgen(js_name = rainSvg)]
    pub fn rain_svg(&self) -> String {
        self.diagram.to_rain_svg()
    }

    /// The SVG as a certflow snapshot of the last moment, whatever the
    /// header.
    #[must_use]
    #[wasm_bindgen(js_name = certflowSvg)]
    pub fn certflow_svg(&self) -> String {
        self.diagram.to_certflow_svg()
    }

    /// Unicode text art.
    #[must_use]
    pub fn text(&self) -> String {
        self.diagram.to_text()
    }

    /// Everything a principal can act on as of the last event, as JSON:
    /// `{ "subjects": [{ "key", "level" }], "events": [index, …] }`, where
    /// `events` are the certificates on the routes. Unknown keys reach
    /// nothing.
    #[must_use]
    pub fn reach(&self, principal: &str) -> String {
        let d = &self.diagram;
        let (Some(holder), Some(now)) = (d.principal_by_key(principal), d.now()) else {
            return r#"{"subjects":[],"events":[]}"#.into();
        };

        let reach = d.reach(holder, now);
        let mut events: Vec<usize> = reach
            .iter()
            .flat_map(|r| {
                r.route
                    .certs()
                    .map(rain_diagram_core::diagram::EventIx::index)
            })
            .collect();
        events.sort_unstable();
        events.dedup();

        // Hand-written: `serde_json::json!` costs ~90 KB of Wasm
        let subjects: Vec<String> = reach
            .iter()
            .filter_map(|r| {
                let key = serde_json::to_string(d.principal(r.subject)?.key()).ok()?;
                Some(format!(
                    r#"{{"key":{key},"level":"{}"}}"#,
                    r.level.keyword()
                ))
            })
            .collect();
        let events: Vec<String> = events.iter().map(ToString::to_string).collect();
        format!(
            r#"{{"subjects":[{}],"events":[{}]}}"#,
            subjects.join(","),
            events.join(",")
        )
    }

    /// Indices of the events that revoking event `event` would void (empty
    /// for anything but a certificate).
    #[must_use]
    pub fn dependents(&self, event: usize) -> Vec<usize> {
        self.diagram
            .event_ix(event)
            .filter(|&ix| self.diagram.cert(ix).is_some())
            .map(|ix| {
                self.diagram
                    .dependents(ix)
                    .map(rain_diagram_core::diagram::EventIx::index)
                    .collect()
            })
            .unwrap_or_default()
    }
}
