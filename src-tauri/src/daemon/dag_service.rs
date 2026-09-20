use crate::dag::journal::{parse_run_checkpoint, DagRunSnapshot};
use std::path::Path;

pub use crate::dag::journal::resolve_dag_runs_dir;

pub fn scan_project_inventory(project_path: &Path) -> Vec<DagRunSnapshot> {
    let runs_dir = resolve_dag_runs_dir(project_path);
    if !runs_dir.is_dir() {
        return Vec::new();
    }
    let mut snapshots = Vec::new();
    let Ok(entries) = std::fs::read_dir(&runs_dir) else {
        return snapshots;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(snapshot) = parse_run_checkpoint(&content) {
                    snapshots.push(snapshot);
                }
            }
        }
    }
    snapshots.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    snapshots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_dag_runs_dir_variants() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();

        assert_eq!(
            resolve_dag_runs_dir(root),
            root.join(".omo/senpi-task/dag/runs")
        );

        let runs_dir = root.join("runs");
        std::fs::create_dir_all(&runs_dir).expect("create runs");
        assert_eq!(resolve_dag_runs_dir(root), runs_dir);

        let nested_runs = root.join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&nested_runs).expect("create nested runs");
        assert_eq!(resolve_dag_runs_dir(root), nested_runs);
    }

    #[test]
    fn test_scan_project_inventory_empty_on_missing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let inv = scan_project_inventory(temp.path());
        assert!(inv.is_empty());
    }
}
