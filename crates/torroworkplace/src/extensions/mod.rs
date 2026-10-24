//! MCP servers connected to the suite (issue #302, `docs/tasks/mcp-anbindungen.md`): the core is their MCP
//! client and offers their tools to the assistant, its bots and MCP clients. The types on the wire and the
//! rules for ids and names are `torro_ipc::extensions`; this module holds the logic.
//!
//! - `catalog`: the built-in catalog (`assets/extensions/catalog.toml`), read and checked (task MA.2).
//! - `prereq`: whether the programs a server needs (`npx`, `uvx`, `docker`) are on this computer (task MA.2).

pub mod catalog;
pub mod prereq;

use torro_ipc::extensions::CatalogEntry;

use self::catalog::CatalogError;
use self::prereq::Roots;

/// The core's MCP servers. [`Extensions::system`] looks for programs where the service does; tests bring
/// their own roots ([`Extensions::with_roots`]).
#[derive(Debug, Clone)]
pub struct Extensions {
    /// Where the programs a server needs are looked for.
    roots: Roots,
}

impl Extensions {
    /// The service's: programs on its `PATH` and in the usual folders ([`Roots::system`]).
    pub fn system() -> Extensions {
        Extensions { roots: Roots::system() }
    }

    /// Programs looked for in `roots` only.
    pub fn with_roots(roots: Roots) -> Extensions {
        Extensions { roots }
    }

    /// `extensions.list_catalog`: the built-in entries in their order, each with what of its prerequisites
    /// is missing on this computer now ([`CatalogEntry::missing`]). Looked up anew on every call, so a
    /// program installed meanwhile counts without a restart. `Err` only for a broken built-in catalog.
    pub fn catalog(&self) -> Result<Vec<CatalogEntry>, CatalogError> {
        let entries = catalog::builtin()?;
        Ok(entries
            .iter()
            .map(|entry| CatalogEntry {
                missing: prereq::missing(&entry.info.requires, &self.roots),
                ..entry.info.clone()
            })
            .collect())
    }
}
