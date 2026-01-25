//! Desktop background renderer wrapper
//!
//! WASM-bindgen wrapper for the WebGPU background renderer from zos-desktop.

use wasm_bindgen::prelude::*;
use zos_desktop::background;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// WASM-bindgen wrapper for the background renderer
#[wasm_bindgen]
pub struct DesktopBackground {
    renderer: Option<background::BackgroundRenderer>,
}

#[wasm_bindgen]
impl DesktopBackground {
    /// Create a new desktop background (uninitialized)
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { renderer: None }
    }

    /// Initialize the background renderer with a canvas element
    /// Returns a Promise that resolves when ready
    #[wasm_bindgen]
    pub async fn init(&mut self, canvas: web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
        log("[background] Initializing WebGPU background renderer...");

        match background::BackgroundRenderer::new(canvas).await {
            Ok(renderer) => {
                log("[background] WebGPU background renderer initialized successfully");
                self.renderer = Some(renderer);
                Ok(())
            }
            Err(e) => {
                log(&format!(
                    "[background] Failed to initialize renderer: {}",
                    e
                ));
                Err(JsValue::from_str(&e))
            }
        }
    }

    /// Check if the renderer is initialized
    #[wasm_bindgen]
    pub fn is_initialized(&self) -> bool {
        self.renderer.is_some()
    }

    /// Resize the renderer
    #[wasm_bindgen]
    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(renderer) = &mut self.renderer {
            renderer.resize(width, height);
        }
    }

    /// Render a frame
    #[wasm_bindgen]
    pub fn render(&mut self) -> Result<(), JsValue> {
        if let Some(renderer) = &mut self.renderer {
            renderer.render().map_err(|e| JsValue::from_str(&e))
        } else {
            Err(JsValue::from_str("Renderer not initialized"))
        }
    }

    /// Get all available background types as JSON
    /// Returns: [{ "id": "grain", "name": "Film Grain" }, ...]
    #[wasm_bindgen]
    pub fn get_available_backgrounds(&self) -> String {
        let backgrounds: Vec<serde_json::Value> = background::BackgroundType::all()
            .iter()
            .map(|bg| {
                serde_json::json!({
                    "id": format!("{:?}", bg).to_lowercase(),
                    "name": bg.name()
                })
            })
            .collect();
        serde_json::to_string(&backgrounds).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get the current background type ID
    #[wasm_bindgen]
    pub fn get_current_background(&self) -> String {
        if let Some(renderer) = &self.renderer {
            format!("{:?}", renderer.current_background()).to_lowercase()
        } else {
            "grain".to_string()
        }
    }

    /// Set the background type by ID (e.g., "grain", "mist")
    /// Returns true if successful, false if ID is invalid
    #[wasm_bindgen]
    pub fn set_background(&mut self, id: &str) -> bool {
        let bg_type = match id.to_lowercase().as_str() {
            "grain" => background::BackgroundType::Grain,
            "mist" => background::BackgroundType::Mist,
            _ => return false,
        };

        if let Some(renderer) = &mut self.renderer {
            renderer.set_background(bg_type);
            log(&format!("[background] Switched to: {}", bg_type.name()));
            true
        } else {
            false
        }
    }

    /// Set viewport state for zoom effects
    /// Called before render to update zoom level and camera position
    #[wasm_bindgen]
    pub fn set_viewport(&mut self, zoom: f32, center_x: f32, center_y: f32) {
        if let Some(renderer) = &mut self.renderer {
            renderer.set_viewport(zoom, center_x, center_y);
        }
    }

    /// Set workspace info for multi-workspace rendering when zoomed out
    /// backgrounds_json should be a JSON array of background type strings, e.g. ["grain", "mist"]
    #[wasm_bindgen]
    pub fn set_workspace_info(&mut self, count: usize, active: usize, backgrounds_json: &str) {
        if let Some(renderer) = &mut self.renderer {
            // Parse background types from JSON
            let backgrounds: Vec<background::BackgroundType> =
                serde_json::from_str::<Vec<String>>(backgrounds_json)
                    .unwrap_or_default()
                    .iter()
                    .map(|s| background::BackgroundType::from_id(s).unwrap_or_default())
                    .collect();

            renderer.set_workspace_info(count, active, &backgrounds);
        }
    }

    /// Set whether we're transitioning between workspaces
    /// Only during transitions can you see other workspaces
    #[wasm_bindgen]
    pub fn set_transitioning(&mut self, transitioning: bool) {
        if let Some(renderer) = &mut self.renderer {
            renderer.set_transitioning(transitioning);
        }
    }

    /// Set workspace layout dimensions (must match Rust desktop engine)
    /// Called when workspaces are created or screen is resized
    #[wasm_bindgen]
    pub fn set_workspace_dimensions(&mut self, width: f32, height: f32, gap: f32) {
        if let Some(renderer) = &mut self.renderer {
            renderer.set_workspace_dimensions(width, height, gap);
        }
    }
}

impl Default for DesktopBackground {
    fn default() -> Self {
        Self::new()
    }
}
