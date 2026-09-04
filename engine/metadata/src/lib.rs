//! Sidecar non distruttivo: schema e (de)serializzazione JSON.
//! Corrisponde esattamente all'esempio in `docs/ARCHITECTURE.md`, §3.1 —
//! ogni modifica dell'utente è un'operazione in una lista ordinata (`history`),
//! mai una scrittura sul file RAW originale.

use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", content = "params", rename_all = "snake_case")]
pub enum EditOperation {
    WhiteBalance { temp: u32, tint: i32 },
    Exposure { ev: f32 },
    ToneCurve { points: Vec<(u8, u8)> },
    Hsl { hue: [i32; 8], sat: [i32; 8], lum: [i32; 8] },
    SplitToning {
        shadow_hue: i32,
        shadow_sat: i32,
        highlight_hue: i32,
        highlight_sat: i32,
        balance: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartBatchDeltas {
    pub exposure_ev: f32,
    pub wb_temp_delta: i32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub schema_version: u32,
    pub source_file: String,
    pub source_hash: String,
    pub created_at: String,
    pub history: Vec<EditOperation>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub harmonic_look_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub smart_batch_deltas: Option<SmartBatchDeltas>,
}

impl Sidecar {
    pub fn new(source_file: impl Into<String>, source_hash: impl Into<String>, created_at: impl Into<String>) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            source_file: source_file.into(),
            source_hash: source_hash.into(),
            created_at: created_at.into(),
            history: Vec::new(),
            harmonic_look_ref: None,
            smart_batch_deltas: None,
        }
    }

    /// Aggiunge un'operazione in coda alla history (undo = pop dell'ultimo elemento).
    pub fn push_operation(&mut self, op: EditOperation) {
        self.history.push(op);
    }

    pub fn undo(&mut self) -> Option<EditOperation> {
        self.history.pop()
    }
}

pub fn load_sidecar(json: &str) -> serde_json::Result<Sidecar> {
    serde_json::from_str(json)
}

pub fn save_sidecar(sidecar: &Sidecar) -> serde_json::Result<String> {
    serde_json::to_string_pretty(sidecar)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_sidecar() -> Sidecar {
        let mut sc = Sidecar::new("IMG_0421.CR3", "blake3:8f2a...", "2026-09-04T10:12:00Z");
        sc.push_operation(EditOperation::WhiteBalance { temp: 5200, tint: 4 });
        sc.push_operation(EditOperation::Exposure { ev: 0.35 });
        sc.push_operation(EditOperation::ToneCurve {
            points: vec![(0, 0), (64, 58), (128, 130), (192, 205), (255, 255)],
        });
        sc.smart_batch_deltas = Some(SmartBatchDeltas {
            exposure_ev: -0.12,
            wb_temp_delta: -80,
            reason: "scene=backlit, clipped_highlight_frac=0.18".to_string(),
        });
        sc
    }

    #[test]
    fn round_trips_through_json() {
        let original = sample_sidecar();
        let json = save_sidecar(&original).expect("serialize");
        let parsed = load_sidecar(&json).expect("deserialize");
        assert_eq!(original, parsed);
    }

    #[test]
    fn matches_documented_schema_shape() {
        let json = save_sidecar(&sample_sidecar()).expect("serialize");
        // Verifica che l'enum "internamente taggato" produca esattamente la forma
        // {"op": "...", "params": {...}} descritta in docs/ARCHITECTURE.md §3.1.
        assert!(json.contains("\"op\": \"white_balance\""));
        assert!(json.contains("\"params\""));
        assert!(json.contains("\"temp\": 5200"));
    }

    #[test]
    fn undo_removes_last_operation() {
        let mut sc = sample_sidecar();
        let len_before = sc.history.len();
        let removed = sc.undo();
        assert!(removed.is_some());
        assert_eq!(sc.history.len(), len_before - 1);
    }
}
