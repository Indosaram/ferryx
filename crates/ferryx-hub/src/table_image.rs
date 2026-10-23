//! Markdown tables become the same SVG omo draws, then a PNG via usvg and resvg.
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTable {
    pub original_text: String,
    pub image_bytes: Vec<u8>,
    pub filename: String,
}

const MAX_TABLE_ROWS: usize = 300;

fn split_table_cells(inner: &str) -> Vec<String> {
    let code_aware = inner.matches('`').count().is_multiple_of(2);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut in_code = false;
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if chars.peek() == Some(&'|') => {
                current.push('|');
                chars.next();
            }
            '`' if code_aware => {
                in_code = !in_code;
                current.push(ch);
            }
            '|' if !in_code => cells.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    cells.push(current);
    cells
}

fn parse_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = match inner.strip_suffix('|') {
        Some(stripped) if !stripped.ends_with('\\') => stripped,
        _ => inner,
    };
    split_table_cells(inner)
        .into_iter()
        .map(|col| col.trim().replace("<br>", "\n").replace("<br/>", "\n"))
        .collect()
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return false;
    }
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').all(|col| {
        let col = col.trim();
        !col.is_empty() && col.chars().all(|c| c == '-' || c == ':' || c == ' ')
    })
}

pub fn extract_markdown_tables(text: &str) -> Vec<MarkdownTable> {
    extract_markdown_table_spans(text)
        .into_iter()
        .map(|(_, table)| table)
        .collect()
}

