//! Read-only picture status snapshot. Every source is optional: one failing service becomes
//! `null`, so a firmware that lacks a source never hides the rest.
use crate::{
    Result,
    system::{self, System},
};
use serde_json::{Value, json};

fn service(system: &impl System, name: &str, method: &str, payload: Value) -> Value {
    system::luna(system, name, method, &payload)
        .ok()
        .filter(|reply| reply["returnValue"] == true)
        .unwrap_or(Value::Null)
}

/// Sound services omit `returnValue` on some firmware; only an explicit `false` is a failure.
fn unflagged_service(system: &impl System, name: &str, method: &str, payload: Value) -> Value {
    system::luna_unflagged(system, name, method, &payload)
        .ok()
        .filter(|reply| reply["returnValue"] != false)
        .unwrap_or(Value::Null)
}

/// Raw replies; Luma normalises them so parsing can be tested without a TV. The whole picture
/// category is read (no key list), so a firmware without some key cannot fail the request.
pub fn collect(system: &impl System) -> Result<Value> {
    system::root(system)?;
    Ok(json!({
        "returnValue": true,
        "picture": service(
            system,
            "com.webos.service.settings",
            "getSystemSettings",
            json!({"category": "picture"}),
        ),
        "video": service(
            system,
            "com.webos.service.videooutput",
            "getStatus",
            json!({"subscribe": false}),
        ),
        // The format of the sound that is playing (codec and Dolby Atmos layer).
        "audio": unflagged_service(
            system,
            "com.webos.service.audio",
            "tv/getAudioInputProperty",
            json!({}),
        ),
    }))
}
