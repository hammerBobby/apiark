//! JSON syntax-highlighted view for HTTP response bodies.
//!
//! Pretty-prints JSON (when detectable) and produces highlight ranges for keys,
//! strings, numbers, booleans and null, rendered via GPUI `StyledText`.

use crate::theme;
use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use gpui_kit::*;
use std::{ops::Range, rc::Rc};

/// Try to pretty-print a response body as JSON. Returns `None` when the body
/// isn't valid JSON so the caller can fall back to plain text.
pub fn try_format_json(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// Build syntax-highlight ranges for a (pretty-printed) JSON string.
pub(crate) fn highlight_ranges(text: &str) -> Vec<(Range<usize>, Hsla)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                // A string is a key if the next non-whitespace char is ':'.
                let after = text[i..].trim_start().chars().next();
                let color = if after == Some(':') {
                    hsla(crate::theme::hue(217.0), 0.83, 0.45, 1.0) // keys: blue
                } else {
                    hsla(crate::theme::hue(142.0), 0.71, 0.36, 1.0) // string values: green
                };
                ranges.push((start..i, color));
            }
            b'0'..=b'9' | b'-' => {
                let start = i;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    i += 1;
                }
                ranges.push((start..i, hsla(crate::theme::hue(32.0), 0.85, 0.40, 1.0))); // numbers: amber
            }
            b't' | b'f' if text[i..].starts_with("true") || text[i..].starts_with("false") => {
                let len = if text[i..].starts_with("true") { 4 } else { 5 };
                ranges.push((i..i + len, hsla(crate::theme::hue(263.0), 0.70, 0.52, 1.0))); // bool: purple
                i += len;
            }
            b'n' if text[i..].starts_with("null") => {
                ranges.push((i..i + 4, theme::text_muted()));
                i += 4;
            }
            _ => i += 1,
        }
    }
    ranges
}

/// Lightweight JSON highlighter for GPUI editors. Avoids tree-sitter parsing
/// on every keystroke, which was noticeably slow for request/response bodies.
pub struct JsonHighlighter {
    ranges: Vec<(Range<usize>, Hsla)>,
}

impl JsonHighlighter {
    pub fn factory() -> InputHighlighterFactory {
        Rc::new(|language| {
            if language == "json" {
                Some(Box::new(Self { ranges: Vec::new() }))
            } else {
                None
            }
        })
    }
}

impl InputHighlighter for JsonHighlighter {
    fn language(&self) -> SharedString {
        "json".into()
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.ranges = highlight_ranges(&text.to_string());
    }

    fn styles(
        &self,
        range: &Range<usize>,
        _resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let mut result = Vec::new();
        let mut cursor = range.start;
        for (highlight, color) in &self.ranges {
            let start = highlight.start.max(range.start);
            let end = highlight.end.min(range.end);
            if start >= end || end < cursor {
                continue;
            }
            let start = start.max(cursor);
            if cursor < start {
                result.push((cursor..start, HighlightStyle::default()));
            }
            result.push((start..end, HighlightStyle::color(*color)));
            cursor = end;
        }
        if cursor < range.end {
            result.push((cursor..range.end, HighlightStyle::default()));
        }
        result
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

/// Render a response body: pretty-printed + syntax-highlighted when JSON,
/// otherwise as plain text.
pub fn render_body(body: &str, window: &mut Window) -> impl IntoElement {
    if let Some(formatted) = try_format_json(body) {
        let mut style = window.text_style();
        style.color = theme::text_primary();
        style.font_size = px(13.0).into();
        let highlights = highlight_ranges(&formatted).into_iter().map(|(range, color)| {
            (range, gpui_kit::HighlightStyle::color(color))
        });
        let styled = StyledText::new(SharedString::from(formatted))
            .with_default_highlights(&style, highlights);
        div()
            .font_family("monospace")
            .child(styled)
    } else {
        let mut style = window.text_style();
        style.color = theme::text_primary();
        style.font_size = px(13.0).into();
        let styled = StyledText::new(SharedString::from(body.to_string()))
            .with_default_highlights(&style, std::iter::empty());
        div().font_family("monospace").child(styled)
    }
}


