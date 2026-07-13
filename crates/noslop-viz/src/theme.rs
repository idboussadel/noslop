//! Color styling for rendered cells. Honors the caller's `color` flag (the CLI
//! disables it for `NO_COLOR` and non-terminals).

use owo_colors::{OwoColorize, Style as OwoStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellStyle {
    /// Plain — never painted.
    None,
    /// A package box (border + label).
    Node,
    /// A box that participates in an import cycle.
    Cycle,
    /// An edge line or arrowhead.
    Edge,
}

pub fn paint(s: &str, style: CellStyle, color: bool) -> String {
    if !color || s.is_empty() || style == CellStyle::None {
        return s.to_string();
    }
    let owo = match style {
        CellStyle::None => return s.to_string(),
        CellStyle::Node => OwoStyle::new().cyan().bold(),
        CellStyle::Cycle => OwoStyle::new().red().bold(),
        CellStyle::Edge => OwoStyle::new().bright_black(),
    };
    s.style(owo).to_string()
}
