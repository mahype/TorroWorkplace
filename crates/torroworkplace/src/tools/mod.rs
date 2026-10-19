//! The assistant's own tools (concept "Eigene Werkzeuge" in `docs/assistent.md`, issue #225): abilities only the
//! assistant and the bots have, each with a switch and settings of its own. The first one makes PDFs.
//!
//! - `pdf`: sets a PDF from Markdown and one of the three page templates (task WZ.3).

pub mod pdf;
