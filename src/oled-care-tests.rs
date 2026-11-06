use super::*;
use serde_json::json;

fn full() -> Value {
    json!({
        "returnValue": true,
        "panel": {"panelUsageTimeStore": 8440, "panelUsageTime": 8514},
        "gsr": {"enable": true, "stressCount": 4},
        "tpc": {"enable": true},
        "picture": {"settings": {
            "screenShift": "on", "logoLuminanceAdjust": "strong", "scheduledPixelCleaning": "off"
        }},
        "records": {
            "autoOffRsLastTime": "1418\n", "autoOffRsInterval": "4", "autoJbLastTime": "1000",
            "autoJbInterval": "2000", "completedOffRsCount": "112", "completedJbCount": "1",
            "failAlertCount": "0", "gsrStressCount": "9", "autoOffRsTime": null,
        },
    })
}

#[test]
fn panel_hours_are_ten_minute_units_and_the_countdowns_follow_native_tv() {
    let care = normalize(&full()).unwrap();
    assert_eq!(care.panel_hours, Some(1419));
    // 8514 units = 1419.0 h; 1419.0 - 1418 = 1.0 h since the last compensation of a 4 h interval.
    assert_eq!(care.maintenance_secs, Some(3 * 3600));
    // 1419 - 1000 = 419 h since the refresher of a 2000 h interval.
    assert_eq!(care.refresher_hours, Some(1581));
    assert_eq!(care.maintenance_cycles, Some(112));
    assert_eq!(care.refresher_cycles, Some(1));
    assert_eq!(care.failures, Some(0));
}

#[test]
fn protections_come_from_the_live_services_and_the_picture_settings() {
    let care = normalize(&full()).unwrap();
    assert_eq!(care.screen_shift, Some(true));
    assert_eq!(care.logo_dimming.as_deref(), Some("high"));
    assert_eq!(care.asbl, Some(true));
    assert_eq!(care.gsr, Some(true));
    assert_eq!(
        care.gsr_stress_events,
        Some(4),
        "the live count wins over the record"
    );
    assert_eq!(care.scheduled_cleaning, Some(false));
    let mut reply = full();
    reply["gsr"] = json!(null);
    reply["tpc"] = json!(null);
    let care = normalize(&reply).unwrap();
    assert_eq!((care.asbl, care.gsr), (None, None));
    assert_eq!(care.gsr_stress_events, Some(9));
}

#[test]
fn a_missing_usage_reading_falls_back_to_the_compensation_record() {
    let mut reply = full();
    reply["panel"] = json!(null);
    let care = normalize(&reply).unwrap();
    assert_eq!(care.panel_hours, Some(1418));
    assert_eq!(care.maintenance_secs, Some(4 * 3600));
}

#[test]
fn intervals_have_native_tvs_defaults_and_limits() {
    let mut reply = full();
    reply["records"]["autoOffRsInterval"] = json!("99");
    reply["records"]["autoJbInterval"] = json!(null);
    let care = normalize(&reply).unwrap();
    assert_eq!(
        care.maintenance_secs,
        Some(3 * 3600),
        "out of range keeps 4 h"
    );
    assert_eq!(care.refresher_hours, Some(1581), "2000 h by default");
    reply["records"]["autoOffRsIntervalHomeMode"] = json!("6");
    assert_eq!(normalize(&reply).unwrap().maintenance_secs, Some(0));
}

#[test]
fn logo_dimming_words_are_normalised() {
    for (raw, shown) in [
        ("off", "off"),
        ("weak", "low"),
        ("middle", "medium"),
        ("strong", "high"),
        ("extraBright", "Extra Bright"),
    ] {
        assert_eq!(logo_dimming(raw), shown);
    }
}

#[test]
fn nothing_readable_is_an_error() {
    assert!(normalize(&json!(null)).is_err());
    assert!(normalize(&json!({"returnValue": true})).is_err());
}
