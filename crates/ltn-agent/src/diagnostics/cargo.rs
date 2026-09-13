use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticSpan {
    pub file_name: String,
    pub line_start: usize,
    pub line_end: usize,
    pub column_start: usize,
    pub column_end: usize,
    pub is_primary: bool,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompilerDiagnosticMessage {
    pub reason: String,
    pub message: Option<String>,
    pub level: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub spans: Vec<DiagnosticSpan>,
}

pub struct CargoDiagnosticsParser;

impl CargoDiagnosticsParser {
    fn extract_diagnostic_message(msg: &serde_json::Value) -> Option<CompilerDiagnosticMessage> {
        let reason = msg.get("reason").and_then(|r| r.as_str())?;

        let level = msg
            .get("message")
            .and_then(|m| m.get("level"))
            .and_then(|l| l.as_str())
            .map(|s| s.to_string());

        let text = msg
            .get("message")
            .and_then(|m| m.get("rendered"))
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());

        let code = msg
            .get("message")
            .and_then(|m| m.get("code"))
            .and_then(|c| c.get("code"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        let mut spans = Vec::new();
        if let Some(span_array) = msg
            .get("message")
            .and_then(|m| m.get("spans"))
            .and_then(|s| s.as_array())
        {
            for span_val in span_array {
                let file_name = span_val
                    .get("file_name")
                    .and_then(|f| f.as_str())
                    .unwrap_or("")
                    .to_string();
                let line_start = span_val
                    .get("line_start")
                    .and_then(|l| l.as_u64())
                    .unwrap_or(0) as usize;
                let line_end = span_val
                    .get("line_end")
                    .and_then(|l| l.as_u64())
                    .unwrap_or(0) as usize;
                let column_start = span_val
                    .get("column_start")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0) as usize;
                let column_end = span_val
                    .get("column_end")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0) as usize;
                let is_primary = span_val
                    .get("is_primary")
                    .and_then(|p| p.as_bool())
                    .unwrap_or(false);
                let label = span_val
                    .get("label")
                    .and_then(|l| l.as_str())
                    .map(|s| s.to_string());

                spans.push(DiagnosticSpan {
                    file_name,
                    line_start,
                    line_end,
                    column_start,
                    column_end,
                    is_primary,
                    label,
                });
            }
        }

        Some(CompilerDiagnosticMessage {
            reason: reason.to_string(),
            message: text,
            level,
            code,
            spans,
        })
    }

    pub fn parse_json_stream(stream: &str) -> Vec<CompilerDiagnosticMessage> {
        let mut messages = Vec::new();
        for line in stream.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(parsed) = Self::extract_diagnostic_message(&msg) {
                    messages.push(parsed);
                }
            }
        }

        // 2. If no messages found, try parsing entire stream as a single multi-line formatted JSON object
        if messages.is_empty() {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(stream.trim()) {
                if let Some(parsed) = Self::extract_diagnostic_message(&msg) {
                    messages.push(parsed);
                }
            }
        }

        messages
    }

    pub fn filter_errors(msgs: &[CompilerDiagnosticMessage]) -> Vec<&CompilerDiagnosticMessage> {
        msgs.iter()
            .filter(|m| m.level.as_deref() == Some("error"))
            .collect()
    }

    pub fn filter_warnings(msgs: &[CompilerDiagnosticMessage]) -> Vec<&CompilerDiagnosticMessage> {
        msgs.iter()
            .filter(|m| m.level.as_deref() == Some("warning"))
            .collect()
    }

    pub fn has_errors(msgs: &[CompilerDiagnosticMessage]) -> bool {
        msgs.iter().any(|m| m.level.as_deref() == Some("error"))
    }

    pub fn is_multi_file_span(msg: &CompilerDiagnosticMessage) -> bool {
        let files: HashSet<&str> = msg
            .spans
            .iter()
            .map(|s| s.file_name.as_str())
            .filter(|f| !f.is_empty())
            .collect();
        files.len() > 1
    }

    pub fn primary_span(msg: &CompilerDiagnosticMessage) -> Option<&DiagnosticSpan> {
        msg.spans.iter().find(|s| s.is_primary)
    }
}
