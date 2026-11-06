//! Read-only OLED panel snapshot. Every source is optional: a firmware that lacks one still
//! returns the rest, and Luma does the arithmetic so it can be tested without a TV.
use crate::{
    Result,
    system::{self, System},
};
use serde_json::{Map, Value, json};

const DIRECTORY: &str = "/mnt/lg/cmn_data/pnwash";
/// The panel-care records worth reading; each is a few bytes of text.
const RECORDS: [&str; 13] = [
    "autoJbInterval",
    "autoJbLastTime",
    "autoPnwashTime",
    "autoOffRsInterval",
    "autoOffRsIntervalHomeMode",
    "autoOffRsLastTime",
    "autoOffRsTime",
    "completedJbCount",
    "completedOffRsCount",
    "failAlertCount",
    "gsrStressCount",
    "gsrOff",
    "tpcOff",
];

fn record(system: &impl System, name: &str) -> Value {
    system::read_virtual(system, format!("{DIRECTORY}/{name}").as_str(), 256)
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

pub fn collect(system: &impl System) -> Result<Value> {
    system::root(system)?;
    let records: Map<String, Value> = RECORDS
        .iter()
        .map(|name| ((*name).to_owned(), record(system, name)))
        .collect();
    Ok(json!({
        "returnValue": true,
        // `panelUsageTime` counts ten-minute units.
        "panel": service(
            system,
            "com.webos.service.panelcontroller",
            "getPanelUsageTime",
            json!({"subscribe": false}),
        ),
        "gsr": service(system, "com.webos.service.oledepl", "getGlobalStressReduction", json!({})),
        "tpc": service(system, "com.webos.service.oledepl", "getTemporalPeakControl", json!({})),
        // The whole category: a key list makes the service fail when one key is missing.
        "picture": service(
            system,
            "com.webos.service.settings",
            "getSystemSettings",
            json!({"category": "picture"}),
        ),
        "records": records,
    }))
}
