use std::path::{Path, PathBuf};

pub fn resolve_dag_runs_dir(project_path: &Path) -> PathBuf {
    let nested = project_path.join(".omo/senpi-task/dag");
    if nested.join("runs").is_dir() {
        nested.join("runs")
    } else if nested.is_dir() {
        nested
    } else if project_path.join("runs").is_dir() {
        project_path.join("runs")
    } else if project_path.ends_with(".omo/senpi-task/dag") || project_path.ends_with("dag") {
        project_path.join("runs")
    } else {
        nested.join("runs")
    }
}
