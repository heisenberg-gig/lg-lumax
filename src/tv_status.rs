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

/// Some keys (black level) are per input type: `{"hdmi":"low","ntsc":"auto"}`. Use the entry of
/// the current input family, or the shared value when every entry agrees.
fn per_input(value: &Value, input: Option<&str>) -> Value {
    let Some(map) = value.as_object() else {
        return value.clone();
    };
    let family = input.map(|i| i.trim_end_matches(|c: char| c.is_ascii_digit()));
    if let Some(entry) = family.and_then(|f| map.get(f)) {
        return entry.clone();
    }
    let mut values = map.values();
    match values.next() {
        Some(first) if values.all(|v| v == first) => first.clone(),
        _ => Value::Null,
    }
}

const PICTURE: [(&str, &str); 10] = [
    ("backlight", "tvOledLight"),
    ("brightness", "tvBrightness"),
    ("contrast", "tvContrast"),
    ("color", "tvColor"),
    ("sharpness", "tvSharpness"),
    ("blackLevel", "tvBlackLevel"),
    ("colorTemperature", "tvColorTemperature"),
    ("gamma", "tvGamma"),
    ("dynamicContrast", "tvDynamicContrast"),
    ("energySaving", "tvEnergySaving"),
];

pub fn normalize(reply: &Value) -> Result<TvStatus, String> {
    if !reply.is_object() {
        return Err("Invalid TV status reply".into());
    }
    let settings = &reply["picture"]["settings"];
    let input = text(&reply["picture"]["dimension"]["input"]);
    let mut picture = vec![];
    if let Some(mode) = text(&settings["pictureMode"]) {
        picture.push(("tvPictureMode", Reading::Text(picture_mode(mode))));
    }
    for (key, label) in PICTURE {
        if let Some(value) = reading(&per_input(&settings[key], input)) {
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
    let status = TvStatus {
        picture,
        hdr: from_info.or(from_dimension),
        game_mode: info.and_then(|info| flag(&info["isGameMode"])),
        freesync: info.and_then(|info| flag(&info["freesyncEnabled"])),
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
            ("tvBrightness", Number(50)),
            ("tvContrast", Number(85)),
            ("tvColor", Number(50)),
            ("tvSharpness", Number(10)),
            ("tvBlackLevel", Text("Auto".into())),
            ("tvColorTemperature", Text("Warm 2".into())),
            ("tvGamma", Text("Medium".into())),
            ("tvDynamicContrast", Text("off".into())),
            ("tvEnergySaving", Text("off".into())),
        ],
        hdr: Some("Dolby Vision".into()),
        game_mode: Some(false),
        freesync: Some(false),
    }
}

/// Schedules Settings → TV reads: at once when the section opens or on request, then slowly.
#[derive(Default)]
pub struct Poller {
    next: Option<std::time::Instant>,
    in_flight: bool,
}

impl Poller {
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
    const RETRY: std::time::Duration = std::time::Duration::from_secs(30);

    pub fn due(&mut self, now: std::time::Instant, active: bool, forced: bool) -> bool {
        if !active {
            self.next = None;
            return false;
        }
        if self.in_flight || !(forced || self.next.is_none_or(|next| now >= next)) {
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
