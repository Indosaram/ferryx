use crate::browser::BrowserError;
use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::remote::design_mode::{DesignModeSnapshot, DomElementBox};
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHighlightResult {
    pub selector: String,
    pub tag_name: String,
    pub rect: ElementRect,
}

pub struct ElementPick {
    pub outer_html: String,
    pub css: String,
    pub rect: ElementRect,
    pub device_pixel_ratio: f64,
}

pub fn unwrap_eval_json(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(inner) = serde_json::from_str::<String>(trimmed) {
        inner
    } else {
        trimmed.to_string()
    }
}

pub fn parse_element_pick(json: &str) -> Result<ElementPick, String> {
    let unwrapped = unwrap_eval_json(json);
    let v: serde_json::Value = serde_json::from_str(&unwrapped).map_err(|e| e.to_string())?;
    let object = if let Some(text) = v.as_str() {
        serde_json::from_str::<serde_json::Value>(text).map_err(|e| e.to_string())?
    } else {
        v
    };
    let outer_html = object
        .get("outerHTML")
        .and_then(|x| x.as_str())
        .ok_or("missing outerHTML")?
        .to_string();
    let css = object
        .get("css")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let rect = object.get("rect").ok_or("missing rect")?;
    let device_pixel_ratio = object
        .get("devicePixelRatio")
        .and_then(|n| n.as_f64())
        .filter(|n| n.is_finite() && *n > 0.0)
        .unwrap_or(1.0);
    Ok(ElementPick {
        outer_html,
        css,
        device_pixel_ratio,
        rect: ElementRect {
            x: rect.get("x").and_then(|n| n.as_f64()).unwrap_or(0.0),
            y: rect.get("y").and_then(|n| n.as_f64()).unwrap_or(0.0),
            width: rect.get("width").and_then(|n| n.as_f64()).unwrap_or(0.0),
            height: rect.get("height").and_then(|n| n.as_f64()).unwrap_or(0.0),
        },
    })
}


pub fn element_pick_to_design_snapshot(pick: &ElementPick, png: &[u8]) -> DesignModeSnapshot {
    let tag = pick
        .outer_html
        .trim_start()
        .strip_prefix('<')
        .map(|rest| {
            let end = rest
                .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
                .unwrap_or(rest.len());
            rest[..end].to_ascii_lowercase()
        })
        .unwrap_or_default();
    DesignModeSnapshot {
        session_id: String::new(),
        timestamp_ms: 0,
        screenshot_png_base64: base64::engine::general_purpose::STANDARD.encode(png),
        outer_html: pick.outer_html.clone(),
        css: pick.css.clone(),
        dom_elements: vec![DomElementBox {
            id: String::new(),
            tag,
            bounds: [pick.rect.x, pick.rect.y, pick.rect.width, pick.rect.height],
            text: None,
        }],
    }
}

pub fn element_pick_report(json: &str, full_png: &[u8]) -> Result<DesignModeSnapshot, String> {
    let pick = parse_element_pick(json)?;
    if pick.rect.width <= 0.0 || pick.rect.height <= 0.0 {
        return Err("empty element rect".to_string());
    }
    let img = image::load_from_memory(full_png).map_err(|e| e.to_string())?;
    if img.width() == 0 || img.height() == 0 {
        return Err("empty screenshot".to_string());
    }
    let dpr = pick.device_pixel_ratio;
    // Clamp both edges to the screenshot: clamping only the origin keeps the full size and
    // crops pixels the element does not cover when it is partly scrolled off-screen.
    let left = (pick.rect.x * dpr).floor();
    let top = (pick.rect.y * dpr).floor();
    let right = ((pick.rect.x + pick.rect.width) * dpr).ceil();
    let bottom = ((pick.rect.y + pick.rect.height) * dpr).ceil();
    let x1 = left.max(0.0);
    let y1 = top.max(0.0);
    let x2 = right.min(f64::from(img.width()));
    let y2 = bottom.min(f64::from(img.height()));
    if x2 <= x1 || y2 <= y1 {
        return Err("element rect is outside the screenshot".to_string());
    }
    let (x, y) = (x1 as u32, y1 as u32);
    let (cw, ch) = ((x2 - x1) as u32, (y2 - y1) as u32);
    if cw == 0 || ch == 0 {
        return Err("empty crop".to_string());
    }
    let cropped = img.crop_imm(x, y, cw, ch);
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cropped
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(element_pick_to_design_snapshot(&pick, &bytes))
}

