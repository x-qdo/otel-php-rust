use crate::baggage::{
    interfaces::BAGGAGE_BUILDER_INTERFACE,
    metadata::{MetadataClass, init_metadata_object},
};
use phper::{
    classes::{ClassEntity, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};

pub const PARSER_CLASS: &str = r"OpenTelemetry\API\Baggage\Propagation\Parser";
const MAX_BAGGAGE_BYTES: usize = 8_192;
const MAX_BAGGAGE_ENTRIES: usize = 180;

pub type ParserClass = StateClass<Vec<u8>>;

fn trim_php(mut value: &[u8]) -> &[u8] {
    while value
        .first()
        .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t' | 0x0b | 0))
    {
        value = value.get(1..).unwrap_or_default();
    }
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t' | 0x0b | 0))
    {
        value = value
            .get(..value.len().saturating_sub(1))
            .unwrap_or_default();
    }
    value
}

fn is_php_empty(value: &[u8]) -> bool {
    value.is_empty() || value == b"0"
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn url_decode(value: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut index = 0;
    while let Some(byte) = value.get(index).copied() {
        if byte == b'+' {
            decoded.push(b' ');
            index += 1;
            continue;
        }
        if byte == b'%'
            && let (Some(high), Some(low)) = (value.get(index + 1), value.get(index + 2))
            && let (Some(high), Some(low)) = (hex_value(*high), hex_value(*low))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(byte);
        index += 1;
    }
    decoded
}

fn key_has_excluded_character(value: &[u8]) -> bool {
    value.iter().any(|byte| {
        matches!(
            byte,
            b' ' | b'('
                | b')'
                | b'<'
                | b'>'
                | b'@'
                | b','
                | b';'
                | b':'
                | b'\\'
                | b'"'
                | b'/'
                | b'['
                | b']'
                | b'?'
                | b'='
                | b'{'
                | b'}'
        )
    })
}

fn value_has_excluded_character(value: &[u8]) -> bool {
    value
        .iter()
        .any(|byte| matches!(byte, b' ' | b'"' | b',' | b';' | b'\\'))
}

pub fn parse_into_builder(
    header: &[u8],
    builder: &mut phper::objects::ZObj,
    metadata_class: &MetadataClass,
) -> phper::Result<()> {
    if header.len() > MAX_BAGGAGE_BYTES {
        return Ok(());
    }

    let mut accepted = 0;
    for member in header.split(|byte| *byte == b',') {
        if accepted >= MAX_BAGGAGE_ENTRIES {
            break;
        }
        let member = trim_php(member);
        if is_php_empty(member) {
            continue;
        }

        let semicolon = member.iter().position(|byte| *byte == b';');
        let pair = trim_php(match semicolon {
            Some(index) => member.get(..index).unwrap_or_default(),
            None => member,
        });
        if is_php_empty(pair) {
            continue;
        }
        let Some(equals) = pair.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let raw_key = pair.get(..equals).unwrap_or_default();
        let raw_value = pair.get(equals + 1..).unwrap_or_default();
        let key = url_decode(trim_php(raw_key));
        let value = url_decode(trim_php(raw_value));
        let key = trim_php(&key);
        let value = trim_php(&value);
        if is_php_empty(key) || key_has_excluded_character(key) {
            continue;
        }
        if is_php_empty(value) || value_has_excluded_character(value) {
            continue;
        }

        let metadata = semicolon
            .and_then(|index| member.get(index + 1..))
            .map(trim_php)
            .filter(|metadata| !is_php_empty(metadata))
            .map(|metadata| init_metadata_object(metadata_class, metadata.to_vec()).map(ZVal::from))
            .transpose()?
            .unwrap_or_default();

        builder.call(
            "set",
            &mut [
                ZVal::from(key.to_vec()),
                ZVal::from(value.to_vec()),
                metadata,
            ],
        )?;
        accepted += 1;
    }
    Ok(())
}

pub fn make_parser_class(metadata_class: MetadataClass) -> ClassEntity<Vec<u8>> {
    let mut class = ClassEntity::new_with_default_state_constructor(PARSER_CLASS);
    class.set_final();
    class.state_cloner(Clone::clone);

    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            *this.as_mut_state() = crate::util::arg(arguments, 0)?
                .expect_z_str()?
                .to_bytes()
                .to_vec();
            Ok::<_, phper::Error>(())
        })
        .argument(Argument::new("baggageHeader").with_type_hint(ArgumentTypeHint::String));

    class
        .add_method("parseInto", Visibility::Public, move |this, arguments| {
            parse_into_builder(
                this.as_state(),
                crate::util::arg_mut(arguments, 0)?.expect_mut_z_obj()?,
                &metadata_class,
            )
        })
        .argument(
            Argument::new("baggageBuilder").with_type_hint(ArgumentTypeHint::ClassEntry(
                BAGGAGE_BUILDER_INTERFACE.to_string(),
            )),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_decode_matches_form_encoding() {
        assert_eq!(url_decode(b"hello+world%21"), b"hello world!");
        assert_eq!(url_decode(b"%zz%2"), b"%zz%2");
    }

    #[test]
    fn trims_php_ascii_whitespace_only() {
        assert_eq!(trim_php(b" \tvalue\r\n"), b"value");
        assert_eq!(trim_php(b"\xc2\xa0value\xc2\xa0"), b"\xc2\xa0value\xc2\xa0");
    }
}
