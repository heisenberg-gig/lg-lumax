use super::*;
use serde_json::json;

fn full() -> Value {
    json!({
        "returnValue": true,
        "meminfo": "MemTotal:        2900000 kB\nMemFree: 100 kB\nMemAvailable:    1000000 kB\nSwapTotal: 1000000 kB\nSwapFree: 400000 kB\n",
        "temperature": {"lg": true, "value": "43\n"},
        "frequency": "1500000\n",
        "pm_status": "cpu_num: 4\nload: 10 20 30 40\nother: 1\n",
        "cpu_online": "0-3\n",
        "gpu_status": "GPU PLL Out : 800000000\n",
        "emmc_life_time": "0x01 0x01\n",
        "storage": "Filesystem 1K-blocks Used Available Use% Mounted on\n/dev/mapper/appstore 3000000 600000 2400000 20% /mnt/lg/appstore\n",
        "environment": "{\"boardTypeStr\":\"_O22_\",\"ddrSize\":\"3GB\",\"panelOutputFrameRate\":\"120\"}",
        "issue": "webOS TV 10.2.0 \\n \\l\n",
        "properties": {"modelName": "OLED55C54LA", "firmwareVersion": "33.22.45", "sdkVersion": "10.2.0"},
        "connection": {"wifi": {"ssid": "Home"}},
    })
}

#[test]
fn normalizes_a_complete_reply() {
    let info = normalize(&full()).unwrap();
    assert_eq!(info.temperature_c, Some(43));
    assert_eq!(info.core_loads, [10, 20, 30, 40]);
    assert_eq!(info.cpu_load, Some(25));
    assert_eq!(info.cpu_mhz, Some(1500));
    assert_eq!(info.gpu_mhz, Some(800));
    assert_eq!(info.memory_free_kb, Some(1_000_000));
    assert_eq!(info.memory_total_kb, Some(2_900_000));
    assert_eq!(info.swap_free_kb, Some(400_000));
    assert_eq!(info.storage_total_kb, Some(3_000_000));
    assert_eq!(info.storage_free_kb, Some(2_400_000));
    assert_eq!(info.storage_used_percent, Some(20));
    assert_eq!(info.emmc_health.as_deref(), Some(">90% (Healthy)"));
    assert_eq!(info.model.as_deref(), Some("OLED55C54LA"));
    assert_eq!(info.firmware.as_deref(), Some("33.22.45"));
    assert_eq!(info.webos.as_deref(), Some("10.2.0"));
    assert_eq!(info.soc.as_deref(), Some("Alpha 9 Gen 5 (O22)"));
    assert_eq!(info.ram.as_deref(), Some("3GB"));
    assert_eq!(info.panel_refresh_hz, Some(120));
    assert_eq!(info.wifi.as_deref(), Some("Home"));
}

#[test]
fn gaps_are_omitted_and_an_empty_reply_is_an_error() {
    assert!(normalize(&json!({"meminfo": null, "temperature": null})).is_err());
    assert!(normalize(&json!("x")).is_err());
    let info = normalize(&json!({"issue": "webOS 4.9"})).unwrap();
    assert_eq!(info.webos.as_deref(), Some("4.9"));
    assert_eq!(info.temperature_c, None);
    assert!(info.core_loads.is_empty());
}

#[test]
fn temperature_handles_both_sources_and_rejects_nonsense() {
    assert_eq!(temperature(&json!({"lg": true, "value": "46"})), Some(46));
    assert_eq!(
        temperature(&json!({"lg": false, "value": "51500"})),
        Some(52)
    );
    assert_eq!(temperature(&json!({"lg": true, "value": "9999"})), None);
    assert_eq!(temperature(&json!({"lg": false, "value": "x"})), None);
    assert_eq!(temperature(&json!({"lg": true, "value": null})), None);
}

#[test]
fn frequencies_accept_khz_and_hz_scales_and_reject_outliers() {
    assert_eq!(frequency_mhz(&json!("1500000")), Some(1500));
    assert_eq!(frequency_mhz(&json!("1500")), Some(1500));
    assert_eq!(frequency_mhz(&json!("5")), None);
    assert_eq!(gpu_mhz(&json!("x\ngpu pll out: 700000000 hz")), Some(700));
    assert_eq!(gpu_mhz(&json!("nothing")), None);
}

#[test]
fn core_loads_follow_the_online_cores_or_cpu_num() {
    let status = json!("cpu_num: 4\nload: 5 6 7 8\n");
    assert_eq!(core_loads(&status, &json!("0,2-3")), [5, 7, 8]);
    assert_eq!(core_loads(&status, &Value::Null), [5, 6, 7, 8]);
    assert_eq!(core_loads(&json!("load: 150"), &Value::Null), [100]);
}

#[test]
fn emmc_health_uses_the_worst_estimate() {
    assert_eq!(
        emmc_health(&json!("0x01 0x01")).as_deref(),
        Some(">90% (Healthy)")
    );
    assert_eq!(
        emmc_health(&json!("0x01 0x05")).as_deref(),
        Some("50% remaining")
    );
    assert_eq!(
        emmc_health(&json!("0x0b 0x01")).as_deref(),
        Some("0% remaining")
    );
    assert_eq!(emmc_health(&json!("0x00 0x00")), None);
}

#[test]
fn storage_and_memory_are_validated() {
    assert_eq!(
        storage(&json!("hdr\n/dev/a 100 40 60 40% /m\n")),
        Some((100, 60, 40))
    );
    assert_eq!(storage(&json!("hdr\n/dev/a 100 400 60 40% /m\n")), None);
    assert_eq!(storage(&json!("garbage")), None);
    let info = normalize(&json!({"meminfo": "MemTotal: 100 kB\nMemAvailable: 900 kB\n"}));
    let info = info.unwrap();
    assert_eq!(info.memory_total_kb, Some(100));
    assert_eq!(
        info.memory_free_kb, None,
        "free memory above total is rejected"
    );
    assert_eq!(size(512), "1 MB");
    assert_eq!(size(2_516_582), "2.4 GB");
}
