//! Fixed-size numeric records, serialized only at session teardown.
use serde::Serialize;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

pub(crate) const LIMIT: usize = 65_536;

#[derive(Clone, Copy, Default, Serialize)]
pub(crate) struct Record {
    pub at_us: u64,
    pub session: u64,
    pub event: u8,
    pub sequence: u32,
    pub kind: u8,
    pub size: usize,
    pub frame: u64,
    pub value: u64,
    pub keyframe: bool,
    pub repeat: bool,
}

#[derive(Default, Serialize)]
pub(crate) struct Records {
    pub records: Vec<Record>,
    pub overflow: u64,
}

impl Records {
    pub fn push(&mut self, record: Record) {
        if self.records.len() < LIMIT {
            self.records.push(record);
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }
}

pub(crate) struct Trace {
    origin: Instant,
    path: std::path::PathBuf,
    pub records: Mutex<Records>,
}

pub(crate) fn enabled() -> Option<&'static Arc<Trace>> {
    static TRACE: OnceLock<Option<Arc<Trace>>> = OnceLock::new();
    TRACE
        .get_or_init(|| {
            std::env::var_os("MAHO_HOST_TRACE_PATH").map(|path| {
                Arc::new(Trace {
                    origin: Instant::now(),
                    path: path.into(),
                    records: Mutex::new(Records {
                        records: Vec::with_capacity(LIMIT),
                        overflow: 0,
                    }),
                })
            })
        })
        .as_ref()
}

impl Trace {
    #[cfg(test)]
    pub fn for_test(path: std::path::PathBuf) -> Arc<Self> {
        Arc::new(Self {
            origin: Instant::now(),
            path,
            records: Mutex::new(Records::default()),
        })
    }
    pub fn record(&self, mut record: Record) {
        record.at_us = self.time(Instant::now());
        self.records
            .lock()
            .expect("host trace poisoned")
            .push(record);
    }

    pub fn time(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.origin).as_micros()).unwrap_or(u64::MAX)
    }

    pub fn dump(&self) -> Result<(), Box<dyn std::error::Error>> {
        let records = self.records.lock().map_err(|e| e.to_string())?;
        let file = std::fs::File::create(&self.path)?;
        use std::io::Write;
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer(&mut writer, &*records)?;
        writer.flush()?;
        Ok(())
    }
}
