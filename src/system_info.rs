//! Read-only system readings for Settings → System, parsed from the raw text the TV helper
//! collects (`luma-helper tv-system`). Pure parsing, so every firmware quirk is testable.
//! The readings and their sources follow native-tv's Metrics menu.
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemInfo {
    pub temperature_c: Option<u32>,
    pub cpu_load: Option<u32>,
    pub cpu_mhz: Option<u32>,
    pub gpu_mhz: Option<u32>,
    pub core_loads: Vec<u32>,
    pub memory_free_kb: Option<u64>,
    pub memory_total_kb: Option<u64>,
    pub swap_free_kb: Option<u64>,
    pub swap_total_kb: Option<u64>,
    pub storage_free_kb: Option<u64>,
    pub storage_total_kb: Option<u64>,
    pub storage_used_percent: Option<u32>,
    pub emmc_health: Option<String>,
    pub model: Option<String>,
    pub firmware: Option<String>,
    pub webos: Option<String>,
    pub soc: Option<String>,
    pub ram: Option<String>,
    pub panel_refresh_hz: Option<u32>,
    pub wifi: Option<String>,
}

impl SystemInfo {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn text(raw: &Value) -> Option<&str> {
    raw.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn unsigned(token: &str) -> Option<u64> {
    token.trim().parse().ok()
}

fn clean(value: &Value) -> Option<String> {
    text(value)
        .filter(|s| s.len() <= 64)
        .map(|s| s.chars().filter(|c| !c.is_control()).collect())
}

fn meminfo(raw: &Value, name: &str) -> Option<u64> {
    text(raw)?.lines().find_map(|line| {
        let rest = line.strip_prefix(name)?.strip_prefix(':')?;
        unsigned(rest.trim().trim_end_matches("kB").trim())
    })
}

fn temperature(raw: &Value) -> Option<u32> {
    let value = unsigned(text(&raw["value"])?)?;
    let millidegrees = if raw["lg"] == true {
        (value > 0 && value <= 1000).then_some(value * 1000)?
    } else {
        (value > 0 && value <= 1_000_000).then_some(value)?
    };
    Some(((millidegrees as f64) / 1000.).round() as u32)
}

fn frequency_mhz(raw: &Value) -> Option<u32> {
    let value = unsigned(text(raw)?)? as f64;
    let hz = value * if value > 10_000. { 1000. } else { 1_000_000. };
    (100e6..=10_000e6)
        .contains(&hz)
        .then(|| (hz / 1e6).round() as u32)
}

fn gpu_mhz(raw: &Value) -> Option<u32> {
    let line = text(raw)?
        .lines()
        .map(str::to_lowercase)
        .find(|line| line.contains("gpu pll out"))?;
    let after = &line[line.find("gpu pll out")? + 11..];
    let hz = unsigned(after.split_once(':')?.1.split_whitespace().next()?)?;
    Some((hz as f64 / 1e6).round() as u32)
}

fn cpu_ids(raw: &Value) -> Vec<usize> {
    let mut ids = vec![];
    for part in text(raw).unwrap_or("").split(',') {
        let mut bounds = part.trim().splitn(2, '-');
        let first = bounds.next().and_then(|v| v.parse::<usize>().ok());
        let last = bounds.next().map_or(first, |v| v.parse().ok());
        if let (Some(first), Some(last)) = (first, last)
            && first <= last
            && last < 4096
        {
            ids.extend(first..=last);
        }
    }
    ids
}

/// LG's instantaneous per-core load slots (`load: 3 10 0 7`), mapped to the online cores.
fn core_loads(status: &Value, online: &Value) -> Vec<u32> {
    let Some(status) = text(status) else {
        return vec![];
    };
    let mut slots: Vec<u64> = vec![];
    let mut cores = None;
    for line in status.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        match name.trim() {
            "load" => {
                slots = rest
                    .split_whitespace()
                    .map_while(|token| token.parse().ok())
                    .collect();
            }
            "cpu_num" => cores = rest.trim().parse::<usize>().ok().filter(|n| *n <= 4096),
            _ => {}
        }
    }
    let mut ids = cpu_ids(online);
    if ids.is_empty() {
        ids = (0..cores.unwrap_or(0)).collect();
    }
    let loads: Vec<u32> = if ids.is_empty() {
        slots.iter().map(|v| (*v).min(100) as u32).collect()
    } else {
        ids.iter()
            .filter_map(|id| slots.get(*id))
            .map(|v| (*v).min(100) as u32)
            .collect()
    };
    loads
}

fn emmc_health(raw: &Value) -> Option<String> {
    let health = text(raw)?
        .split_whitespace()
        .take(2)
        .filter_map(|token| u32::from_str_radix(token.trim_start_matches("0x"), 16).ok())
        .filter(|value| (1..=11).contains(value))
        .map(|value| if value == 11 { 0 } else { 100 - value * 10 })
        .min()?;
    Some(if health >= 90 {
        ">90% (Healthy)".into()
    } else {
        format!("{health}% remaining")
    })
}

/// `df -k` output: the last row ends with total, used, available, percentage and mount point.
fn storage(raw: &Value) -> Option<(u64, u64, u32)> {
    let tokens: Vec<&str> = text(raw)?.lines().last()?.split_whitespace().collect();
    let [.., total, used, available, _percent, _mount] = tokens.as_slice() else {
        return None;
    };
    let (total, used, available) = (unsigned(total)?, unsigned(used)?, unsigned(available)?);
    (total > 0 && used <= total && available <= total).then(|| {
        (
            total,
            available,
            ((used as f64 / total as f64) * 100.).round() as u32,
        )
    })
}

fn soc_name(raw: &str) -> Option<String> {
    let key = raw.trim_matches('_').trim().to_uppercase();
    Some(
        match key.as_str() {
            "" => return None,
            "O22" => "Alpha 9 Gen 5 (O22)",
            "O20" => "Alpha 9 Gen 3 (O20)",
            "O18" => "Alpha 9 Gen 1 (O18)",
            "M16P" | "M16PLUS" => "Alpha 7 (M16P)",
            other => return Some(other.chars().take(32).collect()),
        }
        .into(),
    )
}

fn digits_after(text: &str, marker: &str) -> Option<String> {
    let rest = text[text.find(marker)? + marker.len()..].trim_start();
    let version: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    (!version.is_empty()).then_some(version)
}

fn webos(reply: &Value) -> Option<String> {
    let issue = text(&reply["issue"]).unwrap_or("");
    digits_after(issue, "webOS TV")
        .or_else(|| digits_after(issue, "webOS"))
        .or_else(|| digits_after(text(&reply["release"]).unwrap_or(""), "release"))
        .or_else(|| clean(&reply["properties"]["sdkVersion"]))
}

pub fn normalize(reply: &Value) -> Result<SystemInfo, String> {
    if !reply.is_object() {
        return Err("Invalid system reply".into());
    }
    let environment: Value = text(&reply["environment"])
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(Value::Null);
    let soc = ["boardTypeStr", "socChip"]
        .iter()
        .find_map(|key| text(&environment[*key]))
        .or_else(|| text(&reply["chip_name"]))
        .and_then(soc_name);
    let loads = core_loads(&reply["pm_status"], &reply["cpu_online"]);
    let storage = storage(&reply["storage"]);
    let total = meminfo(&reply["meminfo"], "MemTotal").filter(|v| *v > 0);
    let available = meminfo(&reply["meminfo"], "MemAvailable");
    let swap_total = meminfo(&reply["meminfo"], "SwapTotal").filter(|v| *v > 0);
    let info = SystemInfo {
        temperature_c: temperature(&reply["temperature"]),
        cpu_load: (!loads.is_empty())
            .then(|| (loads.iter().sum::<u32>() as f64 / loads.len() as f64).round() as u32),
        cpu_mhz: frequency_mhz(&reply["frequency"]),
        gpu_mhz: gpu_mhz(&reply["gpu_status"]),
        core_loads: loads,
        memory_total_kb: total,
        memory_free_kb: total.and(available).filter(|a| Some(*a) <= total),
        swap_total_kb: swap_total,
        swap_free_kb: swap_total
            .and(meminfo(&reply["meminfo"], "SwapFree"))
            .filter(|f| Some(*f) <= swap_total),
        storage_total_kb: storage.map(|s| s.0),
        storage_free_kb: storage.map(|s| s.1),
        storage_used_percent: storage.map(|s| s.2),
        emmc_health: emmc_health(&reply["emmc_life_time"]),
        model: clean(&reply["properties"]["modelName"]),
        firmware: clean(&reply["properties"]["firmwareVersion"]),
        webos: webos(reply),
        soc,
        ram: clean(&environment["ddrSize"]),
        panel_refresh_hz: environment["panelOutputFrameRate"]
            .as_u64()
            .or_else(|| text(&environment["panelOutputFrameRate"]).and_then(unsigned))
            .filter(|hz| (1..=1000).contains(hz))
            .map(|hz| hz as u32),
        wifi: clean(&reply["connection"]["wifi"]["ssid"]),
    };
    if info.is_empty() {
        return Err("The TV returned no system information".into());
    }
    Ok(info)
}

/// "1.2 GB" / "845 MB" from kilobytes.
pub fn size(kilobytes: u64) -> String {
    let megabytes = kilobytes as f64 / 1024.;
    if megabytes >= 1024. {
        format!("{:.1} GB", megabytes / 1024.)
    } else {
        format!("{} MB", megabytes.round() as u64)
    }
}

/// Desktop builds never call a TV; this mirrors a typical OLED.
pub fn demo() -> SystemInfo {
    SystemInfo {
        temperature_c: Some(46),
        cpu_load: Some(12),
        cpu_mhz: Some(1500),
        gpu_mhz: Some(800),
        core_loads: vec![14, 10, 18, 6],
        memory_free_kb: Some(1_245_184),
        memory_total_kb: Some(2_897_920),
        swap_free_kb: Some(786_432),
        swap_total_kb: Some(1_048_576),
        storage_free_kb: Some(2_516_582),
        storage_total_kb: Some(3_145_728),
        storage_used_percent: Some(20),
        emmc_health: Some(">90% (Healthy)".into()),
        model: Some("OLED55C54LA".into()),
        firmware: Some("33.22.45".into()),
        webos: Some("10.2.0".into()),
        soc: Some("Alpha 9 Gen 7 (O24)".into()),
        ram: Some("3GB".into()),
        panel_refresh_hz: Some(120),
        wifi: Some("Home Wi-Fi".into()),
    }
}

#[cfg(test)]
#[path = "system-info-tests.rs"]
mod tests;
