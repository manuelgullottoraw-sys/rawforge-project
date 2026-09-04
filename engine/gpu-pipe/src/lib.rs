//! Definizione statica degli stage della pipeline GPU (docs/ARCHITECTURE.md, §3.2).
//!
//! Qui vive il DAG di stage e il sorgente WGSL di ciascuno. L'esecuzione reale
//! (creazione di `wgpu::Device`/`Queue`, bind group, dispatch) è volutamente
//! rimandata all'integrazione UniFFI (Fase 1 della roadmap): richiede un vero
//! adapter GPU per essere testata in modo significativo, cosa che una CI
//! headless non garantisce su tutti i runner. Gli shader stessi, però, sono
//! reali e vengono validati sintatticamente e semanticamente nei test qui sotto
//! tramite `naga` — lo stesso front-end WGSL usato internamente da `wgpu` — senza
//! bisogno di alcun hardware grafico.

pub struct PipelineStage {
    pub name: &'static str,
    pub wgsl_source: &'static str,
    pub entry_point: &'static str,
}

pub const WHITE_BALANCE: PipelineStage = PipelineStage {
    name: "white_balance",
    wgsl_source: include_str!("../shaders/white_balance.wgsl"),
    entry_point: "main",
};

pub const COLOR_GRADE: PipelineStage = PipelineStage {
    name: "color_grade",
    wgsl_source: include_str!("../shaders/color_grade.wgsl"),
    entry_point: "main",
};

/// Ordine di esecuzione degli stage attualmente definiti. Nella pipeline
/// completa (docs/ARCHITECTURE.md, §3.2) precedono e seguono altri stage
/// (esposizione, tone curve, output transform) non ancora presenti qui.
pub fn all_stages() -> Vec<&'static PipelineStage> {
    vec![&WHITE_BALANCE, &COLOR_GRADE]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid_wgsl(label: &str, source: &str) {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("[{label}] WGSL non valido: {e}"));

        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|e| panic!("[{label}] shader non valido semanticamente: {e}"));
    }

    #[test]
    fn white_balance_shader_is_valid() {
        assert_valid_wgsl(WHITE_BALANCE.name, WHITE_BALANCE.wgsl_source);
    }

    #[test]
    fn color_grade_shader_is_valid() {
        assert_valid_wgsl(COLOR_GRADE.name, COLOR_GRADE.wgsl_source);
    }

    #[test]
    fn all_stages_are_registered() {
        assert_eq!(all_stages().len(), 2);
    }
}
