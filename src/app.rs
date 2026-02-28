use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::model::{DateYmd, LoanInput, LoanMetrics, RateOverride, calculate_metrics};

const TEXT_FIELD_COUNT: usize = 7;
const FIELD_COUNT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    LoanAmount,
    OneTimeFees,
    MonthlyFees,
    InterestRate,
    TermYears,
    StartDate,
    PaymentDay,
    RoundPaymentsUp,
}

impl FieldId {
    pub const ALL: [FieldId; FIELD_COUNT] = [
        FieldId::LoanAmount,
        FieldId::OneTimeFees,
        FieldId::MonthlyFees,
        FieldId::InterestRate,
        FieldId::TermYears,
        FieldId::StartDate,
        FieldId::PaymentDay,
        FieldId::RoundPaymentsUp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FieldId::LoanAmount => "Loan Amount",
            FieldId::OneTimeFees => "One-time Fees",
            FieldId::MonthlyFees => "Monthly Fees",
            FieldId::InterestRate => "Base Interest Rate (% APR)",
            FieldId::TermYears => "Term (years)",
            FieldId::StartDate => "Start Date (YYYY-MM-DD)",
            FieldId::PaymentDay => "Payment Day (1-31)",
            FieldId::RoundPaymentsUp => "Round Payment To Nearest Integer",
        }
    }

    pub fn is_integer(self) -> bool {
        matches!(self, FieldId::TermYears | FieldId::PaymentDay)
    }

    pub fn is_date(self) -> bool {
        matches!(self, FieldId::StartDate)
    }

    pub fn is_text_input(self) -> bool {
        !matches!(self, FieldId::RoundPaymentsUp)
    }

    pub fn index(self) -> usize {
        match self {
            FieldId::LoanAmount => 0,
            FieldId::OneTimeFees => 1,
            FieldId::MonthlyFees => 2,
            FieldId::InterestRate => 3,
            FieldId::TermYears => 4,
            FieldId::StartDate => 5,
            FieldId::PaymentDay => 6,
            FieldId::RoundPaymentsUp => {
                unreachable!("checkbox field does not map to text input")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    Inputs,
    Schedule,
}

#[derive(Debug, Clone)]
pub struct App {
    pub inputs: [String; TEXT_FIELD_COUNT],
    pub metrics: Option<LoanMetrics>,
    pub error: Option<String>,
    pub is_row_rate_popup_open: bool,
    pub row_rate_input_buffer: String,
    pub selected_month: u32,
    pub round_payments_up: bool,
    pub focus_area: FocusArea,
    pub schedule_selected_index: usize,
    pub schedule_scroll_offset: usize,
    active_field_idx: usize,
    schedule_viewport_rows: usize,
    rate_overrides: BTreeMap<u32, f64>,
}

impl Default for App {
    fn default() -> Self {
        let mut app = Self {
            inputs: default_inputs(),
            metrics: None,
            error: None,
            is_row_rate_popup_open: false,
            row_rate_input_buffer: String::new(),
            selected_month: 1,
            round_payments_up: false,
            focus_area: FocusArea::Inputs,
            schedule_selected_index: 0,
            schedule_scroll_offset: 0,
            active_field_idx: 0,
            schedule_viewport_rows: 1,
            rate_overrides: BTreeMap::new(),
        };
        app.recalculate();
        app
    }
}

impl App {
    pub fn active_field(&self) -> FieldId {
        FieldId::ALL[self.active_field_idx]
    }

    pub fn field_value(&self, field: FieldId) -> &str {
        assert!(field.is_text_input(), "checkbox field has no string input");
        &self.inputs[field.index()]
    }

    pub fn field_display_value(&self, field: FieldId) -> String {
        if field == FieldId::RoundPaymentsUp {
            if self.round_payments_up {
                "[x]".to_string()
            } else {
                "[ ]".to_string()
            }
        } else {
            self.field_value(field).to_string()
        }
    }

    pub fn is_schedule_focused(&self) -> bool {
        self.focus_area == FocusArea::Schedule
    }

    pub fn focus_inputs(&mut self) {
        self.focus_area = FocusArea::Inputs;
    }

    pub fn focus_schedule(&mut self) {
        self.focus_area = FocusArea::Schedule;
        self.clamp_schedule_selection();
        self.ensure_schedule_selection_visible();
    }

    pub fn toggle_focus_area(&mut self) {
        if self.is_schedule_focused() {
            self.focus_inputs();
        } else {
            self.focus_schedule();
        }
    }

    pub fn next_field(&mut self) {
        self.active_field_idx = (self.active_field_idx + 1) % FIELD_COUNT;
    }

    pub fn previous_field(&mut self) {
        self.active_field_idx = if self.active_field_idx == 0 {
            FIELD_COUNT - 1
        } else {
            self.active_field_idx - 1
        };
    }

    pub fn navigate_up(&mut self) {
        if self.is_schedule_focused() {
            self.move_schedule_selection(-1);
        } else {
            self.previous_field();
        }
    }

    pub fn navigate_down(&mut self) {
        if self.is_schedule_focused() {
            self.move_schedule_selection(1);
        } else {
            self.next_field();
        }
    }

    pub fn move_schedule_selection_by_page(&mut self, delta_pages: i32) {
        let step = self.schedule_viewport_rows.max(1) as i32;
        self.move_schedule_selection(delta_pages * step);
    }

    pub fn move_schedule_selection_to_start(&mut self) {
        let row_count = self.selectable_month_count();
        if row_count == 0 || self.schedule_selected_index == 0 {
            return;
        }

        self.schedule_selected_index = 0;
        self.sync_selected_month_from_selection();
        self.ensure_schedule_selection_visible();
        self.recalculate();
    }

    pub fn move_schedule_selection_to_end(&mut self) {
        let row_count = self.selectable_month_count();
        if row_count == 0 {
            return;
        }

        let last_index = row_count - 1;
        if self.schedule_selected_index == last_index {
            return;
        }

        self.schedule_selected_index = last_index;
        self.sync_selected_month_from_selection();
        self.ensure_schedule_selection_visible();
        self.recalculate();
    }

    pub fn move_schedule_selection(&mut self, delta: i32) {
        let row_count = self.selectable_month_count();
        if row_count == 0 {
            self.schedule_selected_index = 0;
            self.selected_month = 1;
            self.schedule_scroll_offset = 0;
            return;
        }

        let max_index = row_count.saturating_sub(1) as i32;
        let next = (self.schedule_selected_index as i32 + delta).clamp(0, max_index) as usize;
        if next == self.schedule_selected_index {
            return;
        }

        self.schedule_selected_index = next;
        self.sync_selected_month_from_selection();
        self.ensure_schedule_selection_visible();
        self.recalculate();
    }

    pub fn set_schedule_viewport_rows(&mut self, rows: usize) {
        self.schedule_viewport_rows = rows.max(1);
        self.ensure_schedule_selection_visible();
    }

    pub fn open_row_rate_popup(&mut self) {
        self.clamp_schedule_selection();
        self.sync_selected_month_from_selection();
        self.sync_row_rate_input_buffer();
        self.is_row_rate_popup_open = true;
    }

    pub fn close_row_rate_popup(&mut self) {
        self.is_row_rate_popup_open = false;
    }

    pub fn format_schedule_month(&self, month_index: u32) -> String {
        if month_index == 0 {
            return "---- --".to_string();
        }

        let Some(start_date) = try_parse_date(self.field_value(FieldId::StartDate)) else {
            return "---- --".to_string();
        };

        let (year, month) = add_months(start_date.year, start_date.month, month_index as i32);
        format!("{year:04}-{month:02}")
    }

    pub fn input_char(&mut self, c: char) {
        let active = self.active_field();
        if !active.is_text_input() {
            return;
        }

        if active.is_date() {
            if c.is_ascii_digit() || c == '-' {
                self.inputs[active.index()].push(c);
            }
            return;
        }

        if active.is_integer() {
            if c.is_ascii_digit() {
                self.inputs[active.index()].push(c);
            }
            return;
        }

        if !c.is_ascii_digit() && c != '.' {
            return;
        }

        if c == '.' {
            let value = &self.inputs[active.index()];
            if value.contains('.') {
                return;
            }

            if value.is_empty() {
                self.inputs[active.index()].push('0');
            }
        }

        self.inputs[active.index()].push(c);
    }

    pub fn backspace(&mut self) {
        if !self.active_field().is_text_input() {
            return;
        }

        self.inputs[self.active_field().index()].pop();
    }

    pub fn toggle_round_payments_up(&mut self) {
        self.round_payments_up = !self.round_payments_up;
        self.recalculate();
    }

    pub fn reset(&mut self) {
        self.inputs = default_inputs();
        self.active_field_idx = 0;
        self.error = None;
        self.is_row_rate_popup_open = false;
        self.row_rate_input_buffer.clear();
        self.selected_month = 1;
        self.round_payments_up = false;
        self.focus_area = FocusArea::Inputs;
        self.schedule_selected_index = 0;
        self.schedule_scroll_offset = 0;
        self.rate_overrides.clear();
        self.recalculate();
    }

    pub fn recalculate(&mut self) {
        self.normalize_rate_state();

        match self.build_input() {
            Ok(input) => match calculate_metrics(&input, self.selected_month) {
                Ok(metrics) => {
                    self.metrics = Some(metrics);
                    self.error = None;
                }
                Err(err) => {
                    self.metrics = None;
                    self.error = Some(err.to_string());
                }
            },
            Err(err) => {
                self.metrics = None;
                self.error = Some(err);
            }
        }

        self.clamp_schedule_selection();
        self.sync_selected_month_from_selection();
        self.ensure_schedule_selection_visible();
    }

    pub fn row_rate_input_char(&mut self, c: char) {
        if !c.is_ascii_digit() && c != '.' {
            return;
        }

        if c == '.' {
            if self.row_rate_input_buffer.contains('.') {
                return;
            }

            if self.row_rate_input_buffer.is_empty() {
                self.row_rate_input_buffer.push('0');
            }
        }

        self.row_rate_input_buffer.push(c);
    }

    pub fn row_rate_input_backspace(&mut self) {
        self.row_rate_input_buffer.pop();
    }

    pub fn apply_row_rate_override_at_selected_month(&mut self) {
        let trimmed = self.row_rate_input_buffer.trim();
        if trimmed.is_empty() {
            self.error = Some("Rate override APR is required".to_string());
            return;
        }

        match trimmed.parse::<f64>() {
            Ok(parsed) if parsed.is_finite() && parsed >= 0.0 => {
                self.rate_overrides.insert(self.selected_month, parsed);
                self.error = None;
                self.sync_row_rate_input_buffer();
                self.is_row_rate_popup_open = false;
                self.recalculate();
            }
            _ => {
                self.error = Some("Rate override APR must be a non-negative number".to_string());
            }
        }
    }

    pub fn clear_row_rate_override_at_selected_month(&mut self) {
        self.rate_overrides.remove(&self.selected_month);
        self.error = None;
        self.sync_row_rate_input_buffer();
        self.is_row_rate_popup_open = false;
        self.recalculate();
    }

    pub fn override_count(&self) -> usize {
        self.rate_overrides.len()
    }

    pub fn override_for_month(&self, month: u32) -> Option<f64> {
        self.rate_overrides.get(&month).copied()
    }

    pub fn effective_rate_for_month(&self, month: u32) -> Option<f64> {
        if month == 0 {
            return None;
        }

        let mut effective = parse_f64(
            FieldId::InterestRate,
            self.field_value(FieldId::InterestRate),
        )
        .ok()?;

        for (_, override_rate) in self.rate_overrides.range(..=month) {
            effective = *override_rate;
        }

        Some(effective)
    }

    fn normalize_rate_state(&mut self) {
        if let Some(max_month) = self.term_months_from_input() {
            self.rate_overrides.retain(|month, _| *month <= max_month);
            let max_index = max_month.saturating_sub(1) as usize;
            self.schedule_selected_index = self.schedule_selected_index.min(max_index);
        } else {
            self.clamp_schedule_selection();
        }

        self.sync_selected_month_from_selection();
    }

    fn selectable_month_count(&self) -> usize {
        if let Some(months) = self.term_months_from_input() {
            return months as usize;
        }

        self.metrics
            .as_ref()
            .map(|metrics| metrics.repayment_schedule.len().max(1))
            .unwrap_or(1)
    }

    fn clamp_schedule_selection(&mut self) {
        let row_count = self.selectable_month_count();
        if row_count == 0 {
            self.schedule_selected_index = 0;
        } else {
            self.schedule_selected_index = self.schedule_selected_index.min(row_count - 1);
        }
    }

    fn ensure_schedule_selection_visible(&mut self) {
        let row_count = self.selectable_month_count();
        if row_count == 0 {
            self.schedule_scroll_offset = 0;
            return;
        }

        let max_offset = row_count.saturating_sub(1);
        self.schedule_scroll_offset = self.schedule_scroll_offset.min(max_offset);

        if self.schedule_selected_index < self.schedule_scroll_offset {
            self.schedule_scroll_offset = self.schedule_selected_index;
        } else {
            let visible_end = self
                .schedule_scroll_offset
                .saturating_add(self.schedule_viewport_rows.max(1).saturating_sub(1));
            if self.schedule_selected_index > visible_end {
                self.schedule_scroll_offset = self
                    .schedule_selected_index
                    .saturating_add(1)
                    .saturating_sub(self.schedule_viewport_rows.max(1));
            }
        }

        self.schedule_scroll_offset = self.schedule_scroll_offset.min(max_offset);
    }

    fn sync_selected_month_from_selection(&mut self) {
        self.selected_month = self.schedule_selected_index.saturating_add(1) as u32;
    }

    fn sync_row_rate_input_buffer(&mut self) {
        if let Some(rate) = self.rate_overrides.get(&self.selected_month).copied() {
            self.row_rate_input_buffer = format_rate_for_input(rate);
        } else {
            self.row_rate_input_buffer.clear();
        }
    }

    fn term_months_from_input(&self) -> Option<u32> {
        let years = parse_u32(FieldId::TermYears, self.field_value(FieldId::TermYears)).ok()?;
        if years == 0 {
            return None;
        }

        Some(years.saturating_mul(12))
    }

    fn build_input(&self) -> Result<LoanInput, String> {
        let loan_amount = parse_f64(FieldId::LoanAmount, self.field_value(FieldId::LoanAmount))?;
        let one_time_fees =
            parse_f64(FieldId::OneTimeFees, self.field_value(FieldId::OneTimeFees))?;
        let monthly_fees = parse_f64(FieldId::MonthlyFees, self.field_value(FieldId::MonthlyFees))?;
        let base_annual_interest_rate_pct = parse_f64(
            FieldId::InterestRate,
            self.field_value(FieldId::InterestRate),
        )?;
        let term_years = parse_u32(FieldId::TermYears, self.field_value(FieldId::TermYears))?;
        let start_date = parse_date(FieldId::StartDate, self.field_value(FieldId::StartDate))?;
        let payment_day = parse_u32(FieldId::PaymentDay, self.field_value(FieldId::PaymentDay))?;

        if payment_day == 0 || payment_day > 31 {
            return Err("Payment Day must be between 1 and 31".to_string());
        }

        let rate_overrides = self
            .rate_overrides
            .iter()
            .map(|(start_month, annual_interest_rate_pct)| RateOverride {
                start_month: *start_month,
                annual_interest_rate_pct: *annual_interest_rate_pct,
            })
            .collect();

        Ok(LoanInput {
            loan_amount,
            one_time_fees,
            monthly_fees,
            round_monthly_payment_up: self.round_payments_up,
            base_annual_interest_rate_pct,
            term_years,
            start_date,
            payment_day,
            rate_overrides,
        })
    }
}

fn default_inputs() -> [String; TEXT_FIELD_COUNT] {
    let (year, month, day) = current_utc_year_month_day();

    [
        "300000".to_string(),
        "8000".to_string(),
        "120".to_string(),
        "6.0".to_string(),
        "30".to_string(),
        format!("{year:04}-{month:02}-{day:02}"),
        day.to_string(),
    ]
}

fn parse_f64(field: FieldId, value: &str) -> Result<f64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{} is required", field.label()));
    }

    trimmed
        .parse::<f64>()
        .map_err(|_| format!("{} must be a number", field.label()))
}

