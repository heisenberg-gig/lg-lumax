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
        "connection": {
            "isInternetConnectionAvailable": true,
            "wired": {"state": "disconnected"},
            "wifi": {"state": "connected", "ssid": "Home", "ipAddress": "192.168.1.85",
                "netmask": "255.255.255.0", "gateway": "192.168.1.1", "dns1": "192.168.1.1",
                "dns2": "bad", "interfaceName": "wlan0"},
        },
        "net_dev": "Inter-|   Receive\n lo: 1 2 3 4 5 6 7 8 9\nwlan0: 5000 2 3 4 5 6 7 8 900 1\n",
        "wireless": "Inter-| sta\n face | tus | link level\n wlan0: 0000   62.  -48.  -256 0 0\n",
        "mac": {"wlan0": "AA:BB:CC:DD:EE:FF\n", "eth0": null},
        "remote": {"returnValue": true, "batteryLevel": 80},
        "power": {"returnValue": true, "time": {"uptime": "90000", "ontime": "8040"}},
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
    assert_eq!(info.remote_battery, Some(80));
    assert_eq!(info.session_secs, Some(8040));
    let net = &info.network;
    assert_eq!(net.link, Some(Link::Wifi));
    assert_eq!(net.connected, Some(true));
    assert_eq!(net.internet, Some(true));
    assert_eq!(net.ssid.as_deref(), Some("Home"));
    assert_eq!(net.ip.as_deref(), Some("192.168.1.85"));
    assert_eq!(net.netmask.as_deref(), Some("255.255.255.0"));
    assert_eq!(net.gateway.as_deref(), Some("192.168.1.1"));
    assert_eq!(net.dns, ["192.168.1.1"], "invalid DNS entries are dropped");
    assert_eq!((net.signal_quality, net.signal_dbm), (Some(62), Some(-48)));
    assert_eq!(net.interface.as_deref(), Some("wlan0"));
    assert_eq!(net.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    assert_eq!((net.rx_bytes, net.tx_bytes), (Some(5000), Some(900)));
}

#[test]
fn wired_connections_use_the_ethernet_interface_and_hide_wifi_fields() {
    let mut reply = full();
    reply["connection"] = json!({
        "wired": {"state": "connected", "ipAddress": "10.0.0.5", "interfaceName": "eth0"},
        "wifi": {"state": "disconnected"},
    });
    reply["net_dev"] = json!("eth0: 10 2 3 4 5 6 7 8 20\n");
    reply["mac"]["eth0"] = json!("00:00:5e:00:53:01\n");
    let net = normalize(&reply).unwrap().network;
    assert_eq!(net.link, Some(Link::Wired));
    assert_eq!(net.ssid, None);
    assert_eq!((net.signal_quality, net.signal_dbm), (None, None));
    assert_eq!(net.interface.as_deref(), Some("eth0"));
    assert_eq!(net.mac.as_deref(), Some("00:00:5e:00:53:01"));
    assert_eq!((net.rx_bytes, net.tx_bytes), (Some(10), Some(20)));
}

#[test]
fn network_gaps_and_bad_interface_names_are_safe() {
    let mut reply = full();
    reply["connection"] = Value::Null;
    reply["net_dev"] = Value::Null;
    assert_eq!(normalize(&reply).unwrap().network, Network::default());
    reply["connection"] = json!({"wifi": {"state": "connected", "interfaceName": "../x; rm"}});
    reply["wireless"] = json!("wlan0: 0000 0 0");
    let net = normalize(&reply).unwrap().network;
    assert_eq!(net.interface.as_deref(), Some("wlan0"));
    assert_eq!(
        net.signal_dbm, None,
        "a driver with no reading reports nothing"
    );
}

#[test]
fn remote_battery_prefers_the_service_then_the_pairing_file() {
    let mut reply = full();
    reply["remote"] = json!({"batteryLevel": 150});
    reply["remote_pairing"] = json!("name=LGE MR\nbattery=55\n");
    assert_eq!(remote_battery(&reply), Some(55));
    reply["remote_pairing"] = json!("battery=20 (low)\n");
    assert_eq!(
        remote_battery(&reply),
        None,
        "threshold lines are not a level"
    );
    reply["remote"] = json!({"batteryLevel": "73"});
    assert_eq!(remote_battery(&reply), Some(73));
    assert_eq!(remote_battery(&Value::Null), None);
}

#[test]
fn remote_battery_uses_the_saved_level_when_the_remote_is_asleep() {
    let mut reply = full();
    reply["remote"] = json!({"returnValue": true});
    reply["remote_saved"] = json!("{\"level\": 64, \"mac\": \"x\"}");
    reply["remote_pairing"] = json!("battery=20 (low)\n");
    assert_eq!(remote_battery(&reply), Some(64));
    reply["remote"] = json!({"returnValue": true, "batteryLevel": "HIGH"});
    reply["remote_saved"] = json!("{\"level\":63.0,\"connected\":true}");
    assert_eq!(
        remote_battery(&reply),
        Some(63),
        "float level and a word level"
    );
    reply["remote_saved"] = json!("{\"level\": 400}");
    assert_eq!(remote_battery(&reply), None);
    reply["remote_saved"] = json!("not json");
    assert_eq!(remote_battery(&reply), None);
}

#[test]
fn session_time_reads_ontime_and_formats_compactly() {
    let mut reply = full();
    reply["power"] = json!({"time": {"ontime": 0}});
    assert_eq!(normalize(&reply).unwrap().session_secs, Some(0));
    reply["power"] = json!(null);
    assert_eq!(normalize(&reply).unwrap().session_secs, None);
    reply["power"] = json!({"time": {"uptime": "4313.13", "ontime": "377.08"}});
    assert_eq!(normalize(&reply).unwrap().session_secs, Some(377));
    assert_eq!(duration(45), "45s");
    assert_eq!(duration(840), "14m");
    assert_eq!(duration(8040), "2h 14m");
    assert_eq!(duration(3600), "1h 00m");
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

#[test]
fn cpu_model_names_arm_cores_and_counts_them() {
    let arm = json!(
        "processor\t: 0\nCPU implementer\t: 0x41\nCPU part\t: 0xd09\nprocessor\t: 1\nCPU implementer\t: 0x41\nCPU part\t: 0xd09\n"
    );
    assert_eq!(cpu_model(&arm).as_deref(), Some("Cortex-A73 × 2"));
    let named = json!("processor : 0\nmodel name : Some CPU @ 2GHz\n");
    assert_eq!(cpu_model(&named).as_deref(), Some("Some CPU @ 2GHz × 1"));
    assert_eq!(
        cpu_model(&json!(
            "processor : 0\nCPU implementer : 0x99\nCPU part : 0x1\n"
        )),
        None
    );
    assert_eq!(cpu_model(&Value::Null), None);
}

#[test]
fn unnamed_chip_codes_fall_back_to_the_models_year() {
    assert_eq!(soc_from_model("OLED55C54LA"), Some("Alpha 9 AI Gen 8"));
    assert_eq!(soc_from_model("OLED65G56LS"), Some("Alpha 9 AI Gen 8"));
    assert_eq!(soc_from_model("OLED55C44LA"), None);
    assert_eq!(soc_from_model("55UT80006LA"), None);
    let mut reply = full();
    reply["environment"] = json!("{\"boardTypeStr\":\"_O25_\"}");
    reply["properties"] = json!({"modelName": "OLED55C54LA"});
    assert_eq!(
        normalize(&reply).unwrap().soc.as_deref(),
        Some("Alpha 9 AI Gen 8 (O25)")
    );
}
