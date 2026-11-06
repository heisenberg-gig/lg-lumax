//! Read-only system readings (temperature, load, memory, storage, identity). Every source is
//! optional: a missing file or refused service becomes `null`, so firmware differences never hide
//! the other readings. Luma parses the raw text, which keeps the parsing testable off the TV.
use crate::{
    Result,
    system::{self, System},
};
use serde_json::{Value, json};
use std::time::Duration;

const APP_STORAGE: &str = "/mnt/lg/appstore";
/// native-tv keeps the last battery level its Bluetooth read got; the remote only reports a level
/// while it is awake, so this file is the best source when it is installed.
const NATIVE_TV_BATTERY: &str = "/var/lib/tvweb/remote-battery.json";

/// Kernel files (/proc, /sys) are read by bytes: their advertised size is 0 or a whole page.
fn file(system: &impl System, path: &str, limit: usize) -> Value {
    system::read_virtual(system, path, limit)
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

/// The remote's pairing file differs between webOS releases; the first that exists wins.
fn pairing(system: &impl System) -> Value {
    [
        "/mnt/lg/cmn_data/mrcu/mrcu1.info",
        "/mnt/lg/cmn_data/btsvc/mrcu1.info",
    ]
    .iter()
    .map(|path| file(system, path, 16 * 1024))
    .find(|value| !value.is_null())
    .unwrap_or(Value::Null)
}

fn storage(system: &impl System) -> Value {
    system
        .run(
            "/bin/df",
            &["-k".into(), APP_STORAGE.into()],
            Duration::from_secs(5),
            16 * 1024,
        )
        .ok()
        .filter(|output| output.success)
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map_or(Value::Null, Value::String)
}

pub fn collect(system: &impl System) -> Result<Value> {
    system::root(system)?;
    let lg_temperature = file(system, "/proc/lg/pm/temperature", 256);
    let emmc = "/sys/block/mmcblk0/device";
    Ok(json!({
        "returnValue": true,
        "meminfo": file(system, "/proc/meminfo", 16 * 1024),
        // `{"lg": true}` marks degrees; the thermal zone reports millidegrees.
        "temperature": if lg_temperature.is_null() {
            json!({"lg": false, "value": file(system, "/sys/class/thermal/thermal_zone0/temp", 256)})
        } else {
            json!({"lg": true, "value": lg_temperature})
        },
        "frequency": file(system, "/proc/lg/pm/frequency", 256),
        "pm_status": file(system, "/proc/lg/pm/status", 16 * 1024),
        "cpu_online": file(system, "/sys/devices/system/cpu/online", 256),
        "gpu_status": file(system, "/proc/lg/sys/status", 64 * 1024),
        "emmc_life_time": file(system, &format!("{emmc}/life_time"), 256),
        "emmc_pre_eol": file(system, &format!("{emmc}/pre_eol_info"), 256),
        "storage": storage(system),
        "environment": file(system, "/var/luna/preferences/environmentCondition", 64 * 1024),
        "chip_name": file(system, "/proc/lg/base/chip_name", 256),
        "issue": file(system, "/etc/issue", 1024),
        "release": file(system, "/etc/starfish-release", 1024),
        "properties": service(
            system,
            "com.webos.service.tv.systemproperty",
            "getSystemProperties",
            json!({"keys": ["modelName", "firmwareVersion", "boardType", "sdkVersion"]}),
        ),
        "connection": service(system, "com.palm.connectionmanager", "getStatus", json!({})),
        "net_dev": file(system, "/proc/net/dev", 16 * 1024),
        "wireless": file(system, "/proc/net/wireless", 16 * 1024),
        "mac": {
            "wlan0": file(system, "/sys/class/net/wlan0/address", 64),
            "eth0": file(system, "/sys/class/net/eth0/address", 64),
        },
        "remote": service(
            system,
            "com.webos.service.mrcu",
            "getDeviceInfo",
            json!({"subscribe": false, "deviceName": "MagicRemote_1"}),
        ),
        "remote_pairing": pairing(system),
        "remote_saved": file(system, NATIVE_TV_BATTERY, 4 * 1024),
        // `time.ontime` counts seconds since the TV last powered on (0 in standby).
        "power": service(system, "com.webos.service.tvpower", "power/getPowerOnTime", json!({})),
    }))
}
