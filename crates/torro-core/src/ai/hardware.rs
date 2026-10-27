//! What this computer offers a local model (issue #325): memory, processor and graphics card, as
//! the core measured them (`torroworkplace::account::hardware`, task MW.4). Only the types live
//! here; `ai.recommend` answers with them and picks a tier of models by them (task MW.5).
//!
//! What could not be measured is `None`, never a guess: a recommendation then takes the smallest
//! tier rather than a model the computer cannot run.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// This computer as measured: what decides which local models run well on it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Hardware {
    /// The processor's name as the system reports it (`Apple M3 Max`, `AMD Ryzen 9 7950X`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    /// The main memory in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
    /// The largest graphics card; `None` when none was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<Gpu>,
    /// Processor and graphics share the main memory (Apple silicon): a model may then use most of
    /// [`Hardware::memory_bytes`], and there is no video memory of its own.
    #[serde(default)]
    pub unified_memory: bool,
}

/// A graphics card as measured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Gpu {
    /// Its name as the driver reports it (`NVIDIA GeForce RTX 4070`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its own video memory in bytes; `None` when the driver does not say or the memory is shared
    /// ([`Hardware::unified_memory`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_not_measured_is_left_out_and_reads_back_as_none() {
        let unknown = Hardware::default();
        assert_eq!(serde_json::to_value(&unknown).expect("json"), serde_json::json!({"unified_memory": false}));
        assert_eq!(serde_json::from_value::<Hardware>(serde_json::json!({})).expect("back"), unknown);
        let card = Hardware {
            cpu: Some("AMD Ryzen 9 7950X".into()),
            memory_bytes: Some(64 << 30),
            gpu: Some(Gpu { name: Some("NVIDIA GeForce RTX 4070".into()), vram_bytes: Some(12 << 30) }),
            unified_memory: false,
        };
        let json = serde_json::to_value(&card).expect("json");
        assert_eq!(json["gpu"]["vram_bytes"], serde_json::json!(12u64 << 30));
        assert_eq!(serde_json::from_value::<Hardware>(json).expect("back"), card);
    }
}