pub fn browser_highlight_script(selector: &str, duration_ms: u64) -> Result<String, BrowserError> {
    let selector_json = serde_json::to_string(selector).map_err(|error| {
        BrowserError::AutomationFailed(format!("failed to encode selector: {error}"))
    })?;
    Ok(format!(
        r#"(() => {{
  const selector = {selector_json};
  if (!selector || typeof selector !== 'string' || !selector.trim()) {{
    return JSON.stringify({{ ok: false, error: 'invalid_selector', message: 'selector cannot be empty' }});
  }}

  let element;
  try {{
    element = document.querySelector(selector);
  }} catch (e) {{
    return JSON.stringify({{ ok: false, error: 'invalid_selector', message: e && e.message ? e.message : 'invalid selector syntax' }});
  }}

  if (!element) {{
    return JSON.stringify({{ ok: false, error: 'not_found', message: 'no element matched selector' }});
  }}

  if (typeof element.scrollIntoView === 'function') {{
    element.scrollIntoView({{ block: 'nearest', inline: 'nearest' }});
  }}

  const prev = window.__ferryxHighlight;
  if (prev) {{
    if (prev.timer) clearTimeout(prev.timer);
    if (prev.overlay && prev.overlay.parentNode) {{
      prev.overlay.parentNode.removeChild(prev.overlay);
    }}
    if (prev.outlinedEl) {{
      prev.outlinedEl.style.outline = prev.prevOutline;
    }}
    window.__ferryxHighlight = null;
  }}

  const rect = element.getBoundingClientRect();
  const prevOutline = element.style.outline || '';
  element.style.outline = '2px solid #ff9800';

  const overlay = document.createElement('div');
  overlay.id = '__ferryx_highlight_overlay';
  overlay.style.position = 'fixed';
  overlay.style.left = (rect.left - 2) + 'px';
  overlay.style.top = (rect.top - 2) + 'px';
  overlay.style.width = (rect.width + 4) + 'px';
  overlay.style.height = (rect.height + 4) + 'px';
  overlay.style.border = '2px solid #ff9800';
  overlay.style.backgroundColor = 'rgba(255, 152, 0, 0.2)';
  overlay.style.pointerEvents = 'none';
  overlay.style.zIndex = '2147483647';
  overlay.style.boxSizing = 'border-box';
  overlay.style.transition = 'opacity 0.2s ease-out';
  (document.body || document.documentElement).appendChild(overlay);

  const cleanup = () => {{
    if (overlay.parentNode) {{
      overlay.parentNode.removeChild(overlay);
    }}
    if (element) {{
      element.style.outline = prevOutline;
    }}
    if (window.__ferryxHighlight && window.__ferryxHighlight.overlay === overlay) {{
      window.__ferryxHighlight = null;
    }}
  }};

  const timer = setTimeout(cleanup, {duration_ms});

  window.__ferryxHighlight = {{
    overlay,
    timer,
    outlinedEl: element,
    prevOutline,
    cleanup,
  }};

  return JSON.stringify({{
    ok: true,
    selector,
    tagName: element.tagName.toLowerCase(),
    rect: {{
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height
    }}
  }});
}})()"#,
        duration_ms = duration_ms.max(100)
    ))
}

pub const BROWSER_CLEAR_HIGHLIGHT_SCRIPT: &str = r#"(() => {
  const prev = window.__ferryxHighlight;
  if (!prev) return false;
  if (prev.timer) clearTimeout(prev.timer);
  if (prev.overlay && prev.overlay.parentNode) {
    prev.overlay.parentNode.removeChild(prev.overlay);
  }
  if (prev.outlinedEl) {
    prev.outlinedEl.style.outline = prev.prevOutline;
  }
  window.__ferryxHighlight = null;
  return true;
})()"#;

