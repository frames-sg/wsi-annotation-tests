#![allow(dead_code)]

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use wsi_annotation_tests::probe::ViewerProbe;
use wsi_annotation_tests::shim::ReferenceShim;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn reference_python() -> PathBuf {
    let python = root().join(".venv/bin/python");
    assert!(python.is_file(), "run `uv sync --locked` first");
    python
}

pub fn probe_path() -> PathBuf {
    let probe = env::var_os("ANNOTATION_PROBE").map_or_else(
        || {
            env::var_os("WSI_DICOM_ANNOTATIONS_ROOT")
                .map_or_else(|| root().join("../wsi-dicom-annotations"), PathBuf::from)
                .join("target/debug/annotation_probe")
        },
        PathBuf::from,
    );
    assert!(
        probe.is_file(),
        "build annotation_probe or set ANNOTATION_PROBE"
    );
    probe
}

pub fn real_adapters() -> (ReferenceShim, ViewerProbe) {
    (
        ReferenceShim::from_program(
            reference_python(),
            vec![root().join("shim/reference_shim.py").into_os_string()],
            Duration::from_mins(10),
        )
        .unwrap(),
        ViewerProbe::from_program(probe_path(), Vec::new(), Some(Duration::from_mins(10))).unwrap(),
    )
}
