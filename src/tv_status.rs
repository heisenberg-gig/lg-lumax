//! Read-only TV status for the Settings "TV" section. The helper returns raw service replies;
//! everything here is pure parsing so it can be tested without a TV. Names follow LG's own
//! terms (picture modes, HDR formats) and are not translated.
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TvStatus {
    pub picture_mode: Option<String>,
    pub oled_light: Option<u32>,
    /// Raw LG key such as `off`, `min`, `auto`.
    pub energy_saving: Option<String>,
    pub hdr: Option<String>,
    pub game_mode: Option<bool>,
    pub freesync: Option<bool>,
    pub temperature_c: Option<u32>,
    pub memory_used_mb: Option<u32>,
    pub memory_total_mb: Option<u32>,
    pub panel_hours: Option<u32>,
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
        if c.is_uppercase() && !word.is_empty() {
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

fn number(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.) as u64)),
        Value::String(s) => {
            let digits: String = s.trim().chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        }
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

fn temperature(raw: &Value) -> Option<u32> {
    let value = number(&Value::String(raw.as_str()?.to_owned()))?;
    // /proc/lg/pm/temperature reports degrees; the thermal zone fallback reports millidegrees.
    let degrees = if value > 250 { value / 1000 } else { value };
    (degrees <= 250).then_some(degrees as u32)
}

fn memory(raw: &Value) -> Option<(u32, u32)> {
    let meminfo = raw.as_str()?;
    let field = |name: &str| {
        meminfo.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix(':')?;
            number(&Value::String(rest.trim().to_owned()))
        })
    };
    let total = field("MemTotal")?;
    let free = field("MemAvailable").or_else(|| field("MemFree"))?;
    (total > 0 && free <= total).then(|| (((total - free) / 1024) as u32, (total / 1024) as u32))
}

pub fn normalize(reply: &Value) -> Result<TvStatus, String> {
    if !reply.is_object() {
        return Err("Invalid TV status reply".into());
    }
    let settings = &reply["picture"]["settings"];
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
    let (memory_used_mb, memory_total_mb) = memory(&reply["meminfo"]).unzip();
    let status = TvStatus {
        picture_mode: text(&settings["pictureMode"]).map(picture_mode),
        oled_light: number(&settings["backlight"])
            .filter(|v| *v <= 100)
            .map(|v| v as u32),
        energy_saving: text(&settings["energySaving"]).map(str::to_owned),
        hdr: from_info.or(from_dimension),
        game_mode: info.and_then(|info| flag(&info["isGameMode"])),
        freesync: info.and_then(|info| flag(&info["freesyncEnabled"])),
        temperature_c: temperature(&reply["temperature"]),
        memory_used_mb,
        memory_total_mb,
        panel_hours: number(&reply["panel"]["panelUsageTime"])
            .filter(|units| *units < 6_000_000)
            .map(|units| (units / 6) as u32),
    };
    if status.is_empty() {
        return Err("The TV returned no status information".into());
    }
    Ok(status)
}

/// Desktop builds never call a TV; this mirrors a typical OLED in game mode.
pub fn demo() -> TvStatus {
    TvStatus {
        picture_mode: Some("Filmmaker Mode".into()),
        oled_light: Some(80),
        energy_saving: Some("off".into()),
        hdr: Some("Dolby Vision".into()),
        game_mode: Some(false),
        freesync: Some(false),
        temperature_c: Some(46),
        memory_used_mb: Some(1210),
        memory_total_mb: Some(2830),
        panel_hours: Some(1284),
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
