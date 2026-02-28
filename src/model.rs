use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateYmd {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl DateYmd {
    pub fn from_ymd_opt(year: i32, month: u32, day: u32) -> Option<Self> {
        if !(1..=12).contains(&month) {
            return None;
        }

        let last_day = last_day_of_month(year, month);
        if day == 0 || day > last_day {
            return None;
        }

        Some(Self { year, month, day })
    }

    pub fn parse_yyyy_mm_dd(value: &str) -> Option<Self> {
        if value.len() != 10 {
            return None;
        }

        let bytes = value.as_bytes();
        if bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }

        let year = value[0..4].parse::<i32>().ok()?;
        let month = value[5..7].parse::<u32>().ok()?;
        let day = value[8..10].parse::<u32>().ok()?;

        Self::from_ymd_opt(year, month, day)
    }

    pub fn format_yyyy_mm_dd(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn format_yyyy_mm(self) -> String {
        format!("{:04}-{:02}", self.year, self.month)
    }

    pub fn days_since_epoch(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }
}

impl fmt::Display for DateYmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateOverride {
    pub effective_date: DateYmd,
    pub annual_interest_rate_pct: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaymentSegment {
    pub start_month: u32,
    pub end_month: u32,
    pub annual_interest_rate_pct: f64,
    pub monthly_payment_base: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepaymentScheduleEntry {
    pub month_index: u32,
    pub payment_date: DateYmd,
    pub effective_annual_interest_rate_pct: f64,
    pub total_payment: f64,
    pub interest_payment: f64,
    pub principal_payment: f64,
    pub fees_payment: f64,
}

#[derive(Debug, Clone)]
pub struct LoanInput {
    pub loan_amount: f64,
    pub one_time_fees: f64,
    pub monthly_fees: f64,
    pub round_monthly_payment_up: bool,
    pub base_annual_interest_rate_pct: f64,
    pub term_years: u32,
    pub start_date: DateYmd,
    pub payment_day: u32,
    pub rate_overrides: Vec<RateOverride>,
}

#[derive(Debug, Clone)]
pub struct LoanMetrics {
    pub first_monthly_payment_base: f64,
    pub selected_month: u32,
    pub selected_monthly_payment_base: f64,
    pub selected_monthly_payment_with_fees: f64,
    pub selected_month_effective_rate_pct: f64,
    pub next_change_month: Option<u32>,
    pub next_change_monthly_payment_base: Option<f64>,
    pub total_interest: f64,
    pub total_monthly_fees: f64,
    pub total_repayment: f64,
    pub total_paid_all_in: f64,
    pub loan_cost: f64,
    pub purchase_price_estimate: f64,
    pub down_payment_ratio_pct: f64,
    pub segments: Vec<PaymentSegment>,
    pub repayment_schedule: Vec<RepaymentScheduleEntry>,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum CalcError {
    #[error("{0} must be a finite number greater than or equal to 0")]
    InvalidNonNegativeField(&'static str),
    #[error("loan_amount must be greater than 0")]
    LoanAmountMustBePositive,
    #[error("term_years must be greater than 0")]
    TermYearsMustBePositive,
    #[error("payment_day must be in range 1..=31")]
    InvalidPaymentDay,
    #[error("rate override date {date} is out of range ({min_date}..={max_date})")]
    InvalidOverrideDate {
        date: DateYmd,
        min_date: DateYmd,
        max_date: DateYmd,
    },
    #[error("duplicate rate override for date {0}")]
    DuplicateOverrideDate(DateYmd),
    #[error("rate override APR for date {date} must be a finite number >= 0")]
    InvalidOverrideRate { date: DateYmd },
    #[error("selected month {month} is out of range (1..={max_month})")]
    InvalidSelectedMonth { month: u32, max_month: u32 },
}

pub fn calculate_metrics(input: &LoanInput, selected_month: u32) -> Result<LoanMetrics, CalcError> {
    validate_non_negative("one_time_fees", input.one_time_fees)?;
    validate_non_negative("monthly_fees", input.monthly_fees)?;
    validate_non_negative(
        "base_annual_interest_rate_pct",
        input.base_annual_interest_rate_pct,
    )?;

    if !input.loan_amount.is_finite() || input.loan_amount <= 0.0 {
        return Err(CalcError::LoanAmountMustBePositive);
    }

    if input.term_years == 0 {
        return Err(CalcError::TermYearsMustBePositive);
    }

    if input.payment_day == 0 || input.payment_day > 31 {
        return Err(CalcError::InvalidPaymentDay);
    }

    let total_months = input.term_years.saturating_mul(12);
    if selected_month == 0 || selected_month > total_months {
        return Err(CalcError::InvalidSelectedMonth {
            month: selected_month,
            max_month: total_months,
        });
    }

    let payment_dates = build_payment_dates(input.start_date, input.payment_day, total_months);
    let last_payment_date = *payment_dates
        .last()
        .expect("term has at least one month and therefore at least one payment date");
    let start_month_anchor_day = input.payment_day.min(last_day_of_month(
        input.start_date.year,
        input.start_date.month,
    ));
    let start_month_anchor = DateYmd::from_ymd_opt(
        input.start_date.year,
        input.start_date.month,
        start_month_anchor_day,
    )
    .expect("start-month anchor date must be valid");
    let rate_overrides =
        normalize_rate_overrides(&input.rate_overrides, input.start_date, last_payment_date)?;

    let mut remaining_principal = input.loan_amount;
    let mut segments = Vec::with_capacity(rate_overrides.len() + 1);
    let mut repayment_schedule = Vec::with_capacity(total_months as usize);
    let mut current_segment_start_month = 1;
    let mut current_segment_rate = rate_at_date(
        payment_dates[0],
        input.base_annual_interest_rate_pct,
        &rate_overrides,
    );
    let mut current_monthly_payment_base =
        compute_monthly_payment(remaining_principal, current_segment_rate, total_months);

    for month in 1..=total_months {
        let payment_date = payment_dates[(month - 1) as usize];
        let regular_period_start = if month == 1 {
            start_month_anchor
        } else {
            payment_dates[(month - 2) as usize]
        };

        let payment_rate = rate_at_date(
            payment_date,
            input.base_annual_interest_rate_pct,
            &rate_overrides,
        );
        if month > 1 && (payment_rate - current_segment_rate).abs() > 1e-12 {
            segments.push(PaymentSegment {
                start_month: current_segment_start_month,
                end_month: month - 1,
                annual_interest_rate_pct: current_segment_rate,
                monthly_payment_base: current_monthly_payment_base,
            });

            current_segment_start_month = month;
            current_segment_rate = payment_rate;
            let remaining_term_months = total_months - month + 1;
            current_monthly_payment_base = compute_monthly_payment(
                remaining_principal,
                current_segment_rate,
                remaining_term_months,
            );
        }

        let interest_payment = accrue_interest_for_period_daily(
            remaining_principal,
            regular_period_start,
            payment_date,
            input.base_annual_interest_rate_pct,
            &rate_overrides,
        );
        let arrears_interest_signed = if month == 1 {
            accrue_interest_for_period_daily_signed(
                remaining_principal,
                input.start_date,
                start_month_anchor,
                input.base_annual_interest_rate_pct,
                &rate_overrides,
            )
        } else {
            0.0
        };
        let total_interest_payment = interest_payment + arrears_interest_signed;
        let mut principal_payment = current_monthly_payment_base - interest_payment;

        if month == total_months || principal_payment > remaining_principal {
            principal_payment = remaining_principal;
        }

        remaining_principal -= principal_payment;
        if remaining_principal.abs() < 1e-8 {
            remaining_principal = 0.0;
        }

        let fees_payment = input.monthly_fees;
        let (interest_payment_for_schedule, principal_payment_for_schedule) =
            if input.round_monthly_payment_up {
                (
                    round_half_up(total_interest_payment),
                    round_half_up(principal_payment),
                )
            } else {
                (total_interest_payment, principal_payment)
            };
        let total_payment =
            interest_payment_for_schedule + principal_payment_for_schedule + fees_payment;

        repayment_schedule.push(RepaymentScheduleEntry {
            month_index: month,
            payment_date,
            effective_annual_interest_rate_pct: payment_rate,
            total_payment,
            interest_payment: interest_payment_for_schedule,
            principal_payment: principal_payment_for_schedule,
            fees_payment,
        });
    }

    segments.push(PaymentSegment {
        start_month: current_segment_start_month,
        end_month: total_months,
        annual_interest_rate_pct: current_segment_rate,
        monthly_payment_base: current_monthly_payment_base,
    });

    let selected_segment = segments
        .iter()
        .find(|segment| {
            selected_month >= segment.start_month && selected_month <= segment.end_month
        })
        .expect("selected month is always in at least one segment");
    let selected_schedule_entry = repayment_schedule
        .iter()
        .find(|entry| entry.month_index == selected_month)
        .expect("selected month is always in repayment schedule");

    let next_segment = segments
        .iter()
        .find(|segment| segment.start_month > selected_month);

    let total_interest: f64 = repayment_schedule
        .iter()
        .map(|entry| entry.interest_payment)
        .sum();
    let total_monthly_fees: f64 = repayment_schedule
        .iter()
        .map(|entry| entry.fees_payment)
        .sum();
    let total_repayment: f64 = repayment_schedule
        .iter()
        .map(|entry| entry.total_payment)
        .sum();
    let total_paid_all_in = total_repayment + input.one_time_fees;
    let loan_cost = total_paid_all_in - input.loan_amount;
    let purchase_price_estimate = input.loan_amount;
    let down_payment_ratio_pct = 0.0;

    Ok(LoanMetrics {
        first_monthly_payment_base: segments[0].monthly_payment_base,
        selected_month,
        selected_monthly_payment_base: selected_segment.monthly_payment_base,
        selected_monthly_payment_with_fees: selected_schedule_entry.total_payment,
        selected_month_effective_rate_pct: selected_segment.annual_interest_rate_pct,
        next_change_month: next_segment.map(|segment| segment.start_month),
        next_change_monthly_payment_base: next_segment.map(|segment| segment.monthly_payment_base),
        total_interest,
        total_monthly_fees,
        total_repayment,
        total_paid_all_in,
        loan_cost,
        purchase_price_estimate,
        down_payment_ratio_pct,
        segments,
        repayment_schedule,
    })
}

fn normalize_rate_overrides(
    rate_overrides: &[RateOverride],
    start_date: DateYmd,
    last_payment_date: DateYmd,
) -> Result<BTreeMap<DateYmd, f64>, CalcError> {
    let mut overrides = BTreeMap::new();

    for rate_override in rate_overrides {
        if rate_override.effective_date < start_date
            || rate_override.effective_date > last_payment_date
        {
            return Err(CalcError::InvalidOverrideDate {
                date: rate_override.effective_date,
                min_date: start_date,
                max_date: last_payment_date,
            });
        }

        if !rate_override.annual_interest_rate_pct.is_finite()
            || rate_override.annual_interest_rate_pct < 0.0
        {
            return Err(CalcError::InvalidOverrideRate {
                date: rate_override.effective_date,
            });
        }

        if overrides
            .insert(
                rate_override.effective_date,
                rate_override.annual_interest_rate_pct,
            )
            .is_some()
        {
            return Err(CalcError::DuplicateOverrideDate(
                rate_override.effective_date,
            ));
        }
    }

    Ok(overrides)
}

fn rate_at_date(
    date: DateYmd,
    base_annual_interest_rate_pct: f64,
    rate_overrides: &BTreeMap<DateYmd, f64>,
) -> f64 {
    let mut effective = base_annual_interest_rate_pct;
    for (_, override_rate) in rate_overrides.range(..=date) {
        effective = *override_rate;
    }
    effective
}

fn accrue_interest_for_period_daily(
    principal: f64,
    period_start: DateYmd,
    period_end: DateYmd,
    base_annual_interest_rate_pct: f64,
    rate_overrides: &BTreeMap<DateYmd, f64>,
) -> f64 {
    if period_end <= period_start {
        return 0.0;
    }

    let mut interest = 0.0;
    let mut segment_start = period_start;
    let mut segment_rate =
        rate_at_date(segment_start, base_annual_interest_rate_pct, rate_overrides);

    for (change_date, override_rate) in rate_overrides.range(segment_start..period_end) {
        if *change_date <= segment_start || *change_date >= period_end {
            continue;
        }

        let segment_days =
            (change_date.days_since_epoch() - segment_start.days_since_epoch()).max(0) as f64;
        interest += principal * (segment_rate / 100.0) * (segment_days / 365.0);

        segment_start = *change_date;
        segment_rate = *override_rate;
    }

    let remaining_days =
        (period_end.days_since_epoch() - segment_start.days_since_epoch()).max(0) as f64;
    interest += principal * (segment_rate / 100.0) * (remaining_days / 365.0);
    interest
}

fn accrue_interest_for_period_daily_signed(
    principal: f64,
    from: DateYmd,
    to: DateYmd,
    base_annual_interest_rate_pct: f64,
    rate_overrides: &BTreeMap<DateYmd, f64>,
) -> f64 {
    if to >= from {
        accrue_interest_for_period_daily(
            principal,
            from,
            to,
            base_annual_interest_rate_pct,
            rate_overrides,
        )
    } else {
        -accrue_interest_for_period_daily(
            principal,
            to,
            from,
            base_annual_interest_rate_pct,
            rate_overrides,
        )
    }
}

fn build_payment_dates(start_date: DateYmd, payment_day: u32, total_months: u32) -> Vec<DateYmd> {
    let mut dates = Vec::with_capacity(total_months as usize);

    for installment in 1..=total_months {
        let (year, month) =
            add_months_to_year_month(start_date.year, start_date.month, installment);
        let day = payment_day.min(last_day_of_month(year, month));
        let payment_date =
            DateYmd::from_ymd_opt(year, month, day).expect("computed payment date must be valid");
        dates.push(payment_date);
    }

    dates
}

fn add_months_to_year_month(year: i32, month: u32, delta_months: u32) -> (i32, u32) {
    let total_months = year * 12 + (month as i32 - 1) + delta_months as i32;
    let new_year = total_months.div_euclid(12);
    let new_month = total_months.rem_euclid(12) + 1;
    (new_year, new_month as u32)
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };

    let next_month_days = days_from_civil(next_year, next_month, 1);
    let this_month_last_day = civil_from_days(next_month_days - 1);
    this_month_last_day.2
}

fn compute_monthly_payment(principal: f64, annual_rate_pct: f64, remaining_months: u32) -> f64 {
    let monthly_rate = annual_rate_pct / 100.0 / 12.0;

    if monthly_rate.abs() < f64::EPSILON {
        return principal / remaining_months as f64;
    }

    let months = remaining_months as f64;
    let growth = (1.0 + monthly_rate).powf(months);
    principal * monthly_rate * growth / (growth - 1.0)
}

fn round_half_up(value: f64) -> f64 {
    value.round()
}

fn validate_non_negative(field: &'static str, value: f64) -> Result<(), CalcError> {
    if !value.is_finite() || value < 0.0 {
        return Err(CalcError::InvalidNonNegativeField(field));
    }

    Ok(())
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

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let mut y = year as i64;
    let m = month as i64;
    let d = day as i64;

    y -= if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
