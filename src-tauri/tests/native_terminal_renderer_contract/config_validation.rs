//! Contract test for Phase 2 renderer configuration validation.

use ferryx_lib::native_terminal::{NativeTerminalError, NativeTerminalRenderer, RendererConfig};

#[test]
fn test_zero_sized_physical_cell_configuration_rejected() {
    let test_cases = [
        (
            RendererConfig {
                cell_width_px: 0,
                cell_height_px: 20,
                device_scale_factor: 1.0,
                ..Default::default()
            },
            "zero cell width",
        ),
        (
            RendererConfig {
                cell_width_px: 10,
                cell_height_px: 0,
                device_scale_factor: 1.0,
                ..Default::default()
            },
            "zero cell height",
        ),
        (
            RendererConfig {
                cell_width_px: 0,
                cell_height_px: 0,
                device_scale_factor: 1.0,
                ..Default::default()
            },
            "zero cell width and height",
        ),
    ];

    for (config, description) in test_cases {
        let result = NativeTerminalRenderer::new(config);
        match result {
            Err(NativeTerminalError::InvalidDimensions(w, h)) => {
                assert!(
                    w == 0 || h == 0,
                    "InvalidDimensions must report 0 dimensions"
                );
            }
            Err(NativeTerminalError::InvalidValue(_)) => {
                // Accepted typed error variant
            }
            Err(other) => {
                panic!(
                    "Expected InvalidDimensions or InvalidValue error for {description}, got: {other:?}"
                );
            }
            Ok(_) => {
                panic!(
                    "Renderer creation with {description} (width={}, height={}) must be rejected with typed NativeTerminalError",
                    config.cell_width_px, config.cell_height_px
                );
            }
        }
    }
}
