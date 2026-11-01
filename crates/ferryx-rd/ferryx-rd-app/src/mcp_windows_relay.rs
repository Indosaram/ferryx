//! Early entry point: no runtime, tracing, connection, or media work in relays.
use std::io::{self, Read, Write};

pub fn run_if_requested() -> io::Result<bool> {
    match std::env::args().nth(1).as_deref() {
        Some("--maho-mcp-relay-input") => {
            match io::copy(&mut io::stdin().lock(), &mut io::stdout().lock()) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
                Err(error) => return Err(error),
            }
            Ok(true)
        }
        Some("--maho-mcp-relay-output") => {
            let mut input = io::stdin().lock();
            let mut output = io::stdout().lock();
            let mut receipt = io::stderr().lock();
            let mut bytes = [0; 8192];
            loop {
                let mut size = [0; 4];
                match input.read_exact(&mut size) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(true),
                    Err(error) => return Err(error),
                }
                let size = u32::from_le_bytes(size) as usize;
                if size > bytes.len() {
                    return Err(io::Error::other("oversized relay chunk"));
                }
                input.read_exact(&mut bytes[..size])?;
                output.write_all(&bytes[..size])?;
                output.flush()?;
                receipt.write_all(&[1])?;
                receipt.flush()?;
            }
        }
        _ => Ok(false),
    }
}
