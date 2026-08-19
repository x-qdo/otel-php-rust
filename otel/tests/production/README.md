# Production benchmark evidence

`validate_production_benchmark.py` validates representative application performance
evidence. It intentionally does not turn the native span microbenchmarks into a
blanket production approval.

The input report must contain five randomized paired runs on one fixed infrastructure
and dataset for all four profiles: no extension, extension loaded with the SDK
disabled, parent-based 1% with a healthy local Collector, and always-on stress. Every
profile must cover the same replay-safe HTTP read, HTTP write, database, outbound HTTP,
one-shot command, Messenger, and mixed-workload fixtures. Each run records p95, p99,
throughput, CPU per operation, peak worker RSS, application errors, and a digest of the
application output.

The report must also include five healthy/slow/unreachable/rejecting Collector pairs,
exact exporter accounting, a no-network disabled proof, an application-thread syscall
audit, and a post-warm-up 100,000-span Messenger longevity run. Artifact fields may be
CI artifact URLs or immutable object-store references; do not commit credentials,
authenticated requests, tenant data, or raw production payloads.

Run the validator and its unit tests with:

```sh
python3 -m unittest discover -s otel/tests/production -p 'test_*.py'
python3 otel/tests/production/validate_production_benchmark.py /path/to/report.json
```

The validator exits non-zero for incomplete evidence or any breach of the accepted
limits: loaded-disabled 2%/3%/2%/2% for p95/p99/throughput/CPU; parent-based 1%
3%/5%/3%/5%; always-on 8%/10%/10% for p95/p99/CPU; at most 32 MiB extra worker RSS;
Collector-failure deltas bounded by the greater of 1% or 2 ms at p95 and 2% or 5 ms
at p99; and no more than 5 MiB growth or any thread/context/scope leak in the long
worker.

Application configuration, representative datasets, replay traffic, and the fixed
load environment are deliberately deployment-owned inputs. Each adopter should treat
production approval as blocked until a report from its target environment passes this
gate.