fn extract_markdown_table_spans(text: &str) -> Vec<(std::ops::Range<usize>, MarkdownTable)> {
    let mut tables = Vec::new();
    let mut offset = 0;
    let mut starts = Vec::new();
    let lines: Vec<&str> = text
        .split_inclusive('\n')
        .map(|line| {
            starts.push(offset);
            offset += line.len();
            line.trim_end_matches(['\r', '\n'])
        })
        .collect();
    let mut index = 0;
    let mut fence: Option<(char, usize)> = None;
    while index < lines.len() {
        let line = lines[index].trim();
        let marker = line.chars().next().unwrap_or(' ');
        let marker_len = line.chars().take_while(|&ch| ch == marker).count();
        if let Some((open, length)) = fence {
            if marker == open && marker_len >= length && line[marker_len..].trim().is_empty() {
                fence = None;
            }
            index += 1;
            continue;
        }
        if matches!(marker, '`' | '~') && marker_len >= 3 {
            fence = Some((marker, marker_len));
            index += 1;
            continue;
        }
        if line.starts_with('|') && line.ends_with('|') && index + 1 < lines.len() {
            let next_line = lines[index + 1].trim();
            if is_table_separator(next_line) {
                let headers = parse_table_row(line);
                let mut rows = Vec::new();
                let mut row = index + 2;
                while row < lines.len() {
                    let row_line = lines[row].trim();
                    if row_line.starts_with('|') && row_line.ends_with('|') {
                        rows.push(parse_table_row(row_line));
                        row += 1;
                    } else {
                        break;
                    }
                }
                if !rows.is_empty() {
                    let span = starts[index]..starts[row - 1] + lines[row - 1].len();
                    tables.push((
                        span.clone(),
                        MarkdownTable {
                            headers,
                            rows,
                            raw: text[span].to_string(),
                        },
                    ));
                    index = row;
                    continue;
                }
            }
        }
        index += 1;
    }
    tables
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn render_table_to_svg(table: &MarkdownTable) -> String {
    let font_family = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Apple SD Gothic Neo', 'Noto Sans KR', sans-serif";
    let num_cols = table.headers.len().max(1);
    let cell_padding_x = 16.0;
    let cell_padding_y = 12.0;
    let line_height = 20.0;
    let font_size = 14.0;
    let mut max_chars = vec![1usize; num_cols];
    for (col, header) in table.headers.iter().enumerate().take(num_cols) {
        let len = header
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        max_chars[col] = max_chars[col].max(len.max(1));
    }
    for row in &table.rows {
        for (col, cell) in row.iter().enumerate().take(num_cols) {
            let len = cell
                .lines()
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0);
            max_chars[col] = max_chars[col].max(len);
        }
    }
    let total: usize = max_chars.iter().sum::<usize>().max(1);
    let col_widths: Vec<f64> = max_chars
        .iter()
        .map(|weight| ((*weight as f64) / (total as f64) * 860.0).clamp(160.0, 480.0))
        .collect();
    let width: f64 = col_widths.iter().sum();
    let wrap = |text: &str, col_width: f64| -> Vec<String> {
        let max_line = ((col_width - cell_padding_x * 2.0) / font_size).max(1.0) as usize;
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let paragraph = paragraph.trim();
            if paragraph.is_empty() {
                continue;
            }
            let mut current = String::new();
            for word in paragraph.split_whitespace() {
                if current.chars().count() + word.chars().count() + 1 > max_line
                    && !current.is_empty()
                {
                    lines.push(std::mem::take(&mut current));
                }
                if !current.is_empty() {
                    current.push(' ');
                }
                for ch in word.chars() {
                    if current.chars().count() == max_line {
                        lines.push(std::mem::take(&mut current));
                    }
                    current.push(ch);
                }
            }
            if !current.is_empty() {
                lines.push(current);
            }
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    };
    let header_lines: Vec<Vec<String>> = table
        .headers
        .iter()
        .enumerate()
        .map(|(col, header)| wrap(header, col_widths[col]))
        .collect();
    let header_height = (header_lines
        .iter()
        .map(|lines| lines.len())
        .max()
        .unwrap_or(1) as f64
        * line_height)
        + cell_padding_y * 2.0;
    let mut row_data = Vec::new();
    let mut height = header_height + 1.0;
    for row in &table.rows {
        let cells: Vec<Vec<String>> = (0..num_cols)
            .map(|col| {
                wrap(
                    row.get(col).map(String::as_str).unwrap_or(""),
                    col_widths[col],
                )
            })
            .collect();
        let row_height = (cells.iter().map(|lines| lines.len()).max().unwrap_or(1) as f64
            * line_height)
            + cell_padding_y * 2.0;
        height += row_height;
        row_data.push((cells, row_height));
    }
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"0 0 {width:.0} {height:.0}\">\n"
    );
    svg.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#2b2d31\" rx=\"8\" ry=\"8\"/>\n");
    let mut x = 0.0;
    for (col, col_width) in col_widths.iter().enumerate() {
        svg.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"0\" width=\"{col_width:.1}\" height=\"{header_height:.1}\" fill=\"#1e1f22\"/>\n"
        ));
        let mut y = cell_padding_y + 14.0;
        for line in &header_lines[col] {
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{y:.1}\" font-family=\"{font_family}\" font-size=\"{font_size:.1}\" font-weight=\"bold\" fill=\"#f2f3f5\">{}</text>\n",
                x + cell_padding_x,
                html_escape(line)
            ));
            y += line_height;
        }
        x += col_width;
    }
    let mut y = header_height;
    for (row_index, (cells, row_height)) in row_data.iter().enumerate() {
        let bg = if row_index % 2 == 0 {
            "#313338"
        } else {
            "#2b2d31"
        };
        let mut cell_x = 0.0;
        for (col, col_width) in col_widths.iter().enumerate() {
            svg.push_str(&format!(
                "<rect x=\"{cell_x:.1}\" y=\"{y:.1}\" width=\"{col_width:.1}\" height=\"{row_height:.1}\" fill=\"{bg}\"/>\n"
            ));
            let mut text_y = y + cell_padding_y + 14.0;
            for line in &cells[col] {
                svg.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{text_y:.1}\" font-family=\"{font_family}\" font-size=\"{font_size:.1}\" fill=\"#dbdee1\">{}</text>\n",
                    cell_x + cell_padding_x,
                    html_escape(line)
                ));
                text_y += line_height;
            }
            cell_x += col_width;
        }
        y += row_height;
    }
    svg.push_str("</svg>");
    svg
}

const MAX_TABLE_PIXELS: u64 = 32_000_000;

fn system_font_db() -> std::sync::Arc<usvg::fontdb::Database> {
    static FONT_DB: std::sync::OnceLock<std::sync::Arc<usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    FONT_DB
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            std::sync::Arc::new(db)
        })
        .clone()
}

