use super::*;
use serde_json::json;

fn full() -> Value {
    json!({
        "returnValue": true,
        "picture": {
            "returnValue": true,
            "settings": {
                "pictureMode": "dolbyHdrCinema", "backlight": "72", "brightness": "50",
                "contrast": 85, "color": 52, "sharpness": "10",
                "blackLevel": {"ntsc": "auto", "unknown": "low"},
                "colorTemperature": "warm2", "gamma": "mediumHavingLevel",
                "dynamicContrast": "off", "peakBrightness": "high",
                "superResolution": "low", "noiseReduction": "medium",
                "energySaving": "min", "ignored": "x",
            },
            "dimension": {"dynamicRange": "hdr", "input": "hdmi1"},
        },
        "video": {"returnValue": true, "video": [
            {"connected": false, "videoInfo": {"hdrType": "none"}},
            {"connected": true, "videoInfo": {
                "hdrType": "dolby_vision", "isGameMode": true, "freesyncEnabled": false}},
        ]},
        "audio": {"audioInputProperty": [{"codec": "eac3", "immersive": "atmos"}]},
    })
}

fn find<'a>(status: &'a TvStatus, key: &str) -> Option<&'a Reading> {
    status
        .picture
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

#[test]
fn normalizes_a_complete_reply_in_display_order() {
    use Reading::{Number, Text};
    let status = normalize(&full()).unwrap();
    let keys: Vec<_> = status.picture.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        [
            "tvPictureMode",
            "tvOledLight",
            "tvContrast",
            "tvBlackLevel",
            "tvDynamicContrast",
            "tvPeakBrightness",
            "tvGamma",
            "tvSharpness",
            "tvSuperResolution",
            "tvNoiseReduction",
            "tvColor",
            "tvColorTemperature",
            "tvEnergySaving"
        ]
    );
    assert_eq!(
        find(&status, "tvPictureMode"),
        Some(&Text("Dolby Vision Cinema".into()))
    );
    assert_eq!(find(&status, "tvOledLight"), Some(&Number(72)));
    assert_eq!(find(&status, "tvContrast"), Some(&Number(85)));
    assert_eq!(
        find(&status, "tvBlackLevel"),
        Some(&Number(50)),
        "the 0-100 slider, not the per-input auto/low/high setting"
    );
    assert_eq!(
        find(&status, "tvColorTemperature"),
        Some(&Text("Warm 2".into()))
    );
    assert_eq!(find(&status, "tvGamma"), Some(&Text("2.2".into())));
    assert_eq!(
        find(&status, "tvDynamicContrast"),
        Some(&Text("off".into()))
    );
    assert_eq!(
        find(&status, "tvPeakBrightness"),
        Some(&Text("high".into()))
    );
    assert_eq!(
        find(&status, "tvSuperResolution"),
        Some(&Text("low".into()))
    );
    assert_eq!(
        find(&status, "tvNoiseReduction"),
        Some(&Text("medium".into()))
    );
    assert_eq!(status.hdr.as_deref(), Some("Dolby Vision"));
    assert_eq!(status.game_mode, Some(true));
    assert_eq!(status.freesync, Some(false));
    assert_eq!(status.audio.as_deref(), Some("Dolby Digital Plus"));
    assert_eq!(status.audio_immersive.as_deref(), Some("Dolby Atmos"));
}

#[test]
fn missing_sources_are_omitted_and_an_empty_reply_is_an_error() {
    assert!(normalize(&json!({"picture": null, "video": null})).is_err());
    assert!(normalize(&json!("nope")).is_err());
    let status =
        normalize(&json!({"picture": {"settings": {"pictureMode": "vivid"}}, "video": null}))
            .unwrap();
    assert_eq!(status.picture.len(), 1);
    assert_eq!(status.hdr, None);
    assert_eq!(status.game_mode, None);
}

#[test]
fn gamma_names_and_audio_formats_follow_lg_terms() {
    let mut reply = full();
    for (raw, shown) in [
        ("high2", "BT.1886"),
        ("medium", "2.2"),
        ("low", "1.9"),
        ("high1", "2.4"),
    ] {
        reply["picture"]["settings"]["gamma"] = json!(raw);
        assert_eq!(
            find(&normalize(&reply).unwrap(), "tvGamma"),
            Some(&Reading::Text(shown.into()))
        );
    }
    reply["audio"] = json!({"audioInputProperty": [{"codec": "ac3", "immersive": "none"}]});
    assert_eq!(
        normalize(&reply).unwrap().audio.as_deref(),
        Some("Dolby Digital")
    );
    reply["audio"] = json!({"audioInputProperty": []});
    assert_eq!(normalize(&reply).unwrap().audio, None);
    reply["audio"] = json!(null);
    assert_eq!(normalize(&reply).unwrap().audio, None);
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
fn names_follow_lg_terms_and_spell_unknown_keys() {
    assert_eq!(picture_mode("cinema"), "Cinema");
    assert_eq!(picture_mode("hdrCinema"), "HDR Cinema");
    assert_eq!(picture_mode("filmMaker"), "Filmmaker Mode");
    assert_eq!(picture_mode("dolbyHdrGame"), "Dolby Vision Game");
    assert_eq!(picture_mode("sportsBright"), "Sports Bright");
    assert_eq!(spell("expert1"), "Expert 1");
    assert_eq!(hdr_format("hlg").as_deref(), Some("HLG"));
    assert_eq!(
        hdr_format("dolby_ll").as_deref(),
        Some("Dolby Vision (low latency)")
    );
    assert_eq!(hdr_format(""), None);
}

#[test]
fn implausible_numbers_are_dropped() {
    let mut reply = full();
    reply["picture"]["settings"]["backlight"] = json!(250000);
    assert_eq!(find(&normalize(&reply).unwrap(), "tvOledLight"), None);
}
