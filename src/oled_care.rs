//! Read-only OLED care readings for Settings → OLED Care, parsed from the raw replies of
//! `luma-helper tv-oled`. Pure arithmetic and parsing, so every firmware quirk is testable.
//! The figures and their sources follow native-tv's OLED care tab.
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OledCare {
    pub panel_hours: Option<u64>,
    /// Seconds until the next automatic compensation (the short cycle run in standby).
    pub maintenance_secs: Option<u64>,
    /// Panel hours until the next Pixel Refresher.
    pub refresher_hours: Option<u64>,
    pub maintenance_cycles: Option<u64>,
    pub refresher_cycles: Option<u64>,
    pub failures: Option<u64>,
    pub screen_shift: Option<bool>,
    /// `off`, `low`, `medium` or `high`, or LG's own word when it is another one.
    pub logo_dimming: Option<String>,
    /// Temporal peak control, LG's ASBL.
    pub asbl: Option<bool>,
    /// Global stress reduction.
    pub gsr: Option<bool>,
    pub gsr_stress_events: Option<u64>,
    pub scheduled_cleaning: Option<bool>,
}

impl OledCare {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(leading_number))
        .filter(|n| n.is_finite() && *n >= 0.)
}

/// The leading digits of `"1418\n"` or `"2000.0 h"`.
fn leading_number(text: &str) -> Option<f64> {
    let text = text.trim();
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    text[..end].parse().ok()
}

fn record<'a>(reply: &'a Value, name: &str) -> Option<&'a str> {
    reply["records"][name]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn record_number(reply: &Value, name: &str) -> Option<f64> {
    record(reply, name).and_then(leading_number)
}

fn count(reply: &Value, name: &str) -> Option<u64> {
    record_number(reply, name).map(|n| n as u64)
}

fn on_off(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(on) => Some(*on),
        Value::String(text) => match text.as_str() {
            "on" | "true" => Some(true),
            "off" | "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// LG writes the logo luminance adjustment as off, low, medium/high or weak/strong.
fn logo_dimming(raw: &str) -> String {
    match raw {
        "off" | "low" | "medium" | "high" => raw.into(),
        "weak" => "low".into(),
        "middle" => "medium".into(),
        "strong" => "high".into(),
        other => crate::tv_status::spell(other),
    }
}

pub fn normalize(reply: &Value) -> Result<OledCare, String> {
    if !reply.is_object() {
        return Err("Invalid OLED care reply".into());
    }
    // `panelUsageTime` counts ten-minute units; the compensation record is in whole hours.
    let last_compensation = record_number(reply, "autoOffRsTime")
        .or_else(|| record_number(reply, "autoOffRsLastTime"))
        .map(f64::trunc);
    let units = number(&reply["panel"]["panelUsageTime"])
        .filter(|units| *units > 0.)
        .or_else(|| last_compensation.map(|hours| hours * 6.));
    let hours = units.map(|units| (units / 6.).floor());
    let hours_exact = units.map(|units| units / 6.);

    let compensation_interval = match record_number(reply, "autoOffRsIntervalHomeMode") {
        Some(units) => {
            (if units.trunc() <= 0. {
                24.
            } else {
                units.trunc()
            }) * 10.
                / 60.
        }
        None => record_number(reply, "autoOffRsInterval")
            .filter(|hours| *hours > 0.)
            .unwrap_or(4.),
    };
    let compensation_interval = if (0.5..=24.).contains(&compensation_interval) {
        compensation_interval
    } else {
        4.
    };
    let since_compensation = match (hours_exact, last_compensation) {
        (Some(exact), Some(last)) if last > 0. => (exact - last).max(0.),
        _ => 0.,
    };
    let maintenance_secs = hours_exact
        .map(|_| ((compensation_interval - since_compensation).max(0.) * 3600.).round() as u64);

    let last_refresher = record_number(reply, "autoPnwashTime")
        .or_else(|| record_number(reply, "autoJbLastTime"))
        .map(f64::trunc);
    let refresher_interval = record_number(reply, "autoJbInterval")
        .map(f64::trunc)
        .filter(|hours| *hours > 0.)
        .unwrap_or(2000.);
    let since_refresher = match (hours, last_refresher) {
        (Some(hours), Some(last)) if hours > 0. && last > 0. => (hours - last).max(0.),
        _ => 0.,
    };

    let gsr = &reply["gsr"];
    let care = OledCare {
        panel_hours: hours.map(|hours| hours as u64),
        maintenance_secs,
        refresher_hours: hours.map(|_| (refresher_interval - since_refresher).max(0.) as u64),
        maintenance_cycles: count(reply, "completedOffRsCount"),
        refresher_cycles: count(reply, "completedJbCount"),
        failures: count(reply, "failAlertCount"),
        screen_shift: on_off(&reply["picture"]["settings"]["screenShift"]),
        logo_dimming: reply["picture"]["settings"]["logoLuminanceAdjust"]
            .as_str()
            .filter(|raw| !raw.is_empty() && raw.len() <= 32)
            .map(logo_dimming),
        asbl: on_off(&reply["tpc"]["enable"]),
        gsr: on_off(&gsr["enable"]),
        gsr_stress_events: number(&gsr["stressCount"])
            .or_else(|| record_number(reply, "gsrStressCount"))
            .map(|count| count as u64),
        scheduled_cleaning: on_off(&reply["picture"]["settings"]["scheduledPixelCleaning"]),
    };
    if care.is_empty() {
        return Err("The TV returned no OLED information".into());
    }
    Ok(care)
}

/// Desktop builds never call a TV; typical figures for an OLED a year old.
pub fn demo() -> OledCare {
    OledCare {
        panel_hours: Some(1419),
        maintenance_secs: Some(2 * 3600 + 20 * 60),
        refresher_hours: Some(1830),
        maintenance_cycles: Some(112),
        refresher_cycles: Some(1),
        failures: Some(0),
        screen_shift: Some(true),
        logo_dimming: Some("high".into()),
        asbl: Some(true),
        gsr: Some(true),
        gsr_stress_events: Some(4),
        scheduled_cleaning: Some(false),
    }
}

#[cfg(test)]
#[path = "oled-care-tests.rs"]
mod tests;
