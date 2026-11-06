//! Read-only TV status for the Settings "TV" section. The helper returns raw service replies;
//! everything here is pure parsing so it can be tested without a TV. Names follow LG's own
//! terms (picture modes, HDR formats) and are not translated.
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reading {
    Number(i64),
    /// A name such as `off`, `Cinema` or `Warm 2`; `off` is the only value Luma translates.
    Text(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TvStatus {
    /// Picture settings in display order, as `(message key, reading)`; missing ones are absent.
    pub picture: Vec<(&'static str, Reading)>,
    pub hdr: Option<String>,
    pub game_mode: Option<bool>,
    pub freesync: Option<bool>,
    /// The sound format playing, such as "Dolby Digital Plus", and its immersive layer.
    pub audio: Option<String>,
    pub audio_immersive: Option<String>,
}

impl TvStatus {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// `dolbyHdrCinema` -> "Dolby Vision Cinema"; unknown keys are spelled out from camelCase.
pub fn spell(key: &str) -> String {
    let mut words: Vec<String> = vec![];
    let mut word = String::new();
    for c in key.chars().filter(|c| !matches!(c, '_' | '-' | ' ')) {
        let after_letter = word.chars().last().is_some_and(char::is_alphabetic);
        if (c.is_uppercase() || (c.is_ascii_digit() && after_letter)) && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        word.push(c);
    }
    words.push(word);
    let mut out: Vec<String> = vec![];
    for word in words.into_iter().filter(|w| !w.is_empty()) {
        let lower = word.to_lowercase();
        out.push(match lower.as_str() {
            "hdr" => "HDR".into(),
            "dolby" => "Dolby Vision".into(),
            "ai" | "hdmi" | "oled" => lower.to_uppercase(),
            _ => {
                let mut chars = lower.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            }
        });
    }
    // "dolbyHdrCinema" spells "Dolby Vision HDR Cinema"; LG shows "Dolby Vision Cinema".
    if out.len() > 2 && out[0] == "Dolby Vision" && out[1] == "HDR" {
        out.remove(1);
    }
    out.join(" ")
}

pub fn picture_mode(key: &str) -> String {
    match key {
        "cinema" => "Cinema".into(),
        "hdrCinema" => "HDR Cinema".into(),
        "dolbyHdrCinema" => "Dolby Vision Cinema".into(),
        "game" => "Game".into(),
        "expert1" => "Expert (Bright Room)".into(),
        "expert2" => "Expert (Dark Room)".into(),
        "filmMaker" => "Filmmaker Mode".into(),
        _ => spell(key),
    }
}

pub fn hdr_format(key: &str) -> Option<String> {
    Some(
        match key {
            "none" | "sdr" => "SDR",
            "hdr10" | "hdr" => "HDR10",
            "hlg" => "HLG",
            "dolby_vision" | "dolbyHdr" => "Dolby Vision",
            "dolby_ll" => "Dolby Vision (low latency)",
            "technicolorHdr" => "Technicolor HDR",
            "" => return None,
            other => return Some(spell(other)),
        }
        .into(),
    )
}

fn number(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn flag(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.as_str() {
            "true" | "on" => Some(true),
            "false" | "off" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| !s.is_empty() && s.len() <= 64)
}

fn reading(value: &Value) -> Option<Reading> {
    number(value)
        .filter(|n| (-1000..=1000).contains(n))
        .map(Reading::Number)
        .or_else(|| text(value).map(|t| Reading::Text(spell_value(t))))
}

/// `warm2` -> "Warm 2", `off` stays `off` so the view can translate it.
fn spell_value(raw: &str) -> String {
    if raw == "off" { raw.into() } else { spell(raw) }
}

/// `off`, `low`, `medium` and `high` stay lowercase so the view can translate them.
fn level(value: &Value) -> Option<Reading> {
    match text(value)? {
        raw @ ("off" | "low" | "medium" | "high") => Some(Reading::Text(raw.into())),
        other => Some(Reading::Text(spell(other))),
    }
}

/// LG's gamma keys: the 2.2 curve may carry a user level (`mediumHavingLevel`) and `high2` is
/// BT.1886.
fn gamma(value: &Value) -> Option<Reading> {
    let name = match text(value)? {
        "low" => "1.9",
        "medium" | "mediumHavingLevel" => "2.2",
        "high1" => "2.4",
        "high2" => "BT.1886",
        other => return Some(Reading::Text(spell(other))),
    };
    Some(Reading::Text(name.into()))
}

fn audio_codec(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "pcm" | "lpcm" => "PCM".into(),
        "aac" => "AAC".into(),
        "heaac" => "HE-AAC".into(),
        "ac3" => "Dolby Digital".into(),
        "eac3" => "Dolby Digital Plus".into(),
        "ac4" => "Dolby AC-4".into(),
        "truehd" => "Dolby TrueHD".into(),
        "dts" => "DTS".into(),
        "dtshd" => "DTS-HD".into(),
        "dtsx" => "DTS:X".into(),
        "mpegh" => "MPEG-H".into(),
        "mp3" => "MP3".into(),
        "opus" => "Opus".into(),
        "flac" => "FLAC".into(),
        other => other.to_uppercase(),
    }
}

fn audio_immersive(raw: &str) -> Option<String> {
    let key: String = raw
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '_' | ':' | '-' | ' '))
        .collect();
    match key.as_str() {
        "" | "none" => None,
        "atmos" | "dolbyatmos" => Some("Dolby Atmos".into()),
        "dtsx" => Some("DTS:X".into()),
        "mpegh" => Some("MPEG-H".into()),
        _ => Some(raw.into()),
    }
}

/// `getAudioInputProperty` answers `audioInputProperty: [{codec, immersive}]` while a sound plays.
fn audio(reply: &Value) -> (Option<String>, Option<String>) {
    let Some(property) = reply["audioInputProperty"]
        .as_array()
        .and_then(|list| list.first())
    else {
        return (None, None);
    };
    (
        text(&property["codec"]).map(audio_codec),
        text(&property["immersive"]).and_then(audio_immersive),
    )
}

type Parse = fn(&Value) -> Option<Reading>;

/// The Picture card in display order: the menu's own order first, then the extras.
const PICTURE: [(&str, &str, Parse); 12] = [
    ("backlight", "tvOledLight", reading),
    ("contrast", "tvContrast", reading),
    ("brightness", "tvBlackLevel", reading),
    ("dynamicContrast", "tvDynamicContrast", level),
    ("peakBrightness", "tvPeakBrightness", level),
    ("gamma", "tvGamma", gamma),
    ("sharpness", "tvSharpness", reading),
    ("superResolution", "tvSuperResolution", level),
    ("noiseReduction", "tvNoiseReduction", level),
    ("color", "tvColor", reading),
    ("colorTemperature", "tvColorTemperature", reading),
    ("energySaving", "tvEnergySaving", reading),
];

pub fn normalize(reply: &Value) -> Result<TvStatus, String> {
    if !reply.is_object() {
        return Err("Invalid TV status reply".into());
    }
    let settings = &reply["picture"]["settings"];
    let mut picture = vec![];
    if let Some(mode) = text(&settings["pictureMode"]) {
        picture.push(("tvPictureMode", Reading::Text(picture_mode(mode))));
    }
    for (key, label, parse) in PICTURE {
        if let Some(value) = parse(&settings[key]) {
            picture.push((label, value));
        }
    }
    let sink = reply["video"]["video"].as_array().and_then(|sinks| {
        sinks
            .iter()
            .find(|sink| sink["connected"] == true && sink["videoInfo"].is_object())
    });
    let info = sink.map(|sink| &sink["videoInfo"]);
    let from_info = info
        .and_then(|info| text(&info["hdrType"]))
        .and_then(hdr_format);
    let from_dimension = text(&reply["picture"]["dimension"]["dynamicRange"])
        .map(|range| range.trim_end_matches("ALLM"))
        .and_then(hdr_format);
    let (audio, audio_immersive) = audio(&reply["audio"]);
    let status = TvStatus {
        picture,
        hdr: from_info.or(from_dimension),
        game_mode: info.and_then(|info| flag(&info["isGameMode"])),
        freesync: info.and_then(|info| flag(&info["freesyncEnabled"])),
        audio,
        audio_immersive,
    };
    if status.is_empty() {
        return Err("The TV returned no picture information".into());
    }
    Ok(status)
}

/// Desktop builds never call a TV; this mirrors a typical OLED in Filmmaker Mode.
pub fn demo() -> TvStatus {
    use Reading::{Number, Text};
    TvStatus {
        picture: vec![
            ("tvPictureMode", Text("Filmmaker Mode".into())),
            ("tvOledLight", Number(80)),
            ("tvContrast", Number(85)),
            ("tvBlackLevel", Number(50)),
            ("tvDynamicContrast", Text("off".into())),
            ("tvPeakBrightness", Text("high".into())),
            ("tvGamma", Text("2.2".into())),
            ("tvSharpness", Number(10)),
            ("tvSuperResolution", Text("low".into())),
            ("tvNoiseReduction", Text("off".into())),
            ("tvColor", Number(50)),
            ("tvColorTemperature", Text("Warm 2".into())),
            ("tvEnergySaving", Text("off".into())),
        ],
        hdr: Some("Dolby Vision".into()),
        game_mode: Some(false),
        freesync: Some(false),
        audio: Some("Dolby Digital Plus".into()),
        audio_immersive: Some("Dolby Atmos".into()),
    }
}

/// Schedules read-only section reads: at once when the section opens, then slowly.
#[derive(Default)]
pub struct Poller {
    next: Option<std::time::Instant>,
    in_flight: bool,
}

impl Poller {
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
    const RETRY: std::time::Duration = std::time::Duration::from_secs(30);

    pub fn due(&mut self, now: std::time::Instant, active: bool) -> bool {
        if !active {
            self.next = None;
            return false;
        }
        if self.in_flight || self.next.is_some_and(|next| now < next) {
            return false;
        }
        self.in_flight = true;
        true
    }

    pub fn completed(&mut self, now: std::time::Instant, ok: bool) {
        self.in_flight = false;
        self.next = Some(now + if ok { Self::INTERVAL } else { Self::RETRY });
    }
}

#[cfg(test)]
#[path = "tv-status-tests.rs"]
mod tests;
