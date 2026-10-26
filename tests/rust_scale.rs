mod support;

use std::path::PathBuf;
use std::time::Duration;

use tempfile::tempdir;
use wsi_annotation_tests::probe::ViewerProbe;
use wsi_annotation_tests::shim::ReferenceShim;
use wsi_annotation_tests::{ScaleCase, ScaleStatus, default_scale_cases, run_scale_cases};

#[test]
fn scale_profile_requires_one_million_and_profiles_five_million_coordinates() {
    let cases = default_scale_cases();

    assert!(
        cases
            .iter()
            .any(|case| case.coordinate_values == 1_000_000 && case.required)
    );
    assert!(
        cases
            .iter()
            .any(|case| case.coordinate_values == 5_000_000 && !case.required)
    );
}

#[test]
fn failed_scale_roundtrip_retains_inspection_and_failure_evidence() {
    let directory = tempdir().unwrap();
    let python = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".venv/bin/python");
    let reference = ReferenceShim::from_program(
        python.as_os_str(),
        vec![
            "-c".into(),
            r"import pathlib, sys
pathlib.Path(sys.argv[sys.argv.index('--output') + 1]).write_bytes(b'fixture')
print('{}')
"
            .into(),
        ],
        Duration::from_secs(5),
    )
    .unwrap();
    let script = directory.path().join("probe.py");
    let inspected: serde_json::Value =
        serde_json::from_str(include_str!("data/ann-report.json")).unwrap();
    std::fs::write(
        &script,
        format!(
            r"import json, sys
if sys.argv[1] == 'inspect':
    print({:?})
else:
    print(json.dumps({{'schema_version': 1, 'status': 'error', 'operation': 'roundtrip',
        'implementation': {{'name': 'test-probe', 'version': '1'}},
        'error': {{'code': 'WRITE_FAILED', 'message': 'roundtrip diagnostic'}}}}))
    print('retained stderr diagnostic', file=sys.stderr)
    sys.exit(7)
",
            inspected.to_string()
        ),
    )
    .unwrap();
    let probe = ViewerProbe::from_program(
        python,
        vec![script.into_os_string()],
        Some(Duration::from_secs(5)),
    )
    .unwrap();
    let rows = run_scale_cases(
        &reference,
        &probe,
        &directory.path().join("source.dcm"),
        &directory.path().join("scale"),
        &[ScaleCase {
            coordinate_values: 1000,
            required: false,
        }],
    )
    .unwrap();
    let row = serde_json::to_value(&rows[0]).unwrap();
    assert_eq!(row["status"], "failed");
    assert!(row["runtime_ms"].as_f64().is_some_and(|value| value > 0.0));
    assert_eq!(row["viewer_runtime"]["inspect"], inspected["runtime"]);
    assert_eq!(row["peak_tracked_heap_bytes"], 4096);
    assert_eq!(row["probe_commands"].as_object().unwrap().len(), 2);
    assert_eq!(row["execution_errors"]["roundtrip"]["returncode"], 7);
    assert!(
        row["execution_errors"]["roundtrip"]["stderr"]
            .as_str()
            .unwrap()
            .contains("retained stderr diagnostic")
    );
}

#[test]
#[ignore = "requires the external annotation_probe built by scripts/check-core.sh"]
fn rust_runs_a_scale_roundtrip_with_digest_payloads() {
    let (reference, probe) = support::real_adapters();
    let directory = tempdir().unwrap();
    let fixtures = reference
        .generate_core(&directory.path().join("fixtures"))
        .unwrap();
    let qualification = reference
        .qualify_pydcm(
            &fixtures.source,
            &fixtures.ann["2D_VOLUME"],
            &fixtures.seg["BINARY"],
        )
        .unwrap();
    assert_eq!(qualification.implementation, "pydcm");
    assert!(!qualification.primary_failure);

    let observations = run_scale_cases(
        &reference,
        &probe,
        &fixtures.source,
        &directory.path().join("scale"),
        &[ScaleCase {
            coordinate_values: 1_000,
            required: false,
        }],
    )
    .unwrap();

    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].status, ScaleStatus::Passed);
    assert_eq!(observations[0].coordinate_error_max_px, Some(0.0));
    assert!(observations[0].input_bytes.unwrap() > 0);
    assert!(observations[0].peak_rss_bytes.unwrap() > 0);
}
