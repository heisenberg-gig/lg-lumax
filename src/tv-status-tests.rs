use super::*;
use serde_json::json;

fn full() -> Value {
    json!({
        "returnValue": true,
        "picture": {
            "returnValue": true,
            "settings": {"pictureMode": "dolbyHdrCinema", "backlight": "72", "energySaving": "off"},
            "dimension": {"dynamicRange": "hdr", "input": "hdmi1"},
        },
        "video": {"returnValue": true, "video": [
            {"connected": false, "videoInfo": {"hdrType": "none"}},
            {"connected": true, "videoInfo": {
                "hdrType": "dolby_vision", "isGameMode": true, "freesyncEnabled": false}},
        ]},
        "panel": {"returnValue": true, "panelUsageTime": "7710"},
        "temperature": "43\n",
        "meminfo": "MemTotal:        2900000 kB\nMemFree: 100 kB\nMemAvailable:    1000000 kB\n",
    })
}

#[test]
fn normalizes_a_complete_reply() {
    let status = normalize(&full()).unwrap();
    assert_eq!(status.picture_mode.as_deref(), Some("Dolby Vision Cinema"));
    assert_eq!(status.oled_light, Some(72));
    assert_eq!(status.energy_saving.as_deref(), Some("off"));
    assert_eq!(status.hdr.as_deref(), Some("Dolby Vision"));
    assert_eq!(status.game_mode, Some(true));
    assert_eq!(status.freesync, Some(false));
    assert_eq!(status.temperature_c, Some(43));
    assert_eq!(status.panel_hours, Some(1285));
    assert_eq!(status.memory_total_mb, Some(2832));
    assert_eq!(status.memory_used_mb, Some(1855));
}

#[test]
fn missing_sources_become_none_without_failing() {
    let status = normalize(&json!({
        "returnValue": true, "picture": null, "video": null, "panel": null,
        "temperature": null, "meminfo": null,
        "extra": 1,
    }));
    assert!(status.is_err());
    let status = normalize(&json!({"picture": {"settings": {"pictureMode": "vivid"}},
        "video": null, "panel": null, "temperature": null, "meminfo": null}))
    .unwrap();
    assert_eq!(status.picture_mode.as_deref(), Some("Vivid"));
    assert_eq!(status.hdr, None);
    assert_eq!(status.game_mode, None);
    assert_eq!(status.panel_hours, None);
}

#[test]
fn hdr_falls_back_to_the_dynamic_range_without_a_connected_sink() {
    let mut reply = full();
    reply["video"] = json!({"video": [{"connected": false}]});
    assert_eq!(normalize(&reply).unwrap().hdr.as_deref(), Some("HDR10"));
    reply["picture"]["dimension"]["dynamicRange"] = json!("sdrALLM");
    assert_eq!(normalize(&reply).unwrap().hdr.as_deref(), Some("SDR"));
}

#[test]
fn temperature_accepts_degrees_and_millidegrees_and_rejects_nonsense() {
    assert_eq!(temperature(&json!("46\n")), Some(46));
    assert_eq!(temperature(&json!("51000\n")), Some(51));
    assert_eq!(temperature(&json!("999999999")), None);
    assert_eq!(temperature(&json!("hot")), None);
    assert_eq!(temperature(&Value::Null), None);
}

#[test]
fn names_follow_lg_terms_and_spell_unknown_keys() {
    assert_eq!(picture_mode("cinema"), "Cinema");
    assert_eq!(picture_mode("hdrCinema"), "HDR Cinema");
    assert_eq!(picture_mode("filmMaker"), "Filmmaker Mode");
    assert_eq!(picture_mode("dolbyHdrGame"), "Dolby Vision Game");
    assert_eq!(picture_mode("sportsBright"), "Sports Bright");
    assert_eq!(hdr_format("hlg").as_deref(), Some("HLG"));
    assert_eq!(
        hdr_format("dolby_ll").as_deref(),
        Some("Dolby Vision (low latency)")
    );
    assert_eq!(hdr_format(""), None);
}

#[test]
fn panel_hours_reject_implausible_values_and_oled_light_stays_in_range() {
    let mut reply = full();
    reply["panel"]["panelUsageTime"] = json!("99999999");
    reply["picture"]["settings"]["backlight"] = json!(250);
    let status = normalize(&reply).unwrap();
    assert_eq!(status.panel_hours, None);
    assert_eq!(status.oled_light, None);
    assert!(normalize(&json!("nope")).is_err());
}
