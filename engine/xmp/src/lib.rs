//! Generatore di preset Lightroom (`.xmp`) a partire dai parametri calcolati
//! dal motore di Sintesi Armonica di RawForge.
//!
//! Vedi `docs/ARCHITECTURE.md`, sezione 5, per la spiegazione del mapping
//! verso il namespace `crs:` di Adobe Camera Raw.

use std::fmt::Write;

pub use core_types::{HarmonicLook, HslAdjustments, SplitToning, WhiteBalance};

const HUE_NAMES: [&str; 8] = [
    "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
];

/// Escape minimale per testo XML (usato per il nome del preset, l'unico campo
/// libero digitato dall'utente che finisce nel packet).
fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Genera il pacchetto XMP completo, importabile in Lightroom Classic/CC come
/// preset di sviluppo (cartella "Develop Presets").
pub fn generate_lightroom_xmp(look: &HarmonicLook) -> String {
    let mut curve = String::new();
    for (x, y) in &look.tone_curve {
        let _ = write!(curve, "<rdf:li>{}, {}</rdf:li>", x, y);
    }

    let mut hsl_fields = String::new();
    for (i, name) in HUE_NAMES.iter().enumerate() {
        let _ = write!(
            hsl_fields,
            "crs:HueAdjustment{n}=\"{h}\" crs:SaturationAdjustment{n}=\"{s}\" crs:LuminanceAdjustment{n}=\"{l}\"\n            ",
            n = name,
            h = look.hsl.hue[i],
            s = look.hsl.sat[i],
            l = look.hsl.lum[i]
        );
    }

    format!(
        r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="RawForge 1.0">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
        crs:PresetType="Normal"
        crs:Version="17.0"
        crs:ProcessVersion="{pv}"
        crs:WhiteBalance="Custom"
        crs:Temperature="{temp}"
        crs:Tint="{tint}"
        crs:Exposure2012="{exposure:.2}"
        crs:Contrast2012="{contrast}"
        crs:Highlights2012="{highlights}"
        crs:Shadows2012="{shadows}"
        crs:Whites2012="{whites}"
        crs:Blacks2012="{blacks}"
        crs:Vibrance="{vibrance}"
        crs:Saturation="{saturation}"
        crs:Luminance="{noise_luma}"
        crs:Color="{noise_color}"
        {hsl_fields}crs:SplitToningShadowHue="{sh_hue}"
        crs:SplitToningShadowSaturation="{sh_sat}"
        crs:SplitToningHighlightHue="{hl_hue}"
        crs:SplitToningHighlightSaturation="{hl_sat}"
        crs:SplitToningBalance="{balance}"
        crs:HasSettings="True">
      <crs:Name>
        <rdf:Alt><rdf:li xml:lang="x-default">{name}</rdf:li></rdf:Alt>
      </crs:Name>
      <crs:ToneCurvePV2012>
        <rdf:Seq>{curve}</rdf:Seq>
      </crs:ToneCurvePV2012>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"#,
        pv = look.process_version,
        temp = look.white_balance.temp,
        tint = look.white_balance.tint,
        exposure = look.exposure_ev,
        contrast = look.contrast,
        highlights = look.highlights,
        shadows = look.shadows,
        whites = look.whites,
        blacks = look.blacks,
        vibrance = look.vibrance,
        saturation = look.saturation,
        noise_luma = look.noise_reduction_luma,
        noise_color = look.noise_reduction_color,
        hsl_fields = hsl_fields,
        sh_hue = look.split_toning.shadow_hue,
        sh_sat = look.split_toning.shadow_sat,
        hl_hue = look.split_toning.highlight_hue,
        hl_sat = look.split_toning.highlight_sat,
        balance = look.split_toning.balance,
        name = escape_xml_text(&look.name),
        curve = curve,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_well_formed_xmp_packet() {
        let look = HarmonicLook {
            name: "Cinematic Teal & Orange".to_string(),
            exposure_ev: 0.35,
            contrast: 12,
            ..Default::default()
        };

        let xmp = generate_lightroom_xmp(&look);

        assert!(xmp.starts_with("<?xpacket begin="));
        assert!(xmp.trim_end().ends_with("<?xpacket end=\"w\"?>"));
        assert!(xmp.contains("crs:Exposure2012=\"0.35\""));
        assert!(xmp.contains("crs:Contrast2012=\"12\""));
        assert!(xmp.contains("Cinematic Teal &amp; Orange"));
        assert!(xmp.contains("crs:HueAdjustmentRed"));
        assert!(xmp.contains("crs:HueAdjustmentMagenta"));
        assert!(xmp.contains("<rdf:li>0, 0</rdf:li>"));
        assert!(xmp.contains("<rdf:li>255, 255</rdf:li>"));
    }

    #[test]
    fn default_look_round_trips_neutral_values() {
        let look = HarmonicLook::default();
        let xmp = generate_lightroom_xmp(&look);
        assert!(xmp.contains("crs:Temperature=\"5500\""));
        assert!(xmp.contains("crs:Exposure2012=\"0.00\""));
        assert!(xmp.contains("crs:Luminance=\"0\""), "riduzione rumore di default deve esportare 0, non essere omessa");
        assert!(xmp.contains("crs:Color=\"0\""));
    }

    #[test]
    fn noise_reduction_amounts_are_exported_to_the_real_lightroom_detail_panel_tags() {
        let look = HarmonicLook { noise_reduction_luma: 40, noise_reduction_color: 65, ..Default::default() };
        let xmp = generate_lightroom_xmp(&look);
        assert!(xmp.contains("crs:Luminance=\"40\""));
        assert!(xmp.contains("crs:Color=\"65\""));
    }
}
