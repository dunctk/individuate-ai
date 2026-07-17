use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use time::{macros::format_description, Date, Duration, OffsetDateTime};

const ISO_DATE: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]");

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BodyOnboardingPreference {
    pub completed: bool,
    pub identity: Option<String>,
    pub period_tracking_choice: Option<String>,
}

impl BodyOnboardingPreference {
    pub fn validate(&self) -> Result<()> {
        if self.identity.as_ref().is_some_and(|identity| {
            !matches!(
                identity.as_str(),
                "male" | "female" | "other" | "prefer_not_to_say"
            )
        }) {
            return Err(anyhow!("Unsupported identity choice"));
        }
        if self.period_tracking_choice.as_ref().is_some_and(|choice| {
            !matches!(choice.as_str(), "accepted" | "declined" | "not_offered")
        }) {
            return Err(anyhow!("Unsupported period tracking choice"));
        }
        if self.completed && self.identity.is_none() {
            return Err(anyhow!("Choose an option or prefer not to say"));
        }
        if self.completed
            && matches!(self.identity.as_deref(), Some("female" | "other"))
            && !matches!(
                self.period_tracking_choice.as_deref(),
                Some("accepted" | "declined")
            )
        {
            return Err(anyhow!("Choose whether to set up period tracking"));
        }
        if self.completed
            && matches!(self.identity.as_deref(), Some("male" | "prefer_not_to_say"))
            && self.period_tracking_choice.as_deref() != Some("not_offered")
        {
            return Err(anyhow!("Period tracking was not offered for this choice"));
        }
        Ok(())
    }

