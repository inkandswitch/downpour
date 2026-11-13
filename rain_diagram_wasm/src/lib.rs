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

/// Change region `region`'s note (an empty note removes it), returning the
/// rewritten source.
///
/// # Errors
///
/// Throws if there's no such region or the note isn't a valid one-liner.
#[wasm_bindgen(js_name = setRegionNote)]
pub fn set_region_note(source: &str, region: &str, note: &str) -> Result<String, JsError> {
    rain_diagram_core::edit::set_region_note(source, region, note)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Add `kind` (`"delegate"`, `"invoke"`, or `"revoke"`) as statement `at`,
/// as when dragging between lifelines, returning the rewritten source. For
/// a delegation or invocation, `from`/`to` are columns and `level` a level
/// keyword; for a revocation, `from` is the revoker's column and `to` the
/// revoked certificate's event index.
///
/// # Errors
///
/// Throws for an unknown kind or level, bad indices, or a result that
/// wouldn't parse.
#[wasm_bindgen(js_name = insertStatement)]
pub fn insert_statement(
    source: &str,
    at: usize,
    kind: &str,
    from: usize,
    to: usize,
    level: &str,
) -> Result<String, JsError> {
    use rain_diagram_core::{edit::NewStatement, level::Level};
    let level = || {
        level
            .parse::<Level>()
            .map_err(|e| JsError::new(&e.to_string()))
    };
    let new = match kind {
        "delegate" => NewStatement::Delegate {
            issuer: from,
            audience: to,
            level: level()?,
        },
        "invoke" => NewStatement::Invoke {
            by: from,
            on: to,
            level: level()?,
        },
        "revoke" => NewStatement::Revoke {
            by: from,
            target: to,
        },
        other => return Err(JsError::new(&format!("unknown statement kind {other:?}"))),
    };
    rain_diagram_core::edit::insert_statement(source, at, new)
        .map(|(out, _)| out)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Remove statement `statement` (and any region it leaves empty), returning
/// the rewritten source.
///
/// # Errors
///
/// Throws if there's no such statement, a revocation still names it, or
/// the result wouldn't parse.
#[wasm_bindgen(js_name = deleteStatement)]
pub fn delete_statement(source: &str, statement: usize) -> Result<String, JsError> {
    rain_diagram_core::edit::delete_statement(source, statement)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Move the principal in column `from` to column `to` (as when dragging its
/// head), returning the source with an updated `principals:` line.
///
/// # Errors
///
/// Throws if either column doesn't exist.
#[wasm_bindgen(js_name = movePrincipal)]
pub fn move_principal(source: &str, from: usize, to: usize) -> Result<String, JsError> {
    rain_diagram_core::edit::move_principal(source, from, to)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Make region `region` cover statements `first..=last` (as when dragging
/// its edges), returning the rewritten source.
///
/// # Errors
///
/// Throws if there's no such region, the range is invalid, or the result
/// wouldn't parse.
#[wasm_bindgen(js_name = setRegionRows)]
pub fn set_region_rows(
    source: &str,
    region: &str,
    first: usize,
    last: usize,
) -> Result<String, JsError> {
    rain_diagram_core::edit::set_region_rows(source, region, first, last)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Place statement `statement`'s label `percent` of the way along its arrow
/// (or back to the default spot with `undefined`), returning the rewritten
/// source.
///
/// # Errors
///
/// Throws if there's no such statement or `percent` is over 100.
#[wasm_bindgen(js_name = setPosition)]
pub fn set_position(
    source: &str,
    statement: usize,
    percent: Option<u16>,
) -> Result<String, JsError> {
    rain_diagram_core::edit::set_position(source, statement, percent)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Move the `from`th statement to be the `to`th (as when dragging a row),
/// returning the rewritten source.
///
/// # Errors
///
/// Throws if `to` is above something the statement relies on or below
/// something relying on it, or if the result wouldn't parse.
#[wasm_bindgen(js_name = moveStatement)]
pub fn move_statement(source: &str, from: usize, to: usize) -> Result<String, JsError> {
    rain_diagram_core::edit::move_statement(source, from, to)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Convert `source` to the [Keyhive Lab Notes'](https://www.inkandswitch.com/keyhive/notebook/08/) rain diagram JSON.
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

    /// The rain SVG with the "now" line below the first `happened` events.
    #[must_use]
    #[wasm_bindgen(js_name = rainSvgAt)]
    pub fn rain_svg_at(&self, happened: usize) -> String {
        self.diagram.to_rain_svg_at(happened)
    }

    /// The certflow snapshot after the first `happened` events.
    #[must_use]
    #[wasm_bindgen(js_name = certflowSvgAt)]
    pub fn certflow_svg_at(&self, happened: usize) -> String {
        self.diagram.to_certflow_svg_at(happened)
    }

    /// The notation the header names: `"rain"` or `"certflow"`.
    #[must_use]
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.diagram.kind().keyword().into()
    }

    /// How many events there are.
    #[must_use]
    #[wasm_bindgen(getter)]
    pub fn events(&self) -> usize {
        self.diagram.events().len()
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

    /// The rows event `event` can be dragged to, as `[first, last]`: below
    /// everything it relies on, above everything relying on it.
    #[must_use]
    pub fn movable(&self, event: usize) -> Vec<usize> {
        self.diagram
            .event_ix(event)
            .map(|ix| {
                let range = self.diagram.movable(ix);
                Vec::from([*range.start(), *range.end()])
            })
            .unwrap_or_default()
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
