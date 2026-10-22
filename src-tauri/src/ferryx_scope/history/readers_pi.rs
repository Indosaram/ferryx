//! senpi-family JSONL transcripts: omo, pi, omp and gjc share one on-disk record shape.
//! Line 1 (omp: line 2, after its `title` slot) is `{"type":"session","version":N,"id","timestamp","cwd"}`
//! and message records are `{"type":"message","id","parentId","message":{"role","content":[...]}}`.
//! Evidence: `.omo-parity/facts/local-1.md:341-367` (omo/pi), `.omo-parity/facts/raw-omp/sample-session.jsonl`
//! (omp), `.omo-parity/facts/gjc-raw/sample-session.jsonl` (gjc).
use super::*;

pub(super) fn decode(provider: CanonicalProvider, cand: &Candidate, budget: usize) -> Result<Decoded> {
    let bytes = load(&cand.root, &cand.path, budget)?;
    let mut warnings = vec![];
    let mut identity: Option<(String, String)> = None;
    let mut version = None;
    let mut messages = vec![];
    for (ordinal, v) in records(&bytes, &mut warnings) {
        match v["type"].as_str().unwrap_or("") {
            "session" => {
                let (Some(id), Some(cwd)) = (field(&v, "id"), field(&v, "cwd")) else { return Err(HistoryError::InvalidIdentity) };
                let (id, cwd) = (checked_id(&id)?, checked_cwd(&cwd)?);
                if identity.as_ref().is_some_and(|old| old != &(id.clone(), cwd.clone())) { return Err(HistoryError::InvalidIdentity); }
                identity = Some((id, cwd));
            }
            "message" => {
                // omp stamps `runtime.version` on every record; omo, pi and gjc do not emit it.
                version = field(&v["runtime"], "version").or(version);
                let message = &v["message"];
                let text = content_text(&message["content"]);
                if !text.is_empty() {
                    messages.push(Message { ordinal, role: field(message, "role").unwrap_or_default(), text, id: field(&v, "id"), parent_id: field(&v, "parentId") });
                }
            }
            _ => {}
        }
    }
    let (id, cwd) = identity.ok_or(HistoryError::InvalidIdentity)?;
    // `pi --session` takes a transcript path and `omp --resume` prefers one; omo and gjc resume by id.
    let transcript_path = matches!(provider, CanonicalProvider::Pi | CanonicalProvider::Omp).then(|| cand.path.to_string_lossy().into_owned());
    // Forking is unmeasured for this family, so every message record is reported in file order
    // rather than guessing an active branch from `parentId`.
    Ok(Decoded { used: bytes.len(), hash: digest(&bytes), entry: make_entry(provider, id, cwd, version, None, transcript_path), messages, warnings })
}
