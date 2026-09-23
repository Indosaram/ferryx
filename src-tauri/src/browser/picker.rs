use crate::remote::design_mode::{DesignModeSnapshot, DomElementBox};
use base64::Engine;

pub struct ElementRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
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

pub const ELEMENT_PICKER_SCRIPT: &str = r#"(() => {
  const previous = window.__ferryxPicker;
  if (previous && previous.installed) {
    document.removeEventListener('mouseover', previous.onOver, true);
    document.removeEventListener('mouseout', previous.onOut, true);
    document.removeEventListener('click', previous.onClick, true);
    if (previous.outlined && previous.outlined.el) previous.outlined.el.style.outline = previous.outlined.outline;
  }
  const outlined = { el: null, outline: '' };
  const onOver = (e) => {
    const t = e.target;
    if (!t || !t.style) return;
    if (outlined.el && outlined.el !== t) outlined.el.style.outline = outlined.outline;
    outlined.outline = t.style.outline;
    outlined.el = t;
    t.style.outline = '2px solid #4c8dff';
  };
  const onOut = (e) => {
    const t = e.target;
    if (outlined.el === t) {
      t.style.outline = outlined.outline;
      outlined.el = null;
    }
  };
  const onClick = (e) => {
    e.preventDefault();
    e.stopPropagation();
    const t = e.target;
    const r = t.getBoundingClientRect();
    window.__ferryxElementPick = JSON.stringify({
      outerHTML: t.outerHTML,
      css: getComputedStyle(t).cssText,
      devicePixelRatio: window.devicePixelRatio || 1,
      rect: { x: r.x, y: r.y, width: r.width, height: r.height }
    });
    if (typeof window.__ferryxRoute === 'function') window.__ferryxRoute('pick.ferryx.invalid', 'ready', '1');
  };
  document.addEventListener('mouseover', onOver, true);
  document.addEventListener('mouseout', onOut, true);
  document.addEventListener('click', onClick, true);
  window.__ferryxPicker = { installed: true, onOver, onOut, onClick, outlined };
})();"#;

pub const ELEMENT_PICKER_REMOVE_SCRIPT: &str = r#"(() => {
  const previous = window.__ferryxPicker;
  if (!previous || !previous.installed) return;
  document.removeEventListener('mouseover', previous.onOver, true);
  document.removeEventListener('mouseout', previous.onOut, true);
  document.removeEventListener('click', previous.onClick, true);
  if (previous.outlined && previous.outlined.el) previous.outlined.el.style.outline = previous.outlined.outline;
  window.__ferryxElementPick = null;
  window.__ferryxPicker = { installed: false };
})();"#;

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
    let x = (pick.rect.x * dpr).floor().max(0.0) as u32;
    let y = (pick.rect.y * dpr).floor().max(0.0) as u32;
    if x >= img.width() || y >= img.height() {
        return Err("element rect is outside the screenshot".to_string());
    }
    let cw = ((pick.rect.width * dpr).ceil() as u32).min(img.width() - x);
    let ch = ((pick.rect.height * dpr).ceil() as u32).min(img.height() - y);
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
    fn picker_script_can_be_removed_and_reports_device_pixels() {
        assert!(ELEMENT_PICKER_SCRIPT.contains("removeEventListener"));
        assert!(ELEMENT_PICKER_SCRIPT.contains("devicePixelRatio"));
        assert!(ELEMENT_PICKER_SCRIPT.contains("__ferryxRoute"));
        assert!(ELEMENT_PICKER_REMOVE_SCRIPT.contains("removeEventListener"));
        assert!(ELEMENT_PICKER_REMOVE_SCRIPT.contains("installed: false"));
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
}
