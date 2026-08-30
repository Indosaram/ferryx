const EDGE_WRAPPER: &str = include_str!(
    "../../.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/evidence/windows-edges/run-edge-probes.ps1"
);

#[test]
fn edge_wrapper_stages_and_cleans_its_protocol_driver() {
    assert!(
        EDGE_WRAPPER.contains("$driverSource = Join-Path $PSScriptRoot \"probe-daemon-edges.mjs\""),
        "edge wrapper must resolve the committed driver beside the wrapper"
    );
    assert!(
        EDGE_WRAPPER.contains("Copy-Item $driverSource $driver -Force"),
        "edge wrapper must stage the driver at the repo-root runtime path"
    );
    assert!(
        EDGE_WRAPPER.contains("Remove-Item $driver -Force -ErrorAction SilentlyContinue"),
        "edge wrapper must remove the staged driver during cleanup"
    );
}
