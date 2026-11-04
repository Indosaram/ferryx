use super::{Preview, ANSI};
use plist::{Dictionary, Value};

fn rgb(value: &Value) -> Option<String> {
    let d = value.as_dictionary()?;
    let component = |key| {
        let n = d.get(key)?.as_real()?;
        (n.is_finite() && (0.0..=1.0).contains(&n)).then(|| (n * 255.0).round() as u8)
    };
    Some(format!("#{:02x}{:02x}{:02x}", component("Red Component")?, component("Green Component")?, component("Blue Component")?))
}

pub fn parse(bytes: &[u8]) -> Result<Preview, String> {
    let root = Value::from_reader(std::io::Cursor::new(bytes)).map_err(|_| "plist")?;
    let root = root.as_dictionary().ok_or("dictionary")?;
    let profiles = root.get("New Bookmarks").and_then(Value::as_array).ok_or("profiles")?;
    let default = root.get("Default Bookmark Guid").and_then(Value::as_string);
    let profiles: Vec<&Dictionary> = profiles.iter().filter_map(Value::as_dictionary).collect();
    let profile = profiles.iter().copied().find(|p| default.is_some() && p.get("Guid").and_then(Value::as_string) == default)
        .or_else(|| profiles.first().copied()).ok_or("profile")?;
    let mut out = Preview::default();
    if profiles.len() > 1 { out.skipped.push("Other profiles (default profile selected)".into()); }
    for (key, value) in profile {
        let accepted = match key.as_str() {
            "Normal Font" => value.as_string().and_then(|font| font.rsplit_once(' ')).map(|(family, size)| {
                if out.settings.number("font-size", size) && !family.is_empty() {
                    out.settings.font_family = Some(family.into()); true
                } else { false }
            }).unwrap_or(false),
            "Scrollback Lines" => value.as_unsigned_integer().map(|n| out.settings.number("scrollback", &n.to_string())).unwrap_or(false),
            "Cursor Type" => {
                out.settings.cursor_style = match value.as_unsigned_integer() { Some(0) => Some("block".into()),
                    Some(1) => Some("bar".into()), Some(2) => Some("underline".into()), _ => None };
                out.settings.cursor_style.is_some()
            }
            "Background Color" | "Foreground Color" | "Cursor Color" | "Cursor Text Color" | "Selection Color" | "Selected Text Color" => {
                let mapped = match key.as_str() { "Background Color" => "background", "Foreground Color" => "foreground",
                    "Cursor Color" => "cursor", "Cursor Text Color" => "cursorAccent", "Selection Color" => "selectionBackground", _ => "selectionForeground" };
                rgb(value).map(|c| { out.settings.theme.insert(mapped.into(), c); }).is_some()
            }
            name if name.starts_with("Ansi ") && name.ends_with(" Color") => {
                name.strip_prefix("Ansi ").and_then(|n| n.strip_suffix(" Color")).and_then(|n| n.parse::<usize>().ok())
                    .and_then(|n| ANSI.get(n)).and_then(|name| rgb(value).map(|c| { out.settings.theme.insert((*name).into(), c); })).is_some()
            }
            _ => false,
        };
        if !accepted { out.skipped.push(key.clone()); }
    }
    Ok(out)
}
