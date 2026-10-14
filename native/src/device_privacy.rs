use anyhow::{Result, ensure};
use rmcp::model::ClientJsonRpcMessage;
use serde_json::Value;

#[derive(Clone, Copy)]
enum Field {
    Zone,
    Offset,
}
fn key(name: &str) -> String {
    name.rsplit('/')
        .next()
        .unwrap_or(name)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}
fn bare(name: &str) -> Option<Field> {
    match name {
        "timezone" | "tz" | "timezoneid" | "ianatimezone" => Some(Field::Zone),
        "timezoneoffset"
        | "timezoneoffsetminutes"
        | "utcoffset"
        | "utcoffsetminutes"
        | "utcoffsetseconds" => Some(Field::Offset),
        _ => None,
    }
}
fn qualified(name: &str) -> Option<Field> {
    ["device", "system", "client", "browser", "local"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix).and_then(bare))
}
fn context(name: &str) -> bool {
    matches!(
        name,
        "meta"
            | "device"
            | "deviceinfo"
            | "devicecontext"
            | "system"
            | "systeminfo"
            | "systemcontext"
            | "clientinfo"
            | "clientcontext"
            | "browser"
            | "browserinfo"
            | "browsercontext"
            | "userlocation"
    )
}
fn opaque(name: &str) -> bool {
    matches!(
        name,
        "content"
            | "text"
            | "message"
            | "prompt"
            | "code"
            | "files"
            | "schema"
            | "requestedschema"
            | "inputschema"
            | "outputschema"
            | "requeststate"
            | "default"
            | "enum"
            | "examples"
    )
}
fn replace(value: &mut Value, field: Field) -> bool {
    let canonical = match field {
        Field::Zone => Value::String("UTC".into()),
        Field::Offset if value.is_string() => Value::String("0".into()),
        Field::Offset => Value::from(0),
    };
    let changed = *value != canonical;
    *value = canonical;
    changed
}
fn zone(value: &mut Value, depth: usize) -> Result<bool> {
    ensure!(depth <= 128, "Device metadata nesting limit exceeded.");
    if let Some(object) = value.as_object_mut() {
        let mut found = false;
        let mut changed = false;
        for (name, member) in object {
            let name = key(name);
            let field = match name.as_str() {
                "name" | "id" | "zone" => Some(Field::Zone),
                "offset" | "offsetminutes" | "offsetseconds" => Some(Field::Offset),
                _ => bare(&name),
            };
            if let Some(field) = field {
                found = true;
                changed |= replace(member, field);
            } else if !opaque(&name) {
                match qualified(&name) {
                    Some(Field::Zone) => changed |= zone(member, depth + 1)?,
                    Some(Field::Offset) => changed |= replace(member, Field::Offset),
                    None => changed |= walk(member, context(&name), depth + 1)?,
                }
            }
        }
        if found {
            return Ok(changed);
        }
    }
    Ok(replace(value, Field::Zone))
}
fn walk(value: &mut Value, metadata: bool, depth: usize) -> Result<bool> {
    ensure!(depth <= 128, "Device metadata nesting limit exceeded.");
    let mut changed = false;
    match value {
        Value::Object(object) => {
            for (name, child) in object {
                let name = key(name);
                if opaque(&name) {
                    continue;
                }
                match qualified(&name).or_else(|| metadata.then(|| bare(&name)).flatten()) {
                    Some(Field::Zone) => changed |= zone(child, depth + 1)?,
                    Some(Field::Offset) => changed |= replace(child, Field::Offset),
                    None => changed |= walk(child, metadata || context(&name), depth + 1)?,
                }
            }
        }
        Value::Array(array) => {
            for child in array {
                changed |= walk(child, metadata, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(changed)
}
fn normalize(value: &mut Value) -> Result<bool> {
    let mut changed = false;
    for name in ["params", "result"] {
        if let Some(payload) = value.get_mut(name) {
            changed |= walk(payload, false, 0)?;
        }
    }
    Ok(changed)
}

/// Normalize recognized structured device metadata without collecting the OS timezone.
pub fn outgoing(message: ClientJsonRpcMessage) -> Result<ClientJsonRpcMessage> {
    // rmcp stores decoded _meta in Extensions; serialize the merged wire envelope.
    let mut value = serde_json::to_value(&message)
        .map_err(|_| anyhow::anyhow!("Device metadata serialization failed."))?;
    if !normalize(&mut value)? {
        return Ok(message);
    }
    serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("Device metadata normalization failed."))
}

/// Return new UTF-8 bytes only when privacy fields changed; retain other wire bytes.
pub fn body(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let mut value: Value =
        serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("Invalid MCP JSON body."))?;
    ensure!(
        value.is_object() && value["jsonrpc"] == "2.0",
        "Invalid MCP message envelope."
    );
    if !normalize(&mut value)? {
        return Ok(None);
    }
    serde_json::to_vec(&value)
        .map(Some)
        .map_err(|_| anyhow::anyhow!("Device metadata serialization failed."))
}

/// Only device-qualified routing hints are normalized; bare tool arguments are not telemetry.
pub fn header(name: &str) -> Option<&'static str> {
    let field = qualified(&key(name.strip_prefix("mcp-param-")?))?;
    Some(match field {
        Field::Zone => "UTC",
        Field::Offset => "0",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn supported_aliases_shapes_and_opaque_business_data_follow_the_contract() {
        for name in [
            "deviceTimezone",
            "SYSTEM_TIME_ZONE",
            "browser-timezone",
            "localTimezone",
            "client.timezone",
        ] {
            for value in [
                json!("Asia/Tehran"),
                json!(null),
                json!(["Asia/Tehran"]),
                json!(true),
            ] {
                let mut message = json!({"params":{name:value}});
                assert!(normalize(&mut message).unwrap());
                assert_eq!(message["params"][name], "UTC");
            }
        }
        for value in [
            json!(210),
            json!(-300),
            json!(null),
            json!({"unsafe":"Asia/Tehran"}),
        ] {
            let mut message = json!({"result":{"systemUtcOffsetSeconds":value}});
            normalize(&mut message).unwrap();
            assert_eq!(message["result"]["systemUtcOffsetSeconds"], 0);
        }
        let mut message = json!({"params":{"_meta":{"vendor/deviceContext":[{
            "timezone":{"name":"Asia/Tehran","offsetMinutes":"210","format":"iana"}}]},
            "arguments":{"timezone":"Asia/Tokyo","offset":210,
                "content":{"deviceTimezone":"Asia/Tehran"},"requestState":{"deviceTimezone":"Asia/Tehran"},
                "requestedSchema":{"deviceTimezone":{"default":"Asia/Tehran"}}}}});
        let arguments = message["params"]["arguments"].clone();
        assert!(normalize(&mut message).unwrap());
        assert_eq!(
            message["params"]["_meta"]["vendor/deviceContext"][0]["timezone"],
            json!({"name":"UTC","offsetMinutes":"0","format":"iana"})
        );
        assert_eq!(message["params"]["arguments"], arguments);
        assert!(
            !normalize(&mut message).unwrap(),
            "normalization is idempotent"
        );
    }

    #[test]
    fn unchanged_wire_bytes_and_depth_failure_are_explicit() {
        let original = br#"{ "jsonrpc": "2.0", "id": 1, "params": {"timezone":"Asia/Tokyo"} }"#;
        assert!(body(original).unwrap().is_none());
        assert!(body(b"[]").is_err());
        let mut nested = json!({"deviceTimezone":"Asia/Tehran"});
        for _ in 0..130 {
            nested = json!({"nested":nested});
        }
        let mut message = json!({"params":nested});
        assert!(normalize(&mut message).is_err());
        assert_eq!(header("mcp-param-device-timezone"), Some("UTC"));
        assert_eq!(header("mcp-param-client-timezone-offset"), Some("0"));
        assert_eq!(header("mcp-param-timezone"), None);
    }
}
