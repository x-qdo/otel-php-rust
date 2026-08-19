use crate::metrics::meter_provider;
use once_cell::sync::Lazy;
use opentelemetry::{KeyValue, Value};
use opentelemetry_sdk::metrics::{
    InMemoryMetricExporter,
    data::{AggregatedMetrics, MetricData},
};
use phper::{
    arrays::ZArray,
    classes::{ClassEntity, Visibility},
    functions::ReturnType,
    types::ReturnTypeHint,
};
use std::convert::Infallible;

const MEMORY_EXPORTER_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\MemoryMetricsExporter";

pub static MEMORY_EXPORTER: Lazy<InMemoryMetricExporter> =
    Lazy::new(InMemoryMetricExporter::default);

trait Number: Copy {
    fn as_f64(self) -> f64;
}

impl Number for f64 {
    fn as_f64(self) -> f64 {
        self
    }
}
impl Number for i64 {
    fn as_f64(self) -> f64 {
        self as f64
    }
}
impl Number for u64 {
    fn as_f64(self) -> f64 {
        self as f64
    }
}

fn attributes<'a>(values: impl Iterator<Item = &'a KeyValue>) -> ZArray {
    let mut result = ZArray::new();
    for value in values {
        match &value.value {
            Value::Bool(v) => result.insert(value.key.as_str(), *v),
            Value::I64(v) => result.insert(value.key.as_str(), *v),
            Value::F64(v) => result.insert(value.key.as_str(), *v),
            Value::String(v) => result.insert(value.key.as_str(), v.as_str()),
            Value::Array(v) => result.insert(value.key.as_str(), format!("{v:?}")),
            _ => result.insert(value.key.as_str(), format!("{:?}", value.value)),
        }
    }
    result
}

fn serialize_data<T: Number + std::fmt::Debug>(data: &MetricData<T>) -> (&'static str, ZArray) {
    let mut points = ZArray::new();
    match data {
        MetricData::Gauge(gauge) => {
            for point in gauge.data_points() {
                let mut item = ZArray::new();
                item.insert("value", point.value().as_f64());
                item.insert("attributes", attributes(point.attributes()));
                points.insert((), item);
            }
            ("gauge", points)
        }
        MetricData::Sum(sum) => {
            for point in sum.data_points() {
                let mut item = ZArray::new();
                item.insert("value", point.value().as_f64());
                item.insert("attributes", attributes(point.attributes()));
                points.insert((), item);
            }
            (
                if sum.is_monotonic() {
                    "counter"
                } else {
                    "up_down_counter"
                },
                points,
            )
        }
        MetricData::Histogram(histogram) => {
            for point in histogram.data_points() {
                let mut item = ZArray::new();
                item.insert("count", point.count() as i64);
                item.insert("sum", point.sum().as_f64());
                if let Some(min) = point.min() {
                    item.insert("min", min.as_f64());
                }
                if let Some(max) = point.max() {
                    item.insert("max", max.as_f64());
                }
                let mut bounds = ZArray::new();
                for value in point.bounds() {
                    bounds.insert((), value);
                }
                let mut bucket_counts = ZArray::new();
                for value in point.bucket_counts() {
                    bucket_counts.insert((), value as i64);
                }
                item.insert("bounds", bounds);
                item.insert("bucket_counts", bucket_counts);
                item.insert("attributes", attributes(point.attributes()));
                points.insert((), item);
            }
            ("histogram", points)
        }
        MetricData::ExponentialHistogram(histogram) => {
            for point in histogram.data_points() {
                let mut item = ZArray::new();
                item.insert("debug", format!("{point:?}"));
                points.insert((), item);
            }
            ("exponential_histogram", points)
        }
    }
}

pub fn make_memory_exporter_class() -> ClassEntity<()> {
    let mut class = ClassEntity::new(MEMORY_EXPORTER_CLASS_NAME);
    class.set_final();
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_static_method("forceFlush", Visibility::Public, |_| {
            meter_provider::force_flush();
            Ok::<_, Infallible>(())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
        .add_static_method("reset", Visibility::Public, |_| {
            MEMORY_EXPORTER.reset();
            Ok::<_, Infallible>(())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
        .add_static_method("count", Visibility::Public, |_| {
            let count = MEMORY_EXPORTER
                .get_finished_metrics()
                .unwrap_or_default()
                .iter()
                .flat_map(|resource| resource.scope_metrics())
                .flat_map(|scope| scope.metrics())
                .count();
            Ok::<_, Infallible>(count as i64)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Int));

    class
        .add_static_method("getMetrics", Visibility::Public, |_| {
            let mut result = ZArray::new();
            for resource in MEMORY_EXPORTER.get_finished_metrics().unwrap_or_default() {
                for scope in resource.scope_metrics() {
                    for metric in scope.metrics() {
                        let (kind, points) = match metric.data() {
                            AggregatedMetrics::F64(data) => serialize_data(data),
                            AggregatedMetrics::I64(data) => serialize_data(data),
                            AggregatedMetrics::U64(data) => serialize_data(data),
                        };
                        let mut item = ZArray::new();
                        item.insert("name", metric.name());
                        item.insert("description", metric.description());
                        item.insert("unit", metric.unit());
                        item.insert("kind", kind);
                        item.insert("scope", scope.scope().name());
                        item.insert("data_points", points);
                        result.insert((), item);
                    }
                }
            }
            Ok::<_, Infallible>(result)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Array));

    class
}
