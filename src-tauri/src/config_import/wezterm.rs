use super::{color, Preview, ANSI};
use mlua::{Lua, Table, Value, VmState};
use std::time::{Duration, Instant};

// Admit only literals and table constructors before Luau sees any input. In particular,
// field access, calls, functions, loops, operators and global lookups are not expressions.
struct Static<'a> { text: &'a [u8], pos: usize }
impl Static<'_> {
    fn space(&mut self) {
        loop {
            while self.text.get(self.pos).is_some_and(u8::is_ascii_whitespace) { self.pos += 1; }
            if self.text.get(self.pos..self.pos + 2) != Some(b"--") { break; }
            while self.text.get(self.pos).is_some_and(|b| *b != b'\n') { self.pos += 1; }
        }
    }
    fn eat(&mut self, byte: u8) -> bool {
        self.space();
        if self.text.get(self.pos) == Some(&byte) { self.pos += 1; true } else { false }
    }
    fn ident(&mut self) -> bool {
        self.space();
        if !self.text.get(self.pos).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_') { return false; }
        self.pos += 1;
        while self.text.get(self.pos).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') { self.pos += 1; }
        true
    }
    fn value(&mut self, depth: usize) -> bool {
        if depth > 16 { return false; }
        self.space();
        match self.text.get(self.pos).copied() {
            Some(b'{') => {
                self.pos += 1;
                while !self.eat(b'}') {
                    let saved = self.pos;
                    if self.ident() && self.eat(b'=') {
                        if !self.value(depth + 1) { return false; }
                    } else {
                        self.pos = saved;
                        if !self.value(depth + 1) { return false; }
                    }
                    if !self.eat(b',') && !self.eat(b';') { return self.eat(b'}'); }
                }
                true
            }
            Some(quote @ (b'\'' | b'"')) => {
                self.pos += 1;
                while let Some(&b) = self.text.get(self.pos) {
                    self.pos += 1;
                    if b == quote { return true; }
                    if b == b'\\' { self.pos += 1; }
                }
                false
            }
            Some(b'0'..=b'9' | b'-') => {
                let start = self.pos;
                self.pos += 1;
                while self.text.get(self.pos).is_some_and(|b| b.is_ascii_digit() || *b == b'.') { self.pos += 1; }
                std::str::from_utf8(&self.text[start..self.pos]).ok().and_then(|s| s.parse::<f64>().ok()).is_some()
            }
            _ => false,
        }
    }
}

pub fn parse(source: &str) -> Result<Preview, String> {
    let mut out = Preview::default();
    let source = source.trim();
    let source = source.strip_prefix("return").unwrap_or(source).trim();
    let mut syntax = Static { text: source.as_bytes(), pos: 0 };
    let valid = syntax.value(0);
    syntax.space();
    if !valid || syntax.pos != syntax.text.len() {
        out.skipped.push("dynamic WezTerm configuration".into());
        return Ok(out);
    }
    let read = || -> mlua::Result<()> {
        let lua = Lua::new();
        lua.set_memory_limit(4 * 1024 * 1024)?;
        lua.sandbox(true)?;
        let start = Instant::now();
        lua.set_interrupt(move |_| {
            if start.elapsed() >= Duration::from_millis(10) { return Err(mlua::Error::runtime("IMPORT_BUDGET")); }
            Ok(VmState::Continue)
        });
        // No globals, host callbacks, modules, filesystem, process, or user-script bindings.
        let table: Table = lua.load(format!("return {source}")).set_environment(lua.create_table()?).eval()?;
        for pair in table.pairs::<String, Value>() {
            let (key, value) = pair?;
            let accepted = match (key.as_str(), value) {
                ("font", Value::String(s)) => { out.settings.font_family = Some(s.to_str()?.to_string()); true }
                ("font_size", Value::Number(n)) => out.settings.number("font-size", &n.to_string()),
                ("font_size", Value::Integer(n)) => out.settings.number("font-size", &n.to_string()),
                ("scrollback_lines", Value::Integer(n)) => out.settings.number("scrollback", &n.to_string()),
                ("scrollback_lines", Value::Number(n)) => out.settings.number("scrollback", &n.to_string()),
                ("default_cursor_style", Value::String(s)) => {
                    let style = match s.to_str()?.as_ref() { "SteadyBar" | "BlinkingBar" => Some("bar"),
                        "SteadyBlock" | "BlinkingBlock" => Some("block"), "SteadyUnderline" | "BlinkingUnderline" => Some("underline"), _ => None };
                    out.settings.cursor_style = style.map(str::to_owned); style.is_some()
                }
                ("colors", Value::Table(colors)) => {
                    for pair in colors.pairs::<String, Value>() {
                        let (name, value) = pair?;
                        if let Value::String(s) = value.clone() {
                            let mapped = match name.as_str() { "cursor_bg" => "cursor", "cursor_fg" => "cursorAccent",
                                "selection_bg" => "selectionBackground", "selection_fg" => "selectionForeground", other => other };
                            if ["background", "foreground", "cursor", "cursorAccent", "selectionBackground", "selectionForeground"].contains(&mapped) {
                                if let Some(c) = color(s.to_str()?.as_ref()) { out.settings.theme.insert(mapped.into(), c); continue; }
                            }
                        }
                        if let Value::Table(values) = value {
                            if name == "ansi" || name == "brights" {
                                for (i, v) in values.sequence_values::<String>().take(8).enumerate() {
                                    if let Some(c) = color(&v?) { out.settings.theme.insert(ANSI[i + if name == "brights" { 8 } else { 0 }].into(), c); }
                                }
                                continue;
                            }
                        }
                        out.skipped.push(format!("colors.{name}"));
                    }
                    true
                }
                _ => false,
            };
            if !accepted { out.skipped.push(key); }
        }
        if start.elapsed() >= Duration::from_millis(10) { return Err(mlua::Error::runtime("IMPORT_BUDGET")); }
        Ok(())
    };
    let mut read = read;
    read().map_err(|_| "invalid static table".to_string())?;
    Ok(out)
}
