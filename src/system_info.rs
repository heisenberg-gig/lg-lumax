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
    pub remote_battery: Option<u32>,
    /// Seconds since the TV last powered on (any start: cold boot, standby, remote, Wake-on-WiFi).
    pub session_secs: Option<u64>,
    pub network: Network,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    Wifi,
    Wired,
}

/// The active connection; fields the TV does not report stay `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Network {
    pub link: Option<Link>,
    pub connected: Option<bool>,
    pub internet: Option<bool>,
    pub ssid: Option<String>,
    pub ip: Option<String>,
    pub netmask: Option<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub signal_quality: Option<u32>,
    pub signal_dbm: Option<i32>,
    pub interface: Option<String>,
    pub mac: Option<String>,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
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

fn address(value: &Value) -> Option<String> {
    text(value)?
        .parse::<std::net::Ipv4Addr>()
        .ok()
        .map(|ip| ip.to_string())
}

fn mac(value: &Value) -> Option<String> {
    let mac = text(value)?.to_lowercase();
    let valid = mac.len() == 17
        && mac.bytes().enumerate().all(|(i, b)| {
            if i % 3 == 2 {
                b == b':'
            } else {
                b.is_ascii_hexdigit()
            }
        })
        && mac != "00:00:00:00:00:00";
    valid.then_some(mac)
}

/// `/proc/net/dev` row of one interface: (received bytes, transmitted bytes).
fn traffic(raw: &Value, interface: &str) -> Option<(u64, u64)> {
    text(raw)?.lines().find_map(|line| {
        let (name, rest) = line.split_once(':')?;
        if name.trim() != interface {
            return None;
        }
        let values: Vec<&str> = rest.split_whitespace().collect();
        Some((unsigned(values.first()?)?, unsigned(values.get(8)?)?))
    })
}

/// `/proc/net/wireless` row: (link quality, signal in dBm). A driver with no reading reports 0 0.
fn wireless(raw: &Value, interface: &str) -> Option<(u32, i32)> {
    text(raw)?.lines().find_map(|line| {
        let (name, rest) = line.split_once(':')?;
        if name.trim() != interface {
            return None;
        }
        let mut fields = rest.split_whitespace().skip(1);
        let quality: f64 = fields.next()?.trim_end_matches('.').parse().ok()?;
        let level: f64 = fields.next()?.trim_end_matches('.').parse().ok()?;
        if quality == 0. && level == 0. {
            return None;
        }
        let dbm = if level > 127. { level - 256. } else { level };
        ((0. ..=1000.).contains(&quality) && (-130. ..=0.).contains(&dbm))
            .then_some((quality as u32, dbm as i32))
    })
}

fn network(reply: &Value) -> Network {
    let connection = &reply["connection"];
    let state = |side: &str| text(&connection[side]["state"]).map(|s| s == "connected");
    let link = if state("wired") == Some(true) {
        Some(Link::Wired)
    } else if connection["wifi"].is_object() {
        Some(Link::Wifi)
    } else if connection["wired"].is_object() {
        Some(Link::Wired)
    } else {
        None
    };
    let (side, default_interface) = match link {
        Some(Link::Wired) => ("wired", "eth0"),
        _ => ("wifi", "wlan0"),
    };
    let entry = &connection[side];
    let interface = clean(&entry["interfaceName"])
        .filter(|name| {
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
        })
        .unwrap_or_else(|| default_interface.into());
    let wifi_signal = (side == "wifi")
        .then(|| wireless(&reply["wireless"], &interface))
        .flatten();
    let traffic = traffic(&reply["net_dev"], &interface);
    let dns = ["dns1", "dns2"]
        .iter()
        .filter_map(|key| address(&entry[*key]))
        .collect();
    Network {
        link,
        connected: state(side),
        internet: connection["isInternetConnectionAvailable"].as_bool(),
        ssid: (side == "wifi").then(|| clean(&entry["ssid"])).flatten(),
        ip: address(&entry["ipAddress"]),
        netmask: address(&entry["netmask"]),
        gateway: address(&entry["gateway"]),
        dns,
        signal_quality: wifi_signal.map(|s| s.0),
        signal_dbm: wifi_signal.map(|s| s.1),
        mac: mac(&reply["mac"][&interface]),
        interface: Some(interface),
        rx_bytes: traffic.map(|t| t.0),
        tx_bytes: traffic.map(|t| t.1),
    }
    .or_empty()
}

impl Network {
    /// A reply with no connection data at all reports nothing, not a bare interface name.
    fn or_empty(self) -> Self {
        if self.link.is_none() && self.ip.is_none() && self.rx_bytes.is_none() {
            Self::default()
        } else {
            self
        }
    }
}

fn remote_battery(reply: &Value) -> Option<u32> {
    // The TV writes `63.0` in its saved file and `HIGH` instead of a number while the remote sleeps.
    let level = |value: &Value| {
        whole_number(value)
            .filter(|level| *level <= 100)
            .map(|level| level as u32)
    };
    level(&reply["remote"]["batteryLevel"])
        .or_else(|| {
            // native-tv's saved last level, when it is installed.
            let saved: Value = serde_json::from_str(text(&reply["remote_saved"])?).ok()?;
            level(&saved["level"])
        })
        .or_else(|| {
            // The pairing file's `battery=` line; newer webOS writes a threshold such as `20 (low)`.
            text(&reply["remote_pairing"])?.lines().find_map(|line| {
                let (key, value) = line.split_once('=')?;
                if !key.trim().eq_ignore_ascii_case("battery") {
                    return None;
                }
                let value = value.trim();
                let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
                let rest = value[digits.len()..].trim_start();
                (!digits.is_empty() && !rest.starts_with('('))
                    .then(|| level(&Value::String(digits)))
                    .flatten()
            })
        })
}

/// `power/getPowerOnTime` answers `time.ontime` as a string of seconds.
fn session_secs(reply: &Value) -> Option<u64> {
    whole_number(&reply["power"]["time"]["ontime"])
}

/// A number the TV may write as `5`, `"5"`, `5.0` or `"377.08"`; fractions round down.
fn whole_number(value: &Value) -> Option<u64> {
    let number = value
        .as_f64()
        .or_else(|| text(value).and_then(|token| token.parse().ok()))?;
    (number.is_finite() && number >= 0.).then_some(number as u64)
}

/// "2h 14m", "14m", "45s": the session timer's compact form.
pub fn duration(seconds: u64) -> String {
    match (seconds / 3600, seconds / 60 % 60) {
        (0, 0) => format!("{seconds}s"),
        (0, minutes) => format!("{minutes}m"),
        (hours, minutes) => format!("{hours}h {minutes:02}m"),
    }
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
        remote_battery: remote_battery(reply),
        session_secs: session_secs(reply),
        network: network(reply),
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
        remote_battery: Some(80),
        session_secs: Some(8_040),
        network: Network {
            link: Some(Link::Wifi),
            connected: Some(true),
            internet: Some(true),
            ssid: Some("Home Wi-Fi".into()),
            ip: Some("192.168.1.85".into()),
            netmask: Some("255.255.255.0".into()),
            gateway: Some("192.168.1.1".into()),
            dns: vec!["192.168.1.1".into(), "1.1.1.1".into()],
            signal_quality: Some(62),
            signal_dbm: Some(-48),
            interface: Some("wlan0".into()),
            mac: Some("00:00:5e:00:53:01".into()),
            rx_bytes: Some(5_972_719_000),
            tx_bytes: Some(691_308_700),
        },
    }
}

#[cfg(test)]
#[path = "system-info-tests.rs"]
mod tests;
