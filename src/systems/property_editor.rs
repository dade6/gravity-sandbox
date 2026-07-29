use bevy::prelude::*;

/// Plugin for the property editor bridge (JS → ECS).
/// NOTE: The JS bridge (PROPERTY_COMMANDS) has been removed as part of T06-C.
/// Property editing is now handled by the native Bevy UI (SandboxUIPlugin
/// → sync_property_input_to_body). This file retains parse_hex_color and its
/// tests as standalone utility functions, usable by future native UI code.
pub struct PropertyEditorPlugin;

impl Plugin for PropertyEditorPlugin {
    fn build(&self, _app: &mut App) {
        // PropertyEditorPlugin is retained for future use.
        // System registration was removed in T06-C since the JS bridge
        // (PROPERTY_COMMANDS) no longer exists and native UI handles editing.
    }
}

/// Parse a hex color string like "#ff6600" or "#FFF" into [f32; 3] RGB.
/// Returns None if the string is invalid.
fn parse_hex_color(hex: &str) -> Option<[f32; 3]> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some([
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
        ])
    } else if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
        let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
        let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
        Some([
            (r as f32 * 17.0) / 255.0,
            (g as f32 * 17.0) / 255.0,
            (b as f32 * 17.0) / 255.0,
        ])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color_full() {
        assert_eq!(parse_hex_color("#ff6600"), Some([1.0, 0.4, 0.0]));
        assert_eq!(parse_hex_color("#000000"), Some([0.0, 0.0, 0.0]));
        assert_eq!(parse_hex_color("#ffffff"), Some([1.0, 1.0, 1.0]));
        assert_eq!(parse_hex_color("#FF0000"), Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn test_parse_hex_color_short() {
        assert_eq!(parse_hex_color("#f60"), Some([1.0, 0.4, 0.0]));
        assert_eq!(parse_hex_color("#fff"), Some([1.0, 1.0, 1.0]));
        assert_eq!(parse_hex_color("#000"), Some([0.0, 0.0, 0.0]));
    }

    #[test]
    fn test_parse_hex_color_invalid() {
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#xyz"), None);
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color("ff6600"), Some([1.0, 0.4, 0.0])); // without # prefix
    }
}
