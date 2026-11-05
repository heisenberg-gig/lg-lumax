//! Read-only TV status snapshot. Every source is optional: one failing service or file becomes
//! `null`, so a firmware that lacks a source never hides the rest.
use crate::{
    Result,
    system::{self, System},
};
use serde_json::{Value, json};

const PICTURE_KEYS: [&str; 5] = [
    "backlight",
    "pictureMode",
    "energySaving",
    "screenShift",
    "logoLuminanceAdjust",
];

fn text(system: &impl System, path: &str, limit: usize) -> Value {
    system::read(system, path, limit)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map_or(Value::Null, Value::String)
}

fn service(system: &impl System, name: &str, method: &str, payload: Value) -> Value {
    system::luna(system, name, method, &payload)
        .ok()
        .filter(|reply| reply["returnValue"] == true)
        .unwrap_or(Value::Null)
}

/// Raw replies; Luma normalises them so parsing can be tested without a TV.
pub fn collect(system: &impl System) -> Result<Value> {
    system::root(system)?;
    let temperature = match text(system, "/proc/lg/pm/temperature", 256) {
        Value::Null => text(system, "/sys/class/thermal/thermal_zone0/temp", 256),
        value => value,
    };
    Ok(json!({
        "returnValue": true,
        "picture": service(
            system,
            "com.webos.service.settings",
            "getSystemSettings",
            json!({"category": "picture", "keys": PICTURE_KEYS}),
        ),
        "video": service(
            system,
            "com.webos.service.videooutput",
            "getStatus",
            json!({"subscribe": false}),
        ),
        "panel": service(
            system,
            "com.webos.service.panelcontroller",
            "getPanelUsageTime",
            json!({"subscribe": false}),
        ),
        "temperature": temperature,
        "meminfo": text(system, "/proc/meminfo", 16 * 1024),
    }))
}
