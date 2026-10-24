//! MCP servers connected to the suite (issue #302, `docs/tasks/mcp-anbindungen.md`): the core is their MCP
//! client and offers their tools to the assistant, its bots and MCP clients. The types on the wire and the
//! rules for ids and names are `torro_ipc::extensions`; this module holds the logic.
//!
//! - `catalog`: the built-in catalog (`assets/extensions/catalog.toml`), read and checked (task MA.2).
//! - `prereq`: whether the programs a server needs (`npx`, `uvx`, `docker`) are on this computer (task MA.2).
//! - `store`: `<data>/extensions/extensions.json` and the servers' secrets in the keychain (task MA.3).
//! - `env`: the environment and headers a server gets, and which addresses may be used (task MA.3).
//! - `client`: the MCP connection to one server, and what a person allowed compared with what it reports
//!   (task MA.4).
//! - `supervisor`: the processes and connections of all servers, their time limits and restarts (task MA.4).

pub mod catalog;
// The handlers of task MA.5 are the first to start servers; until then only tests and the end of the service
// (`Supervisor::stop_all`) reach this code.
#[allow(dead_code, reason = "the supervisor uses it; the handlers of task MA.5 start servers through it")]
pub mod client;
#[allow(dead_code, reason = "the supervisor uses it; the handlers of task MA.5 start servers through it")]
pub mod env;
pub mod prereq;
#[allow(dead_code, reason = "the handlers of task MA.5 read and write it")]
pub mod store;
#[allow(dead_code, reason = "the handlers of task MA.5 start servers through it; the service stops it")]
pub mod supervisor;

use std::sync::Arc;

use torro_ipc::extensions::CatalogEntry;

use self::catalog::CatalogError;
use self::prereq::Roots;
use self::supervisor::{Supervisor, Timing};

/// The core's MCP servers. [`Extensions::system`] looks for programs where the service does; tests bring
/// their own roots ([`Extensions::with_roots`]).
#[derive(Debug, Clone)]
pub struct Extensions {
    /// Where the programs a server needs are looked for.
    roots: Roots,
    /// Their processes and connections.
    supervisor: Arc<Supervisor>,
}

impl Extensions {
    /// The service's: programs on its `PATH` and in the usual folders ([`Roots::system`]), run by the
    /// service's supervisor ([`Supervisor::service`]), which the end of the service stops.
    pub fn system() -> Extensions {
        Extensions { roots: Roots::system(), supervisor: Supervisor::service() }
    }

    /// Programs looked for in `roots` only, run by a supervisor of its own with an empty environment of the
    /// service and the usual times.
    pub fn with_roots(roots: Roots) -> Extensions {
        let supervisor = Arc::new(Supervisor::new(roots.clone(), Default::default(), Timing::default()));
        Extensions { roots, supervisor }
    }

    /// The supervisor that starts, calls and ends the servers.
    #[allow(dead_code, reason = "the handlers of task MA.5 use it")]
    pub fn supervisor(&self) -> &Arc<Supervisor> {
        &self.supervisor
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
