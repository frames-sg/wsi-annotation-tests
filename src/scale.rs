use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::compare::compare_ann;
use crate::probe::{PayloadMode, ProbeError, ProbeMeasurements, ProbeObservation, ViewerProbe};
use crate::shim::ReferenceShim;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleCase {
    pub coordinate_values: usize,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScaleStatus {
    Passed,
    Failed,
}

impl ScaleCase {
    #[must_use]
    pub fn id(self) -> String {
        format!("ann-scale-{}", self.coordinate_values)
    }
}

#[derive(Debug, Serialize)]
pub struct ScaleObservation {
    pub case_id: String,
    pub coordinate_values: usize,
    pub required: bool,
    pub status: ScaleStatus,
    pub input_bytes: Option<u64>,
    pub output_bytes: Option<u64>,
    pub runtime_ms: Option<f64>,
    pub peak_rss_bytes: Option<u64>,
    pub viewer_runtime: BTreeMap<String, Value>,
    pub peak_tracked_heap_bytes: Option<u64>,
    pub viewer_implementation: Value,
    pub probe_commands: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub execution_errors: BTreeMap<String, ProbeError>,
    pub coordinate_error_max_px: Option<f64>,
    pub message: String,
}

#[must_use]
pub fn default_scale_cases() -> Vec<ScaleCase> {
    vec![
        ScaleCase {
            coordinate_values: 1_000,
            required: false,
        },
        ScaleCase {
            coordinate_values: 10_000,
            required: false,
        },
        ScaleCase {
            coordinate_values: 100_000,
            required: false,
        },
        ScaleCase {
            coordinate_values: 1_000_000,
            required: true,
        },
        ScaleCase {
            coordinate_values: 5_000_000,
            required: false,
        },
    ]
}

/// Generate and roundtrip large-coordinate ANN cases with digest payloads.
///
/// # Errors
///
/// Returns an error only when the scale output directory cannot be created.
/// Each workload failure is retained in its observation.
pub fn run_scale_cases(
    reference: &ReferenceShim,
    probe: &ViewerProbe,
    source: &Path,
    directory: &Path,
    cases: &[ScaleCase],
) -> Result<Vec<ScaleObservation>, String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create scale directory: {error}"))?;
    Ok(cases
        .iter()
        .copied()
        .map(|case| run_scale_case(reference, probe, source, directory, case))
        .collect())
}

fn run_scale_case(
    reference: &ReferenceShim,
    probe: &ViewerProbe,
    source: &Path,
    directory: &Path,
    case: ScaleCase,
) -> ScaleObservation {
    let case_id = case.id();
    let input = directory.join(format!("{case_id}-input.dcm"));
    let output = directory.join(format!("{case_id}-roundtrip.dcm"));
    let mut observation = failed(directory, case, String::new());
    let result = (|| -> Result<(), String> {
        reference
            .build_scale_ann(source, &input, case.coordinate_values)
            .map_err(|error| error.to_string())?;
        let inspected = probe
            .inspect(source, &input, None, PayloadMode::Digest)
            .map_err(|error| observation.record_error("inspect", error))?;
        observation.record_probe("inspect", &inspected);
        let roundtripped = probe
            .roundtrip(source, &input, &output, None, PayloadMode::Digest, false)
            .map_err(|error| observation.record_error("roundtrip", error))?;
        observation.record_probe("roundtrip", &roundtripped);
        let comparison = compare_ann(&inspected.report, &roundtripped.report, 1e-6, 1e-9)?;
        observation.status = if comparison.is_ok() {
            ScaleStatus::Passed
        } else {
            ScaleStatus::Failed
        };
        observation.coordinate_error_max_px = Some(comparison.coordinate_error.max);
        observation.message = comparison
            .findings
            .into_iter()
            .map(|finding| finding.message)
            .collect::<Vec<_>>()
            .join("; ");
        observation.input_bytes = Some(file_size(&input)?);
        observation.output_bytes = Some(file_size(&output)?);
        Ok(())
    })();
    if let Err(message) = result {
        observation.status = ScaleStatus::Failed;
        observation.message = message;
        observation.input_bytes = fs::metadata(&input).ok().map(|metadata| metadata.len());
        observation.output_bytes = fs::metadata(&output).ok().map(|metadata| metadata.len());
    }
    observation
}

impl ScaleObservation {
    fn record_measurements(&mut self, measurements: ProbeMeasurements) {
        if let Some(runtime) = measurements.runtime_ms {
            self.runtime_ms = Some(self.runtime_ms.unwrap_or(0.0) + runtime);
        }
        if let Some(rss) = measurements.peak_rss_bytes {
            self.peak_rss_bytes = Some(self.peak_rss_bytes.unwrap_or(0).max(rss));
        }
        if let Some(heap) = measurements.peak_tracked_heap_bytes {
            self.peak_tracked_heap_bytes =
                Some(self.peak_tracked_heap_bytes.unwrap_or(0).max(heap));
        }
    }

    fn record_probe(&mut self, phase: &str, probe: &ProbeObservation) {
        self.record_measurements(probe.measurements());
        self.viewer_runtime
            .insert(phase.to_owned(), probe.report["runtime"].clone());
        self.viewer_implementation = probe.report["implementation"].clone();
        self.probe_commands
            .insert(phase.to_owned(), probe.command.clone());
    }

    fn record_error(&mut self, phase: &str, error: ProbeError) -> String {
        let message = error.to_string();
        self.record_measurements(error.measurements());
        if let Some(command) = error.command.as_deref() {
            self.probe_commands
                .insert(phase.to_owned(), command.clone());
        }
        self.execution_errors.insert(phase.to_owned(), error);
        message
    }
}

fn file_size(path: &Path) -> Result<u64, String> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("could not stat {}: {error}", path.display()))
}

fn failed(directory: &Path, case: ScaleCase, message: String) -> ScaleObservation {
    let case_id = case.id();
    let input = directory.join(format!("{case_id}-input.dcm"));
    let output = directory.join(format!("{case_id}-roundtrip.dcm"));
    ScaleObservation {
        case_id,
        coordinate_values: case.coordinate_values,
        required: case.required,
        status: ScaleStatus::Failed,
        input_bytes: fs::metadata(input).ok().map(|metadata| metadata.len()),
        output_bytes: fs::metadata(output).ok().map(|metadata| metadata.len()),
        runtime_ms: None,
        peak_rss_bytes: None,
        viewer_runtime: BTreeMap::new(),
        peak_tracked_heap_bytes: None,
        viewer_implementation: Value::Object(serde_json::Map::new()),
        probe_commands: BTreeMap::new(),
        execution_errors: BTreeMap::new(),
        coordinate_error_max_px: None,
        message,
    }
}