fn parse_u32(field: FieldId, value: &str) -> Result<u32, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{} is required", field.label()));
    }

    trimmed
        .parse::<u32>()
        .map_err(|_| format!("{} must be a whole number", field.label()))
}

fn parse_date(field: FieldId, value: &str) -> Result<DateYmd, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{} is required", field.label()));
    }

    DateYmd::parse_yyyy_mm_dd(trimmed)
        .ok_or_else(|| format!("{} must be in YYYY-MM-DD format", field.label()))
}

fn try_parse_date(value: &str) -> Option<DateYmd> {
    DateYmd::parse_yyyy_mm_dd(value.trim())
}

fn format_rate_for_input(value: f64) -> String {
    let mut formatted = format!("{value:.6}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }

    if formatted.ends_with('.') {
        formatted.pop();
    }

    if formatted.is_empty() {
        return "0".to_string();
    }

    formatted
}

fn current_utc_year_month_day() -> (i32, u32, u32) {
    let secs_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    let days_since_epoch = secs_since_epoch.div_euclid(86_400);
    civil_from_days(days_since_epoch)
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }

    (year as i32, month as u32, day as u32)
}

fn add_months(year: i32, month: u32, delta_months: i32) -> (i32, u32) {
    let total_months = year * 12 + (month as i32 - 1) + delta_months;
    let new_year = total_months.div_euclid(12);
    let new_month = total_months.rem_euclid(12) + 1;
    (new_year, new_month as u32)
}