    pub fn identity_label(&self) -> &'static str {
        match self.identity.as_deref() {
            Some("male") => "Male",
            Some("female") => "Female",
            Some("other") => "Other",
            Some("prefer_not_to_say") => "Prefer not to say",
            _ => "Not answered",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CycleProfile {
    pub enabled: bool,
    pub ai_context_enabled: bool,
    pub show_in_chat: bool,
    pub tracking_mode: String,
    pub timezone: String,
    pub utc_offset_minutes: i32,
    pub typical_cycle_days: Option<i64>,
    pub paused: bool,
}

impl Default for CycleProfile {
    fn default() -> Self {
        Self {
            enabled: false,
            ai_context_enabled: true,
            show_in_chat: true,
            tracking_mode: "natural".to_string(),
            timezone: "local".to_string(),
            utc_offset_minutes: 0,
            typical_cycle_days: None,
            paused: false,
        }
    }
}

impl CycleProfile {
    pub fn validate(&self) -> Result<()> {
        if !matches!(
            self.tracking_mode.as_str(),
            "natural"
                | "hormonal"
                | "irregular"
                | "postpartum"
                | "perimenopause"
                | "pregnant"
                | "unknown"
        ) {
            return Err(anyhow!("Unsupported tracking mode"));
        }
        if self.timezone.trim().is_empty() || self.timezone.len() > 80 {
            return Err(anyhow!("Invalid timezone"));
        }
        if !(-840..=840).contains(&self.utc_offset_minutes) {
            return Err(anyhow!("Invalid timezone offset"));
        }
        if self
            .typical_cycle_days
            .is_some_and(|days| !(15..=90).contains(&days))
        {
            return Err(anyhow!(
                "Typical cycle length must be between 15 and 90 days"
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BodyCheckIn {
    pub mood: Option<u8>,
    pub energy: Option<u8>,
    pub sensitivity: Option<u8>,
    pub anxiety: Option<u8>,
    pub sleep: Option<u8>,
    pub pain: Option<u8>,
}

impl BodyCheckIn {
    pub fn validate(&self) -> Result<()> {
        for value in [
            self.mood,
            self.energy,
            self.sensitivity,
            self.anxiety,
            self.sleep,
            self.pain,
        ]
        .into_iter()
        .flatten()
        {
            if !(1..=5).contains(&value) {
                return Err(anyhow!("Check-in values must be between 1 and 5"));
            }
        }
        Ok(())
    }

    fn metric(&self, key: &str) -> Option<f64> {
        match key {
            "mood" => self.mood,
            "energy" => self.energy,
            "sensitivity" => self.sensitivity,
            "anxiety" => self.anxiety,
            "sleep" => self.sleep,
            "pain" => self.pain,
            _ => None,
        }
        .map(f64::from)
    }

    fn context_summary(&self) -> String {
        [
            ("mood", self.mood),
            ("energy", self.energy),
            ("sensitivity", self.sensitivity),
            ("anxiety", self.anxiety),
            ("sleep quality", self.sleep),
            ("physical discomfort", self.pain),
        ]
        .into_iter()
        .filter_map(|(label, value)| value.map(|value| format!("{label} {value}/5")))
        .collect::<Vec<_>>()
        .join(", ")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CycleEvent {
    pub id: String,
    pub kind: String,
    pub local_date: String,
    pub source: String,
    pub flow: Option<String>,
    pub note: Option<String>,
    pub check_in: Option<BodyCheckIn>,
    pub created_at: Option<String>,
}

impl Default for CycleEvent {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: "check_in".to_string(),
            local_date: String::new(),
            source: "manual".to_string(),
            flow: None,
            note: None,
            check_in: None,
            created_at: None,
        }
    }
}

impl CycleEvent {
    pub fn validate(&self) -> Result<()> {
        if !matches!(
            self.kind.as_str(),
            "bleeding_started" | "bleeding_ended" | "check_in" | "ovulation_test"
        ) {
            return Err(anyhow!("Unsupported cycle event"));
        }
        parse_date(&self.local_date)?;
        if !matches!(self.source.as_str(), "manual" | "chat_confirmed" | "import") {
            return Err(anyhow!("Unsupported event source"));
        }
        if self.note.as_ref().is_some_and(|note| note.len() > 500) {
            return Err(anyhow!("Notes must be 500 characters or fewer"));
        }
        if self.kind == "check_in" {
            self.check_in
                .as_ref()
                .ok_or_else(|| anyhow!("A check-in is required"))?
                .validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CycleInsight {
    pub id: String,
    pub text: String,
    pub status: String,
    pub evidence_cycles: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CyclePrediction {
    pub cycle_day: Option<i64>,
    pub state_label: String,
    pub state_detail: String,
    pub confidence: String,
    pub next_start: Option<String>,
    pub next_start_earliest: Option<String>,
    pub next_start_latest: Option<String>,
    pub typical_cycle_days: Option<i64>,
    pub completed_cycles: usize,
    pub observed_start_dates: Vec<String>,
    pub today: String,
    pub timeline_days: usize,
    pub observed_bleeding_days: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CycleDashboard {
    pub profile: CycleProfile,
    pub prediction: CyclePrediction,
    pub events: Vec<CycleEvent>,
    pub insights: Vec<CycleInsight>,
    pub recent_check_in: Option<CycleEvent>,
}

pub fn parse_date(value: &str) -> Result<Date> {
    Date::parse(value, ISO_DATE).map_err(|_| anyhow!("Date must use YYYY-MM-DD"))
}

pub fn format_date(value: Date) -> String {
    value.format(ISO_DATE).unwrap_or_default()
}

pub fn today_utc() -> Date {
    OffsetDateTime::now_utc().date()
}

pub fn today_for_profile(profile: &CycleProfile) -> Date {
    (OffsetDateTime::now_utc() + Duration::minutes(i64::from(profile.utc_offset_minutes))).date()
}

fn period_starts(events: &[CycleEvent]) -> Vec<Date> {
    let mut dates = events
        .iter()
        .filter(|event| event.kind == "bleeding_started")
        .filter_map(|event| parse_date(&event.local_date).ok())
        .collect::<Vec<_>>();
    dates.sort_unstable();
    dates.dedup();
    dates
}

fn median(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]).div_euclid(2)
    } else {
        sorted[middle]
    })
}

pub fn calculate_prediction(
    profile: &CycleProfile,
    events: &[CycleEvent],
    today: Date,
) -> CyclePrediction {
    let starts = period_starts(events);
    let observed_start_dates = starts.iter().map(|date| format_date(*date)).collect();
    let recent_starts = starts.iter().rev().take(7).copied().collect::<Vec<_>>();
    let mut lengths = recent_starts
        .windows(2)
        .filter_map(|pair| {
            let length = (pair[0] - pair[1]).whole_days();
            (15..=90).contains(&length).then_some(length)
        })
        .collect::<Vec<_>>();
    lengths.reverse();
    let computed_length = median(&lengths);
    let typical = computed_length.or(profile.typical_cycle_days);
    let variation = typical
        .and_then(|center| {
            median(
                &lengths
                    .iter()
                    .map(|length| (length - center).abs())
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or(3)
        .max(2);
    let last_start = starts.last().copied();
    let last_end = last_start.and_then(|start| {
        events
            .iter()
            .filter(|event| event.kind == "bleeding_ended")
            .filter_map(|event| parse_date(&event.local_date).ok())
            .filter(|end| *end >= start)
            .min()
    });
    let observed_bleeding_days = last_start
        .map(|start| {
            last_end
                .map(|end| (end - start).whole_days() + 1)
                .unwrap_or(1)
                .clamp(1, 12) as usize
        })
        .unwrap_or(0);
    let cycle_day = last_start
        .map(|start| (today - start).whole_days() + 1)
        .filter(|day| *day >= 1 && *day <= 120);
    let next = last_start
        .zip(typical)
        .map(|(start, days)| start + Duration::days(days));
    let earliest = next.map(|date| date - Duration::days(variation + 1));
    let latest = next.map(|date| date + Duration::days(variation + 1));
    let natural_prediction = profile.tracking_mode == "natural" && !profile.paused;
    let (state_label, state_detail) = if !profile.enabled {
        (
            "Cycle tracking is off".to_string(),
            "Enable it when you want this context.".to_string(),
        )
    } else if profile.paused {
        (
            "Tracking paused".to_string(),
            "History is retained, but predictions and AI context are off.".to_string(),
        )
    } else if profile.tracking_mode != "natural" {
        (
            "Tracking symptoms".to_string(),
            "No ovarian phase is inferred in this tracking mode.".to_string(),
        )
    } else if last_start.is_some_and(|start| {
        today == start || last_end.is_some_and(|end| today >= start && today <= end)
    }) {
        (
            "Recorded bleeding window".to_string(),
            "Based on your latest recorded start date.".to_string(),
        )
    } else if let (Some(next), true) = (next, natural_prediction) {
        let days_until = (next - today).whole_days();
        if (0..=14).contains(&days_until) {
            (
                "Estimated late-cycle window".to_string(),
                "A calendar estimate, not a hormonal measurement.".to_string(),
            )
        } else {
            (
                "Estimated earlier-cycle window".to_string(),
                "A calendar estimate, not a hormonal measurement.".to_string(),
            )
        }
    } else {
        (
            "Building your timeline".to_string(),
            "Add another period start to improve the estimate.".to_string(),
        )
    };
    let confidence = if !natural_prediction || next.is_none() {
        "none"
    } else if lengths.len() >= 3 && variation <= 3 {
        "higher"
    } else if !lengths.is_empty() {
        "moderate"
    } else {
        "low"
    }
    .to_string();

    CyclePrediction {
        cycle_day,
        state_label,
        state_detail,
        confidence,
        next_start: next.map(format_date),
        next_start_earliest: earliest.map(format_date),
        next_start_latest: latest.map(format_date),
        typical_cycle_days: typical,
        completed_cycles: lengths.len(),
        observed_start_dates,
        today: format_date(today),
        timeline_days: typical.unwrap_or(28).clamp(21, 40) as usize,
        observed_bleeding_days,
    }
}

pub fn derive_insights(events: &[CycleEvent], stored: &[CycleInsight]) -> Vec<CycleInsight> {
    let starts = period_starts(events);
    if starts.len() < 4 {
        return Vec::new();
    }
    let status_by_id = stored
        .iter()
        .map(|insight| (insight.id.as_str(), insight.status.as_str()))
        .collect::<BTreeMap<_, _>>();
    let metrics = [
        ("mood", "lower mood", true),
        ("energy", "lower energy", true),
        ("sensitivity", "more sensitivity", false),
        ("anxiety", "more anxiety", false),
        ("sleep", "poorer sleep", true),
        ("pain", "more physical discomfort", false),
    ];
    let mut insights = Vec::new();
    for (key, label, lower_is_more) in metrics {
        let mut pre = Vec::new();
        let mut baseline = Vec::new();
        let mut pre_cycles = BTreeSet::new();
        for event in events.iter().filter(|event| event.kind == "check_in") {
            let Some(value) = event.check_in.as_ref().and_then(|check| check.metric(key)) else {
                continue;
            };
            let Ok(date) = parse_date(&event.local_date) else {
                continue;
            };
            let Some((cycle_index, next_start)) =
                starts.iter().enumerate().find(|(_, start)| **start > date)
            else {
                continue;
            };
            let days_before = (*next_start - date).whole_days();
            if (1..=5).contains(&days_before) {
                pre.push(value);
                pre_cycles.insert(cycle_index);
            } else if (6..=20).contains(&days_before) {
                baseline.push(value);
            }
        }
        if pre.len() < 3 || baseline.len() < 3 || pre_cycles.len() < 3 {
            continue;
        }
        let pre_avg = pre.iter().sum::<f64>() / pre.len() as f64;
        let baseline_avg = baseline.iter().sum::<f64>() / baseline.len() as f64;
        let delta = if lower_is_more {
            baseline_avg - pre_avg
        } else {
            pre_avg - baseline_avg
        };
        if delta < 0.75 {
            continue;
        }
        let id = format!("pre_period_{key}");
        insights.push(CycleInsight {
            status: status_by_id
                .get(id.as_str())
                .copied()
                .unwrap_or("proposed")
                .to_string(),
            id,
            text: format!(
                "{} appeared more often in the five days before bleeding across {} tracked cycles.",
                label,
                pre_cycles.len()
            ),
            evidence_cycles: pre_cycles.len(),
        });
    }
    insights
}

pub fn build_dashboard(
    profile: CycleProfile,
    events: Vec<CycleEvent>,
    stored_insights: Vec<CycleInsight>,
    today: Date,
) -> CycleDashboard {
    let prediction = calculate_prediction(&profile, &events, today);
    let insights = derive_insights(&events, &stored_insights);
    let recent_check_in = events
        .iter()
        .filter(|event| event.kind == "check_in")
        .max_by(|left, right| left.local_date.cmp(&right.local_date))
        .cloned();
    CycleDashboard {
        profile,
        prediction,
        events,
        insights,
        recent_check_in,
    }
}

pub fn format_body_context(dashboard: &CycleDashboard) -> String {
    let profile = &dashboard.profile;
    if !profile.enabled || profile.paused || !profile.ai_context_enabled {
        return String::new();
    }
    let accepted = dashboard
        .insights
        .iter()
        .filter(|insight| insight.status == "accepted")
        .map(|insight| format!("- {}", insight.text))
        .collect::<Vec<_>>();
    let recent = dashboard
        .recent_check_in
        .as_ref()
        .map(|event| {
            let values = event
                .check_in
                .as_ref()
                .map(BodyCheckIn::context_summary)
                .unwrap_or_default();
            format!(
                "Most recent optional check-in ({}): {}.",
                event.local_date, values
            )
        })
        .unwrap_or_else(|| "No recent check-in.".to_string());
    format!(
        "<body_context>\nCycle/body tracking was explicitly enabled by the user.\nCurrent state: {}.\nCycle day: {}.\nConfidence: {}.\nPossible next period range: {} to {}.\n{}\nAccepted within-person patterns:\n{}\nTreat this only as possible context. Never use it to dismiss the user's perception, claim a hormonal cause, infer ovulation, diagnose PMS/PMDD, or mention the cycle unless it is relevant or the user asks. Prefer the user's present report and expose uncertainty.\n</body_context>",
        dashboard.prediction.state_label,
        dashboard
            .prediction
            .cycle_day
            .map(|day| day.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        dashboard.prediction.confidence,
        dashboard.prediction.next_start_earliest.as_deref().unwrap_or("unknown"),
        dashboard.prediction.next_start_latest.as_deref().unwrap_or("unknown"),
        recent,
        if accepted.is_empty() { "none".to_string() } else { accepted.join("\n") },
    )
}

pub fn chat_period_start_suggestion(message: &str, local_today: Date) -> Option<String> {
    let lower = message.to_lowercase();
    let mentioned = [
        "my period started",
        "my period began",
        "started my period",
        "got my period",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if !mentioned {
        return None;
    }
    let date = if lower.contains("yesterday") {
        local_today - Duration::days(1)
    } else {
        local_today
    };
    Some(format_date(date))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, date: &str) -> CycleEvent {
        CycleEvent {
            id: format!("{kind}-{date}"),
            kind: kind.to_string(),
            local_date: date.to_string(),
            source: "manual".to_string(),
            ..CycleEvent::default()
        }
    }

    #[test]
    fn prediction_rebuilds_from_observed_starts_and_exposes_range() {
        let events = vec![
            event("bleeding_started", "2026-04-01"),
            event("bleeding_started", "2026-04-29"),
            event("bleeding_started", "2026-05-28"),
            event("bleeding_started", "2026-06-25"),
        ];
        let prediction = calculate_prediction(
            &CycleProfile {
                enabled: true,
                ..CycleProfile::default()
            },
            &events,
            parse_date("2026-07-17").unwrap(),
        );
        assert_eq!(prediction.cycle_day, Some(23));
        assert_eq!(prediction.typical_cycle_days, Some(28));
        assert_eq!(prediction.next_start.as_deref(), Some("2026-07-23"));
        assert_eq!(prediction.confidence, "higher");
        assert!(prediction.next_start_earliest.is_some());
        assert!(prediction.next_start_latest.is_some());
    }

    #[test]
    fn non_natural_modes_never_claim_a_phase() {
        let profile = CycleProfile {
            enabled: true,
            tracking_mode: "hormonal".to_string(),
            ..CycleProfile::default()
        };
        let prediction = calculate_prediction(
            &profile,
            &[event("bleeding_started", "2026-07-01")],
            parse_date("2026-07-17").unwrap(),
        );
        assert_eq!(prediction.state_label, "Tracking symptoms");
        assert_eq!(prediction.confidence, "none");
    }

    #[test]
    fn chat_detection_suggests_but_does_not_create_an_event() {
        assert_eq!(
            chat_period_start_suggestion(
                "My period started yesterday",
                parse_date("2026-07-17").unwrap()
            )
            .as_deref(),
            Some("2026-07-16")
        );
        assert!(chat_period_start_suggestion("I feel tired", today_utc()).is_none());
    }

    #[test]
    fn onboarding_requires_a_period_choice_only_when_it_is_offered() {
        assert!(BodyOnboardingPreference {
            completed: true,
            identity: Some("male".to_string()),
            period_tracking_choice: Some("not_offered".to_string()),
        }
        .validate()
        .is_ok());
        assert!(BodyOnboardingPreference {
            completed: true,
            identity: Some("male".to_string()),
            period_tracking_choice: Some("accepted".to_string()),
        }
        .validate()
        .is_err());
        assert!(BodyOnboardingPreference {
            completed: true,
            identity: Some("other".to_string()),
            period_tracking_choice: None,
        }
        .validate()
        .is_err());
        assert!(BodyOnboardingPreference {
            completed: true,
            identity: Some("female".to_string()),
            period_tracking_choice: Some("accepted".to_string()),
        }
        .validate()
        .is_ok());
    }
}
