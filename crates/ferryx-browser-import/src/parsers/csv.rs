use crate::{ImportError, ImportResult};

pub(crate) fn parse_csv_records(input: &str) -> ImportResult<Vec<Vec<String>>> {
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut record_has_data = false;

    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                _ => field.push(c),
            }
            continue;
        }

        match c {
            '"' => {
                in_quotes = true;
                record_has_data = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                record_has_data = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                record_has_data = false;
            }
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                record_has_data = false;
            }
            _ => {
                field.push(c);
                record_has_data = true;
            }
        }
    }

    if in_quotes {
        return Err(ImportError::Parse(
            "malformed CSV: unclosed quoted field".into(),
        ));
    }

    if record_has_data || !field.is_empty() {
        record.push(field);
        records.push(record);
    }

    Ok(records)
}