#[cfg(test)]
mod tests {
    use super::App;

    #[test]
    fn term_reduction_prunes_overrides_and_clamps_selected_month() {
        let mut app = App::default();
        app.rate_overrides.insert(120, 7.0);
        app.rate_overrides.insert(300, 8.0);
        app.schedule_selected_index = 239;
        app.inputs[4] = "10".to_string();

        app.recalculate();

        assert_eq!(app.schedule_selected_index, 119);
        assert_eq!(app.selected_month, 120);
        assert!(app.rate_overrides.contains_key(&120));
        assert!(!app.rate_overrides.contains_key(&300));
    }

    #[test]
    fn row_popup_apply_and_clear_selected_month_override() {
        let mut app = App::default();
        app.focus_schedule();
        app.move_schedule_selection(14);

        app.open_row_rate_popup();
        app.row_rate_input_buffer = "7.25".to_string();
        app.apply_row_rate_override_at_selected_month();

        assert_eq!(app.selected_month, 15);
        assert_eq!(app.override_for_month(15), Some(7.25));
        assert!(!app.is_row_rate_popup_open);

        app.open_row_rate_popup();
        app.clear_row_rate_override_at_selected_month();
        assert_eq!(app.override_for_month(15), None);
        assert!(!app.is_row_rate_popup_open);
    }

