from __future__ import annotations

import copy
import unittest

from validate_production_benchmark import PROFILES, REQUIRED_WORKLOADS, validate_report


def build_report() -> dict:
    metrics = {
        "baseline": (100.0, 150.0, 1000.0, 5.0, 100.0),
        "loaded_disabled": (101.5, 154.0, 985.0, 5.05, 105.0),
        "parentbased_1pct": (102.5, 157.0, 975.0, 5.2, 110.0),
        "always_on": (107.0, 164.0, 950.0, 5.45, 120.0),
    }
    profiles = {}
    for profile_index, profile in enumerate(PROFILES):
        p95, p99, throughput, cpu, rss = metrics[profile]
        profiles[profile] = {"workloads": {}}
        for workload in REQUIRED_WORKLOADS:
            profiles[profile]["workloads"][workload] = [
                {
                    "pair_id": pair_id,
                    "execution_order": ((profile_index + pair_id - 1) % len(PROFILES)) + 1,
                    "p95_ms": p95,
                    "p99_ms": p99,
                    "throughput_per_second": throughput,
                    "cpu_ms_per_operation": cpu,
                    "peak_rss_mib": rss,
                    "application_errors": 0,
                    "output_digest": f"{workload}-output-{pair_id}",
                }
                for pair_id in range(1, 6)
            ]

    fault_metrics = {
        "healthy": (100.0, 150.0),
        "slow": (101.5, 154.0),
        "unreachable": (101.0, 153.0),
        "rejecting": (100.5, 152.0),
    }
    return {
        "schema_version": 1,
        "environment": {
            "infrastructure_id": "fixed-runner-1",
            "source_revision": "0123456789abcdef",
            "dataset_id": "replay-safe-seed-v1",
            "php_version": "8.2.33",
            "instance_type": "fixed-4vcpu-8gib",
            "collector_placement": "same-host-container",
            "concurrency": 16,
            "warmup_seconds": 60,
            "measurement_seconds": 180,
            "repetitions": 5,
            "randomized_order": True,
            "image_digests": {profile: f"sha256:{profile}" for profile in PROFILES},
        },
        "profiles": profiles,
        "collector_faults": {
            fault: [
                {"pair_id": pair_id, "p95_ms": values[0], "p99_ms": values[1], "application_errors": 0}
                for pair_id in range(1, 6)
            ]
            for fault, values in fault_metrics.items()
        },
        "messenger_longevity": {
            "measured_spans": 100_000,
            "rss_before_mib": 100.0,
            "rss_after_mib": 104.0,
            "threads_before": 3,
            "threads_after": 3,
            "active_contexts_end": 0,
            "active_scopes_end": 0,
        },
        "export_accounting": {
            "parentbased_1pct": {
                "sampled_ended": 100,
                "exported": 99,
                "dropped_queue_full": 1,
                "dropped_shutdown_timeout": 0,
                "dropped_export_failure": 0,
                "queue_depth_end": 0,
            },
            "always_on": {
                "sampled_ended": 1000,
                "exported": 990,
                "dropped_queue_full": 5,
                "dropped_shutdown_timeout": 3,
                "dropped_export_failure": 2,
                "queue_depth_end": 0,
            },
        },
        "disabled_no_network": True,
        "application_thread_collector_io": False,
        "artifacts": {
            "raw_results": "artifact://raw.json",
            "resource_samples": "artifact://resources.json",
            "syscall_audit": "artifact://syscalls.txt",
            "application_logs": "artifact://application.log",
        },
    }


class ValidatorTest(unittest.TestCase):
    def test_complete_report_passes(self) -> None:
        summary, errors = validate_report(build_report())

        self.assertEqual([], errors)
        self.assertEqual(set(REQUIRED_WORKLOADS), set(summary["workloads"]))

    def test_latency_regression_fails(self) -> None:
        report = build_report()
        for run in report["profiles"]["loaded_disabled"]["workloads"]["http_read"]:
            run["p95_ms"] = 103.0

        _, errors = validate_report(report)

        self.assertTrue(any("loaded_disabled.http_read.p95_ms" in error for error in errors))

    def test_missing_representative_workload_fails(self) -> None:
        report = build_report()
        del report["profiles"]["baseline"]["workloads"]["outbound_http"]

        _, errors = validate_report(report)

        self.assertTrue(any("baseline.outbound_http" in error for error in errors))

    def test_collector_isolation_and_longevity_failures_are_detected(self) -> None:
        report = copy.deepcopy(build_report())
        report["collector_faults"]["unreachable"][0]["p99_ms"] = 500.0
        report["collector_faults"]["unreachable"][1]["p99_ms"] = 500.0
        report["collector_faults"]["unreachable"][2]["p99_ms"] = 500.0
        report["messenger_longevity"]["rss_after_mib"] = 106.0
        report["messenger_longevity"]["active_contexts_end"] = 1

        _, errors = validate_report(report)

        self.assertTrue(any("unreachable.p99_ms" in error for error in errors))
        self.assertTrue(any("RSS growth" in error for error in errors))
        self.assertTrue(any("active context/scope" in error for error in errors))

    def test_malformed_nested_values_are_reported_without_crashing(self) -> None:
        report = build_report()
        report["profiles"]["baseline"]["workloads"]["http_read"][0] = {"pair_id": 1}
        report["collector_faults"]["slow"][0] = "invalid"
        report["export_accounting"]["always_on"]["exported"] = None

        _, errors = validate_report(report)

        self.assertTrue(any("baseline.http_read.run[0].p95_ms" in error for error in errors))
        self.assertTrue(any("collector_faults.slow[0]" in error for error in errors))
        self.assertTrue(any("all counters" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
