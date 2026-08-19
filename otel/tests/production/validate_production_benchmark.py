#!/usr/bin/env python3
"""Validate production-shaped OpenTelemetry benchmark evidence."""

from __future__ import annotations

import json
import statistics
import sys
from pathlib import Path
from typing import Any

REQUIRED_WORKLOADS = (
    "http_read",
    "http_write",
    "database",
    "outbound_http",
    "command",
    "messenger",
    "mixed",
)
PROFILES = ("baseline", "loaded_disabled", "parentbased_1pct", "always_on")
THRESHOLDS = {
    "loaded_disabled": {
        "p95_ms": 2.0,
        "p99_ms": 3.0,
        "throughput_per_second": 2.0,
        "cpu_ms_per_operation": 2.0,
    },
    "parentbased_1pct": {
        "p95_ms": 3.0,
        "p99_ms": 5.0,
        "throughput_per_second": 3.0,
        "cpu_ms_per_operation": 5.0,
    },
    "always_on": {
        "p95_ms": 8.0,
        "p99_ms": 10.0,
        "cpu_ms_per_operation": 10.0,
    },
}
RUN_METRICS = (
    "p95_ms",
    "p99_ms",
    "throughput_per_second",
    "cpu_ms_per_operation",
    "peak_rss_mib",
)
FAULTS = ("healthy", "slow", "unreachable", "rejecting")


def _median(values: list[float]) -> float:
    return float(statistics.median(values))


def _positive_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and value > 0


def _regression_percent(baseline: float, current: float, lower_is_better: bool) -> float:
    if lower_is_better:
        return ((current / baseline) - 1.0) * 100.0
    return (1.0 - (current / baseline)) * 100.0


def _runs(
    report: dict[str, Any],
    profile: str,
    workload: str,
    repetitions: int,
    errors: list[str],
) -> list[dict[str, Any]]:
    profiles = report.get("profiles")
    profile_data = profiles.get(profile) if isinstance(profiles, dict) else None
    workloads = profile_data.get("workloads") if isinstance(profile_data, dict) else None
    value = workloads.get(workload) if isinstance(workloads, dict) else None
    if not isinstance(value, list) or len(value) != repetitions:
        errors.append(f"{profile}.{workload}: expected exactly {repetitions} raw runs")
        return []

    pair_ids: list[int] = []
    valid = True
    for index, run in enumerate(value):
        prefix = f"{profile}.{workload}.run[{index}]"
        if not isinstance(run, dict):
            errors.append(f"{prefix}: must be an object")
            valid = False
            continue
        if not isinstance(run.get("pair_id"), int):
            errors.append(f"{prefix}.pair_id: must be an integer")
            valid = False
        else:
            pair_ids.append(run["pair_id"])
        if not isinstance(run.get("execution_order"), int) or not 1 <= run["execution_order"] <= len(PROFILES):
            errors.append(f"{prefix}.execution_order: must be between 1 and {len(PROFILES)}")
            valid = False
        for metric in RUN_METRICS:
            if not _positive_number(run.get(metric)):
                errors.append(f"{prefix}.{metric}: must be positive")
                valid = False
        if not isinstance(run.get("application_errors"), int) or run["application_errors"] < 0:
            errors.append(f"{prefix}.application_errors: must be a non-negative integer")
            valid = False
        if not isinstance(run.get("output_digest"), str) or not run["output_digest"]:
            errors.append(f"{prefix}.output_digest: must be non-empty")
            valid = False

    if sorted(pair_ids) != list(range(1, repetitions + 1)):
        errors.append(f"{profile}.{workload}: pair_id values must be 1..{repetitions}")
        valid = False
    return value if valid else []