/// Rasterizes the table SVG at the same scale omo uses.
pub fn svg_to_png(svg_str: &str, scale: f32) -> Result<Vec<u8>, String> {
    let opt = usvg::Options {
        font_family: "sans-serif".to_owned(),
        fontdb: system_font_db(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg_str, &opt).map_err(|error| error.to_string())?;
    let pixmap_size = tree
        .size()
        .to_int_size()
        .scale_by(scale)
        .ok_or_else(|| "invalid scale size".to_owned())?;
    if u64::from(pixmap_size.width()) * u64::from(pixmap_size.height()) > MAX_TABLE_PIXELS {
        return Err("table image exceeds pixel budget".into());
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .ok_or_else(|| "failed to allocate pixmap".to_owned())?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|error| error.to_string())
}

pub fn render_table_to_png(table: &MarkdownTable) -> Result<Vec<u8>, String> {
    svg_to_png(&render_table_to_svg(table), 2.0)
}

/// Plain-text rendering of tables whose image upload failed, so the table
/// content is never lost: each original table in a fenced code block.
pub fn fallback_text(tables: &[RenderedTable]) -> String {
    tables
        .iter()
        .map(|table| {
            let longest_run = table
                .original_text
                .split(|ch| ch != '`')
                .map(str::len)
                .max()
                .unwrap_or(0);
            let fence = "`".repeat((longest_run + 1).max(3));
            format!("{fence}\n{}\n{fence}", table.original_text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replaces each renderable table with a placeholder and returns the PNG attachment.
pub fn transform_markdown_tables(content: &str) -> (String, Vec<RenderedTable>) {
    let tables = extract_markdown_table_spans(content);
    if tables.is_empty() {
        return (content.to_owned(), Vec::new());
    }
    let mut rendered = Vec::new();
    let mut modified = String::new();
    let mut cursor = 0;
    for (index, (span, table)) in tables.into_iter().enumerate() {
        if table.rows.len() > MAX_TABLE_ROWS
            || table.rows.iter().any(|row| row.len() > table.headers.len())
        {
            continue;
        }
        let png = match render_table_to_png(&table) {
            Ok(png) => png,
            Err(error) => {
                tracing::warn!(%error, "failed to rasterize table SVG to PNG; keeping raw text");
                continue;
            }
        };
        let filename = format!("table_{}.png", index + 1);
        let placeholder = format!("\n*(📊 아래 첨부된 표 [{filename}] 참조)*\n");
        modified.push_str(&content[cursor..span.start]);
        modified.push_str(&placeholder);
        cursor = span.end;
        rendered.push(RenderedTable {
            original_text: table.raw,
            image_bytes: png,
            filename,
        });
    }
    modified.push_str(&content[cursor..]);
    (modified, rendered)
}

pub fn tables_in_message(content: &str) -> Value {
    let (text, images) = transform_markdown_tables(content);
    serde_json::json!({
        "text": text,
        "images": images.into_iter().map(|image| serde_json::json!({
            "filename": image.filename,
            "bytes": image.image_bytes.len(),
        })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_pipes_stay_in_one_cell_and_the_svg_carries_the_text() {
        let raw = "| Cmd | Note |\n|---|---|\n| `cat f | wc -l` | a \\| b |";
        let tables = extract_markdown_tables(raw);
        assert_eq!(tables[0].rows[0], vec!["`cat f | wc -l`", "a | b"]);
        let (text, images) = transform_markdown_tables(raw);
        assert_eq!(images.len(), 1);
        assert!(text.contains("table_1.png"));
        assert!(images[0].image_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let svg = render_table_to_svg(&tables[0]);
        assert!(svg.contains("<svg"));
        assert!(svg.contains("wc -l"));
        let fenced = fallback_text(&[RenderedTable {
            original_text: "| A |\n|---|\n| ``` |".into(),
            image_bytes: Vec::new(),
            filename: "table_1.png".into(),
        }]);
        assert!(fenced.starts_with("````\n| A |"));
        assert!(fenced.ends_with("| ``` |\n````"));
        let malformed = "| A |\n|---|\n| x | y |";
        let (kept, none) = transform_markdown_tables(malformed);
        assert_eq!(kept, malformed);
        assert!(none.is_empty());
    }
}