    #[test]
    fn schedule_selection_moves_and_scroll_follows_viewport() {
        let mut app = App::default();
        app.focus_schedule();
        app.set_schedule_viewport_rows(5);

        app.move_schedule_selection(6);
        assert_eq!(app.schedule_selected_index, 6);
        assert_eq!(app.schedule_scroll_offset, 2);
        assert_eq!(app.selected_month, 7);

        app.move_schedule_selection(-5);
        assert_eq!(app.schedule_selected_index, 1);
        assert_eq!(app.schedule_scroll_offset, 1);

        app.move_schedule_selection(-50);
        assert_eq!(app.schedule_selected_index, 0);
        assert_eq!(app.schedule_scroll_offset, 0);

        app.move_schedule_selection_to_end();
        assert_eq!(app.schedule_selected_index, 359);
        assert_eq!(app.selected_month, 360);
        assert_eq!(app.schedule_scroll_offset, 355);

        app.move_schedule_selection_to_start();
        assert_eq!(app.schedule_selected_index, 0);
        assert_eq!(app.selected_month, 1);
        assert_eq!(app.schedule_scroll_offset, 0);
    }

    #[test]
    fn selected_schedule_row_drives_summary_selected_month() {
        let mut app = App::default();
        app.focus_schedule();
        app.move_schedule_selection(11);

        let metrics = app
            .metrics
            .as_ref()
            .expect("metrics should exist after moving selection");
        assert_eq!(app.selected_month, 12);
        assert_eq!(metrics.selected_month, 12);
    }

    #[test]
    fn invalid_start_date_sets_error() {
        let mut app = App::default();
        app.inputs[5] = "2026-13-99".to_string();

        app.recalculate();

        assert_eq!(
            app.error.as_deref(),
            Some("Start Date (YYYY-MM-DD) must be in YYYY-MM-DD format")
        );
    }

    #[test]
    fn invalid_payment_day_sets_error() {
        let mut app = App::default();
        app.inputs[6] = "0".to_string();

        app.recalculate();

        assert_eq!(
            app.error.as_deref(),
            Some("Payment Day must be between 1 and 31")
        );
    }

    #[test]
    fn toggling_round_payments_checkbox_changes_flag() {
        let mut app = App::default();
        assert!(!app.round_payments_up);

        app.toggle_round_payments_up();
        assert!(app.round_payments_up);

        app.toggle_round_payments_up();
        assert!(!app.round_payments_up);
    }
}
