mod support;

use std::fs;

use serde_json::Value;
use tempfile::tempdir;
use wsi_annotation_tests::run_core_profile;

#[test]
#[ignore = "requires the external annotation_probe built by scripts/check-core.sh"]
fn rust_core_profile_writes_matrix_and_nonprimary_qualification_results() {
    let (reference, probe) = support::real_adapters();
    let directory = tempdir().unwrap();

    let result = run_core_profile(&reference, &probe, directory.path(), "rust-core-test").unwrap();

    assert!(result.manifest.is_file());
    let manifest: Value = serde_json::from_slice(&fs::read(&result.manifest).unwrap()).unwrap();
    assert_eq!(
        manifest["metadata"]["reference"]["packages"]["highdicom"],
        "0.28.1"
    );
    assert_eq!(manifest["metadata"]["baseline_definition_version"], 1);
    assert!(
        manifest["metadata"]["reference"]["packages"]
            .as_object()
            .unwrap()
            .contains_key("pydcm")
    );
    let observations: Vec<Value> =
        fs::read_to_string(result.manifest.parent().unwrap().join("observations.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert_eq!(
        observations
            .iter()
            .filter(|item| item["phase"] == "matrix")
            .count(),
        9
    );
    assert_eq!(
        observations
            .iter()
            .filter(|item| item["phase"] == "conversion")
            .count(),
        6
    );
    let qualification = observations
        .iter()
        .find(|item| item["phase"] == "qualification")
        .unwrap();
    assert_eq!(qualification["primary_failure"], false);
    assert!(matches!(
        qualification["status"].as_str(),
        Some("qualified" | "unqualified")
    ));
    assert_eq!(
        result.ok,
        observations.iter().all(|item| {
            item["phase"] == "qualification" || item["status"].as_str() == Some("passed")
        })
    );
}
