use super::{color, Preview, ANSI};

pub fn parse(source: &str) -> Result<Preview, String> {
    let mut out = Preview::default();
    for line in source.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (key, value) = line.split_once('=').ok_or("invalid assignment")?;
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        let accepted = match key {
            "font-family" if !value.is_empty() => { out.settings.font_family = Some(value.into()); true }
            "font-size" => out.settings.number(key, value),
            // Ghostty scrollback-limit is bytes, not lines. Do not silently reinterpret it.
            "cursor-style" if ["bar", "block", "underline"].contains(&value) => {
                out.settings.cursor_style = Some(value.into()); true
            }
            "background" | "foreground" | "cursor-color" | "selection-background" | "selection-foreground" => {
                let name = match key { "cursor-color" => "cursor", "selection-background" => "selectionBackground",
                    "selection-foreground" => "selectionForeground", other => other };
                color(value).map(|v| { out.settings.theme.insert(name.into(), v); }).is_some()
            }
            "palette" => value.split_once('=').and_then(|(index, value)| {
                let name = ANSI.get(index.trim().parse::<usize>().ok()?)?;
                let value = color(value.trim())?;
                out.settings.theme.insert((*name).into(), value);
                Some(())
            }).is_some(),
            _ => false,
        };
        if !accepted { out.skipped.push(key.into()); }
    }
    Ok(out)
}