#[derive(Deserialize)]
struct RawHighlightCallback {
    ok: bool,
    error: Option<String>,
    message: Option<String>,
    selector: Option<String>,
    #[serde(rename = "tagName")]
    tag_name: Option<String>,
    rect: Option<ElementRect>,
}

pub fn parse_browser_highlight_callback(
    raw: &str,
    requested_selector: &str,
) -> Result<BrowserHighlightResult, IpcError> {
    let unwrapped = unwrap_eval_json(raw);
    let callback: RawHighlightCallback = serde_json::from_str(&unwrapped).map_err(|e| {
        IpcError::new(
            IpcErrorCode::ParseError,
            format!("failed to parse highlight callback JSON: {e}"),
        )
    })?;

    if !callback.ok {
        let err_kind = callback.error.as_deref().unwrap_or("unknown");
        let msg = callback
            .message
            .unwrap_or_else(|| "highlight failed".to_string());
        return match err_kind {
            "not_found" => Err(IpcError::new(
                IpcErrorCode::BrowserAutomationTargetNotFound,
                format!("no element matched selector '{requested_selector}'"),
            )),
            "invalid_selector" => Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                format!("invalid selector '{requested_selector}': {msg}"),
            )),
            _ => Err(IpcError::new(
                IpcErrorCode::BrowserAutomationFailed,
                format!("highlight failed for '{requested_selector}': {msg}"),
            )),
        };
    }

    let rect = callback.rect.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::ParseError,
            "highlight callback missing rect",
        )
    })?;

    Ok(BrowserHighlightResult {
        selector: callback
            .selector
            .unwrap_or_else(|| requested_selector.to_string()),
        tag_name: callback.tag_name.unwrap_or_default(),
        rect,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_element_pick_fields() {
        let p = parse_element_pick("{\"outerHTML\":\"<button>\",\"css\":\"color:red\",\"rect\":{\"x\":1,\"y\":2,\"width\":3,\"height\":4}}").unwrap();
        assert_eq!(p.outer_html, "<button>");
        assert_eq!(p.css, "color:red");
        assert_eq!(p.rect.width, 3.0);
        assert_eq!(p.device_pixel_ratio, 1.0);
    }

    #[test]
    fn unwraps_eval_quoted_element_pick() {
        let quoted = serde_json::to_string(
            &r#"{"outerHTML":"<button>","css":"color:red","devicePixelRatio":2,"rect":{"x":1,"y":2,"width":3,"height":4}}"#,
        )
        .unwrap();
        let p = parse_element_pick(&quoted).unwrap();
        assert_eq!(p.outer_html, "<button>");
        assert_eq!(p.css, "color:red");
        assert_eq!(p.device_pixel_ratio, 2.0);
    }

    #[test]
    fn rejects_element_pick_without_html() {
        assert!(parse_element_pick("{}").is_err());
    }

    #[test]
    fn element_pick_report_crops_png_into_design_snapshot() {
        use image::{ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(2, 2, |x, y| Rgb([x as u8, y as u8, 1]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let json = "{\"outerHTML\":\"<button>\",\"css\":\"color:red\",\"rect\":{\"x\":0,\"y\":0,\"width\":1,\"height\":1}}";
        let snapshot = element_pick_report(json, &png).unwrap();
        let cropped = base64::engine::general_purpose::STANDARD
            .decode(&snapshot.screenshot_png_base64)
            .unwrap();
        let decoded = image::load_from_memory(&cropped).unwrap();
        assert_eq!(decoded.width(), 1);
        assert_eq!(decoded.height(), 1);
        assert_eq!(snapshot.dom_elements[0].bounds, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(snapshot.outer_html, "<button>");
        assert_eq!(snapshot.css, "color:red");
    }

    #[test]
    fn element_pick_report_scales_crop_by_device_pixel_ratio() {
        use image::{ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(4, 4, |x, y| Rgb([x as u8, y as u8, 1]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let json = "{\"outerHTML\":\"<div>\",\"css\":\"color:blue\",\"devicePixelRatio\":2,\"rect\":{\"x\":0,\"y\":0,\"width\":1,\"height\":1}}";
        let snapshot = element_pick_report(json, &png).unwrap();
        let cropped = base64::engine::general_purpose::STANDARD
            .decode(&snapshot.screenshot_png_base64)
            .unwrap();
        let decoded = image::load_from_memory(&cropped).unwrap();
        assert_eq!(decoded.width(), 2);
        assert_eq!(decoded.height(), 2);
    }

    #[test]
    fn element_pick_report_rejects_empty_rect() {
        use image::{ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(2, 2, Rgb([0, 0, 0]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let json = "{\"outerHTML\":\"<button>\",\"css\":\"\",\"rect\":{\"x\":0,\"y\":0,\"width\":0,\"height\":1}}";
        assert!(element_pick_report(json, &png).is_err());
    }

    #[test]
    fn picker_script_uses_only_the_narrow_pick_ready_hook() {
        // Red-phase lock for the trusted-bridge picker (design 3.6): the picker scripts moved
        // to the isolated world, keep their state off page-visible globals and accept only
        // trusted clicks. Writing a pick into a page global let any page fake one.
        use crate::browser::bridge::{PICKER_ARM_SCRIPT, PICKER_DISARM_SCRIPT, PICKER_READ_SCRIPT};
        for script in [PICKER_ARM_SCRIPT, PICKER_READ_SCRIPT, PICKER_DISARM_SCRIPT] {
            assert!(!script.contains("window.__ferryxElementPick ="));
            assert!(!script.contains("__ferryxPickReady"));
            // A-CORE-1: the picker must not depend on a page-visible nonce router.
            assert!(!script.contains("__ferryxRoute"));
            assert!(!script.contains("pick.ferryx.invalid"));
            assert!(!script.contains("nonce"));
        }
        assert!(PICKER_ARM_SCRIPT.contains("isTrusted"));
        assert!(PICKER_ARM_SCRIPT.contains("removeEventListener"));
        assert!(PICKER_ARM_SCRIPT.contains("devicePixelRatio"));
        assert!(PICKER_DISARM_SCRIPT.contains("picker.disarm()"));
    }

    #[test]
    fn element_pick_report_clamps_both_edges_of_a_partly_offscreen_rect() {
        // A-CORE-9: clamping only the origin kept the full height, cropping rows the
        // element does not cover.
        use image::{ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(4, 4, |x, y| Rgb([x as u8, y as u8, 1]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let json = "{\"outerHTML\":\"<div>\",\"css\":\"\",\"rect\":{\"x\":0,\"y\":-1,\"width\":2,\"height\":2}}";
        let snapshot = element_pick_report(json, &png).unwrap();
        let cropped = base64::engine::general_purpose::STANDARD
            .decode(&snapshot.screenshot_png_base64)
            .unwrap();
        let decoded = image::load_from_memory(&cropped).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 1));

        let right = "{\"outerHTML\":\"<div>\",\"css\":\"\",\"rect\":{\"x\":3,\"y\":0,\"width\":3,\"height\":1}}";
        let snapshot = element_pick_report(right, &png).unwrap();
        let cropped = base64::engine::general_purpose::STANDARD
            .decode(&snapshot.screenshot_png_base64)
            .unwrap();
        let decoded = image::load_from_memory(&cropped).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (1, 1));

        let above = "{\"outerHTML\":\"<div>\",\"css\":\"\",\"rect\":{\"x\":0,\"y\":-5,\"width\":2,\"height\":2}}";
        assert!(element_pick_report(above, &png).is_err());
    }

    #[test]
    fn element_pick_to_design_snapshot_stores_cropped_png_and_rect() {
        use crate::remote::design_mode::{DesignModeSnapshot, DomElementBox};
        const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
        let png = base64::engine::general_purpose::STANDARD
            .decode(PNG_B64)
            .expect("1x1 png");
        assert!(png.starts_with(b"\x89PNG"));
        let pick = ElementPick {
            outer_html: "<button>Submit</button>".to_string(),
            css: "color:red".to_string(),
            device_pixel_ratio: 1.0,
            rect: ElementRect {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 40.0,
            },
        };
        let snapshot: DesignModeSnapshot = element_pick_to_design_snapshot(&pick, &png);
        assert_eq!(snapshot.screenshot_png_base64, PNG_B64);
        assert_eq!(snapshot.outer_html, "<button>Submit</button>");
        assert_eq!(snapshot.css, "color:red");
        assert_eq!(snapshot.session_id, "");
        assert_eq!(snapshot.timestamp_ms, 0);
        assert_eq!(
            snapshot.dom_elements,
            vec![DomElementBox {
                id: String::new(),
                tag: "button".to_string(),
                bounds: [10.0, 20.0, 100.0, 40.0],
                text: None,
            }]
        );
    }

    #[test]
    fn test_browser_highlight_script_encodes_selector_and_duration() {
        let selector = "button.btn[data-id='submit']";
        let script = browser_highlight_script(selector, 1500).unwrap();
        assert!(script.contains(&serde_json::to_string(selector).unwrap()));
        assert!(script.contains("document.querySelector(selector)"));
        assert!(script.contains("element.scrollIntoView"));
        assert!(script.contains("__ferryx_highlight_overlay"));
        assert!(script.contains("1500"));
    }

    #[test]
    fn test_browser_highlight_script_has_safe_error_reporting() {
        let script = browser_highlight_script("#main", 2000).unwrap();
        assert!(script.contains("invalid_selector"));
        assert!(script.contains("not_found"));
        assert!(script.contains("__ferryxHighlight"));
        assert!(script.contains("clearTimeout"));
        assert!(script.contains("removeChild"));
    }

    #[test]
    fn test_clear_highlight_script_contract() {
        assert!(BROWSER_CLEAR_HIGHLIGHT_SCRIPT.contains("__ferryxHighlight"));
        assert!(BROWSER_CLEAR_HIGHLIGHT_SCRIPT.contains("clearTimeout"));
        assert!(BROWSER_CLEAR_HIGHLIGHT_SCRIPT.contains("removeChild"));
    }

    #[test]
    fn test_parse_browser_highlight_callback_success() {
        let raw = r#"{"ok":true,"selector":"button.submit","tagName":"button","rect":{"x":10.0,"y":20.0,"width":100.0,"height":30.0}}"#;
        let res = parse_browser_highlight_callback(raw, "button.submit").unwrap();
        assert_eq!(res.selector, "button.submit");
        assert_eq!(res.tag_name, "button");
        assert_eq!(res.rect.x, 10.0);
        assert_eq!(res.rect.width, 100.0);
    }

    #[test]
    fn test_parse_browser_highlight_callback_unwraps_quoted_json() {
        let raw = r##""{\"ok\":true,\"selector\":\"#header\",\"tagName\":\"div\",\"rect\":{\"x\":0.0,\"y\":0.0,\"width\":800.0,\"height\":60.0}}""##;
        let res = parse_browser_highlight_callback(raw, "#header").unwrap();
        assert_eq!(res.selector, "#header");
        assert_eq!(res.tag_name, "div");
        assert_eq!(res.rect.width, 800.0);
    }

    #[test]
    fn test_parse_browser_highlight_callback_not_found() {
        let raw = r#"{"ok":false,"error":"not_found","message":"no element matched selector"}"#;
        let err = parse_browser_highlight_callback(raw, ".nonexistent").unwrap_err();
        assert_eq!(err.code, IpcErrorCode::BrowserAutomationTargetNotFound);
        assert!(err.message.contains(".nonexistent"));
    }

    #[test]
    fn test_parse_browser_highlight_callback_invalid_selector() {
        let raw = r#"{"ok":false,"error":"invalid_selector","message":"SyntaxError: Failed to execute 'querySelector'"}"#;
        let err = parse_browser_highlight_callback(raw, ":::bad").unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);
        assert!(err.message.contains(":::bad"));
    }
}
