use opentelemetry::logs::Severity as OtelSeverity;
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntry, Visibility},
    enums::EnumEntity,
    errors::ThrowObject,
    functions::{Argument, ReturnType},
    objects::ZObject,
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::{ZVal, ZValRef},
};

pub const SEVERITY_ENUM_NAME: &str = r"OpenTelemetry\API\Logs\Severity";

const CASES: [(&str, i64); 24] = [
    ("TRACE", 1),
    ("TRACE2", 2),
    ("TRACE3", 3),
    ("TRACE4", 4),
    ("DEBUG", 5),
    ("DEBUG2", 6),
    ("DEBUG3", 7),
    ("DEBUG4", 8),
    ("INFO", 9),
    ("INFO2", 10),
    ("INFO3", 11),
    ("INFO4", 12),
    ("WARN", 13),
    ("WARN2", 14),
    ("WARN3", 15),
    ("WARN4", 16),
    ("ERROR", 17),
    ("ERROR2", 18),
    ("ERROR3", 19),
    ("ERROR4", 20),
    ("FATAL", 21),
    ("FATAL2", 22),
    ("FATAL3", 23),
    ("FATAL4", 24),
];

pub fn make_severity_enum() -> EnumEntity<i64> {
    let mut entity = EnumEntity::new(SEVERITY_ENUM_NAME);
    for (name, value) in CASES {
        entity.add_case(name, value);
    }
    let severity = entity.bound_enum();
    entity
        .add_static_method("fromPsr3", Visibility::Public, move |arguments| {
            let level = crate::util::arg(arguments, 0)?
                .expect_z_str()?
                .to_str()?
                .to_ascii_lowercase();
            let case = match level.as_str() {
                "debug" => "DEBUG",
                "info" => "INFO",
                "notice" => "INFO2",
                "warning" => "WARN",
                "error" => "ERROR",
                "critical" => "ERROR2",
                "alert" => "ERROR3",
                "emergency" => "FATAL",
                _ => return Err(value_error(format!("Unknown severity: {level}"))?),
            };
            let object = unsafe { severity.clone().get_mut_case(case)? };
            Ok::<_, phper::Error>(object.to_ref_owned())
        })
        .argument(Argument::new("level").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            "self".to_string(),
        )));
    entity
}

fn value_error(message: String) -> phper::Result<phper::Error> {
    let class = ClassEntry::from_globals("ValueError")?;
    let object = ZObject::new(class, [ZVal::from(message)])?;
    Ok(ThrowObject::new(object)?.into())
}

pub fn severity_number(value: &ZVal) -> phper::Result<i64> {
    match value.to_value()? {
        ZValRef::Long(number) => Ok(number),
        ZValRef::Obj(object) => object.get_property("value").expect_long(),
        _ => value.expect_long(),
    }
}

pub fn otel_severity(number: i64) -> Option<OtelSeverity> {
    Some(match number {
        1 => OtelSeverity::Trace,
        2 => OtelSeverity::Trace2,
        3 => OtelSeverity::Trace3,
        4 => OtelSeverity::Trace4,
        5 => OtelSeverity::Debug,
        6 => OtelSeverity::Debug2,
        7 => OtelSeverity::Debug3,
        8 => OtelSeverity::Debug4,
        9 => OtelSeverity::Info,
        10 => OtelSeverity::Info2,
        11 => OtelSeverity::Info3,
        12 => OtelSeverity::Info4,
        13 => OtelSeverity::Warn,
        14 => OtelSeverity::Warn2,
        15 => OtelSeverity::Warn3,
        16 => OtelSeverity::Warn4,
        17 => OtelSeverity::Error,
        18 => OtelSeverity::Error2,
        19 => OtelSeverity::Error3,
        20 => OtelSeverity::Error4,
        21 => OtelSeverity::Fatal,
        22 => OtelSeverity::Fatal2,
        23 => OtelSeverity::Fatal3,
        24 => OtelSeverity::Fatal4,
        _ => return None,
    })
}