def validate_report(report: dict[str, Any]) -> tuple[dict[str, Any], list[str]]:
    errors: list[str] = []
    environment = report.get("environment")
    if report.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if not isinstance(environment, dict):
        return {}, errors + ["environment must be an object"]

    for field in (
        "infrastructure_id",
        "source_revision",
        "dataset_id",
        "php_version",
        "instance_type",
        "collector_placement",
    ):
        if not isinstance(environment.get(field), str) or not environment[field]:
            errors.append(f"environment.{field} must be non-empty")
    for field in ("concurrency", "warmup_seconds", "measurement_seconds", "repetitions"):
        if not isinstance(environment.get(field), int) or environment[field] <= 0:
            errors.append(f"environment.{field} must be a positive integer")
    if environment.get("warmup_seconds", 0) < 60:
        errors.append("environment.warmup_seconds must be at least 60")
    if environment.get("measurement_seconds", 0) < 180:
        errors.append("environment.measurement_seconds must be at least 180")
    if environment.get("repetitions", 0) < 5:
        errors.append("environment.repetitions must be at least 5")
    if environment.get("randomized_order") is not True:
        errors.append("environment.randomized_order must be true")

    images = environment.get("image_digests")
    if not isinstance(images, dict):
        errors.append("environment.image_digests must be an object")
    else:
        for profile in PROFILES:
            if not isinstance(images.get(profile), str) or not images[profile].startswith("sha256:"):
                errors.append(f"environment.image_digests.{profile} must be a sha256 digest")

    repetitions = environment.get("repetitions") if isinstance(environment.get("repetitions"), int) else 0
    if repetitions <= 0:
        return {}, errors

    profile_runs: dict[str, dict[str, list[dict[str, Any]]]] = {}
    for profile in PROFILES:
        profile_runs[profile] = {}
        for workload in REQUIRED_WORKLOADS:
            profile_runs[profile][workload] = _runs(report, profile, workload, repetitions, errors)

    summary: dict[str, Any] = {"workloads": {}, "collector_faults": {}}
    for workload in REQUIRED_WORKLOADS:
        if any(not profile_runs[profile][workload] for profile in PROFILES):
            continue
        baseline_runs = profile_runs["baseline"][workload]
        baseline_medians = {metric: _median([float(run[metric]) for run in baseline_runs]) for metric in RUN_METRICS}
        summary["workloads"][workload] = {"baseline": baseline_medians}

        baseline_by_pair = {run["pair_id"]: run for run in baseline_runs}
        for profile in PROFILES[1:]:
            current_runs = profile_runs[profile][workload]
            medians = {metric: _median([float(run[metric]) for run in current_runs]) for metric in RUN_METRICS}
            regressions: dict[str, float] = {}
            for metric, limit in THRESHOLDS[profile].items():
                regression = _regression_percent(
                    baseline_medians[metric],
                    medians[metric],
                    lower_is_better=metric != "throughput_per_second",
                )
                regressions[metric] = round(regression, 4)
                if regression > limit:
                    errors.append(
                        f"{profile}.{workload}.{metric}: regression {regression:.3f}% exceeds {limit:.1f}%",
                    )
            if medians["peak_rss_mib"] - baseline_medians["peak_rss_mib"] > 32.0:
                errors.append(f"{profile}.{workload}.peak_rss_mib: adds more than 32 MiB")
            for run in current_runs:
                baseline = baseline_by_pair.get(run["pair_id"])
                if baseline is None:
                    continue
                if run["application_errors"] != baseline["application_errors"]:
                    errors.append(f"{profile}.{workload}.pair[{run['pair_id']}]: application errors changed")
                if run["output_digest"] != baseline["output_digest"]:
                    errors.append(f"{profile}.{workload}.pair[{run['pair_id']}]: application output changed")
            summary["workloads"][workload][profile] = {
                "medians": medians,
                "regressions_percent": regressions,
            }

        for pair_id in range(1, repetitions + 1):
            orders = [
                next(
                    (run["execution_order"] for run in profile_runs[profile][workload] if run["pair_id"] == pair_id),
                    None,
                )
                for profile in PROFILES
            ]
            if sorted(order for order in orders if isinstance(order, int)) != list(range(1, len(PROFILES) + 1)):
                errors.append(f"{workload}.pair[{pair_id}]: execution_order must be unique across profiles")

    faults = report.get("collector_faults")
    if not isinstance(faults, dict):
        errors.append("collector_faults must be an object")
    else:
        fault_runs: dict[str, list[dict[str, Any]]] = {}
        for fault in FAULTS:
            runs = faults.get(fault)
            if not isinstance(runs, list) or len(runs) != repetitions:
                errors.append(f"collector_faults.{fault}: expected exactly {repetitions} runs")
                continue
            valid = True
            pair_ids = []
            for index, run in enumerate(runs):
                if not isinstance(run, dict):
                    errors.append(f"collector_faults.{fault}[{index}]: must be an object")
                    valid = False
                    continue
                for metric in ("p95_ms", "p99_ms"):
                    if not _positive_number(run.get(metric)):
                        errors.append(f"collector_faults.{fault}[{index}].{metric}: must be positive")
                        valid = False
                if not isinstance(run.get("application_errors"), int):
                    errors.append(f"collector_faults.{fault}[{index}].application_errors: must be an integer")
                    valid = False
                if not isinstance(run.get("pair_id"), int):
                    errors.append(f"collector_faults.{fault}[{index}].pair_id: must be an integer")
                    valid = False
                else:
                    pair_ids.append(run["pair_id"])
            if sorted(pair_ids) != list(range(1, repetitions + 1)):
                errors.append(f"collector_faults.{fault}: pair_id values must be 1..{repetitions}")
                valid = False
            if valid:
                fault_runs[fault] = runs
        if all(fault in fault_runs for fault in FAULTS):
            healthy_p95 = _median([float(run["p95_ms"]) for run in fault_runs["healthy"]])
            healthy_p99 = _median([float(run["p99_ms"]) for run in fault_runs["healthy"]])
            healthy_errors = sum(int(run["application_errors"]) for run in fault_runs["healthy"])
            for fault in FAULTS[1:]:
                p95 = _median([float(run["p95_ms"]) for run in fault_runs[fault]])
                p99 = _median([float(run["p99_ms"]) for run in fault_runs[fault]])
                p95_delta = p95 - healthy_p95
                p99_delta = p99 - healthy_p99
                if p95_delta > max(healthy_p95 * 0.01, 2.0):
                    errors.append(f"collector_faults.{fault}.p95_ms: isolation limit exceeded")
                if p99_delta > max(healthy_p99 * 0.02, 5.0):
                    errors.append(f"collector_faults.{fault}.p99_ms: isolation limit exceeded")
                if sum(int(run["application_errors"]) for run in fault_runs[fault]) != healthy_errors:
                    errors.append(f"collector_faults.{fault}: application errors changed")
                summary["collector_faults"][fault] = {"p95_delta_ms": p95_delta, "p99_delta_ms": p99_delta}

    longevity = report.get("messenger_longevity")
    if not isinstance(longevity, dict):
        errors.append("messenger_longevity must be an object")
    else:
        if not isinstance(longevity.get("measured_spans"), int) or longevity["measured_spans"] < 100_000:
            errors.append("messenger_longevity.measured_spans must be at least 100000")
        for field in ("rss_before_mib", "rss_after_mib"):
            if not _positive_number(longevity.get(field)):
                errors.append(f"messenger_longevity.{field} must be positive")
        if _positive_number(longevity.get("rss_before_mib")) and _positive_number(longevity.get("rss_after_mib")):
            if longevity["rss_after_mib"] - longevity["rss_before_mib"] > 5.0:
                errors.append("messenger_longevity RSS growth exceeds 5 MiB")
        if longevity.get("threads_before") != longevity.get("threads_after"):
            errors.append("messenger_longevity thread count changed")
        for field in ("threads_before", "threads_after", "active_contexts_end", "active_scopes_end"):
            if not isinstance(longevity.get(field), int) or longevity[field] < 0:
                errors.append(f"messenger_longevity.{field} must be a non-negative integer")
        if longevity.get("active_contexts_end") != 0 or longevity.get("active_scopes_end") != 0:
            errors.append("messenger_longevity ended with active context/scope state")

    accounting = report.get("export_accounting")
    if not isinstance(accounting, dict):
        errors.append("export_accounting must be an object")
    else:
        for profile in ("parentbased_1pct", "always_on"):
            values = accounting.get(profile)
            if not isinstance(values, dict):
                errors.append(f"export_accounting.{profile} must be an object")
                continue
            counter_fields = (
                "sampled_ended",
                "exported",
                "dropped_queue_full",
                "dropped_shutdown_timeout",
                "dropped_export_failure",
                "queue_depth_end",
            )
            if any(
                not isinstance(values.get(field), int) or isinstance(values.get(field), bool) or values[field] < 0
                for field in counter_fields
            ):
                errors.append(f"export_accounting.{profile}: all counters must be non-negative integers")
                continue
            accounted = sum(
                values[field]
                for field in ("exported", "dropped_queue_full", "dropped_shutdown_timeout", "dropped_export_failure")
            )
            if accounted != values.get("sampled_ended"):
                errors.append(f"export_accounting.{profile}: sampled spans are not exactly accounted")
            if values.get("queue_depth_end") != 0:
                errors.append(f"export_accounting.{profile}: queue did not drain")

    if report.get("disabled_no_network") is not True:
        errors.append("disabled_no_network must be true")
    if report.get("application_thread_collector_io") is not False:
        errors.append("application_thread_collector_io must be false")
    artifacts = report.get("artifacts")
    if not isinstance(artifacts, dict):
        errors.append("artifacts must be an object")
    else:
        for field in ("raw_results", "resource_samples", "syscall_audit", "application_logs"):
            if not isinstance(artifacts.get(field), str) or not artifacts[field]:
                errors.append(f"artifacts.{field} must be non-empty")

    return summary, errors


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {Path(sys.argv[0]).name} REPORT.json", file=sys.stderr)
        return 2
    try:
        report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print(f"unable to read benchmark report: {error}", file=sys.stderr)
        return 2
    if not isinstance(report, dict):
        print("benchmark report root must be an object", file=sys.stderr)
        return 2

    summary, errors = validate_report(report)
    print(json.dumps({"passed": not errors, "summary": summary, "errors": errors}, indent=2, sort_keys=True))
    return 0 if not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
