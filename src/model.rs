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
pub struct ExtraPayment {
    pub effective_date: DateYmd,
    pub amount: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecurringExtraPayment {
    pub start_date: DateYmd,
    pub month: u32,
    pub day: u32,
    pub amount: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterestBasisMode {
    Act365Fixed,
    ActActual,
    ThirtyE360,
    Apr12Monthly,
}

impl InterestBasisMode {
    pub const ALL: [InterestBasisMode; 4] = [
        InterestBasisMode::Act365Fixed,
        InterestBasisMode::ActActual,
        InterestBasisMode::ThirtyE360,
        InterestBasisMode::Apr12Monthly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            InterestBasisMode::Act365Fixed => "ACT/365",
            InterestBasisMode::ActActual => "ACT/ACT",
            InterestBasisMode::ThirtyE360 => "30E/360",
            InterestBasisMode::Apr12Monthly => "APR/12 monthly",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            InterestBasisMode::Act365Fixed => "Actual days with fixed 365-day denominator.",
            InterestBasisMode::ActActual => "Actual days with 365/366 denominator by year.",
            InterestBasisMode::ThirtyE360 => "30E/360 day count convention.",
            InterestBasisMode::Apr12Monthly => "APR/12 monthly basis prorated by actual/30 days.",
        }
    }

    pub fn persisted_key(self) -> &'static str {
        match self {
            InterestBasisMode::Act365Fixed => "act_365",
            InterestBasisMode::ActActual => "act_act",
            InterestBasisMode::ThirtyE360 => "30e_360",
            InterestBasisMode::Apr12Monthly => "apr_12_monthly",
        }
    }

    pub fn from_persisted_key(value: &str) -> Option<Self> {
        match value {
            "act_365" => Some(InterestBasisMode::Act365Fixed),
            "act_act" => Some(InterestBasisMode::ActActual),
            "30e_360" => Some(InterestBasisMode::ThirtyE360),
            "apr_12_monthly" => Some(InterestBasisMode::Apr12Monthly),
            _ => None,
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppliedExtraPaymentEntry {
    pub effective_date: DateYmd,
    pub applied_amount: f64,
    pub source: AppliedExtraPaymentSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppliedExtraPaymentSource {
    OneTime,
    Recurring {
        start_date: DateYmd,
        month: u32,
        day: u32,
    },
}

#[derive(Debug, Clone)]
pub struct LoanInput {
    pub loan_amount: f64,
    pub one_time_fees: f64,
    pub monthly_fees: f64,
    pub round_monthly_payment_up: bool,
    pub interest_basis_mode: InterestBasisMode,
    pub base_annual_interest_rate_pct: f64,
    pub term_years: u32,
    pub start_date: DateYmd,
    pub payment_day: u32,
    pub rate_overrides: Vec<RateOverride>,
    pub extra_payments: Vec<ExtraPayment>,
    pub recurring_extra_payments: Vec<RecurringExtraPayment>,
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
    pub applied_extra_payments: Vec<AppliedExtraPaymentEntry>,
    pub total_extra_payments: f64,
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
    #[error("extra payment date {date} is out of range ({min_date}..={max_date})")]
    InvalidExtraPaymentDate {
        date: DateYmd,
        min_date: DateYmd,
        max_date: DateYmd,
    },
    #[error("extra payment amount for date {date} must be a finite number >= 0")]
    InvalidExtraPaymentAmount { date: DateYmd },
    #[error("recurring extra payment start date {date} is out of range ({min_date}..={max_date})")]
    InvalidRecurringExtraPaymentStartDate {
        date: DateYmd,
        min_date: DateYmd,
        max_date: DateYmd,
    },
    #[error(
        "recurring extra payment month/day ({month:02}-{day:02}) for start date {start_date} must be valid in range month 1..=12, day 1..=31"
    )]
    InvalidRecurringExtraPaymentMonthDay {
        start_date: DateYmd,
        month: u32,
        day: u32,
    },
    #[error(
        "recurring extra payment amount for start date {start_date} and annual date {month:02}-{day:02} must be a finite number >= 0"
    )]
    InvalidRecurringExtraPaymentAmount {
        start_date: DateYmd,
        month: u32,
        day: u32,
    },
    #[error("selected month {month} is out of range (1..={max_month})")]
    InvalidSelectedMonth { month: u32, max_month: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RecurringExtraPaymentKey {
    start_date: DateYmd,
    month: u32,
    day: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExtraPaymentEvent {
    amount: f64,
    source: AppliedExtraPaymentSource,
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
    let extra_payments =
        normalize_extra_payments(&input.extra_payments, input.start_date, last_payment_date)?;
    let recurring_extra_payments = normalize_recurring_extra_payments(
        &input.recurring_extra_payments,
        input.start_date,
        last_payment_date,
    )?;
    let extra_payment_events = build_extra_payment_events(
        &extra_payments,
        &recurring_extra_payments,
        input.start_date,
        last_payment_date,
    );
    let interest_basis_mode = input.interest_basis_mode;

    let mut remaining_principal = input.loan_amount;
    let mut segments = Vec::with_capacity(rate_overrides.len() + extra_payment_events.len() + 1);
    let mut repayment_schedule = Vec::with_capacity(total_months as usize);
    let mut applied_extra_payments = Vec::new();
    let mut current_segment_start_month = 1_u32;
    let mut current_segment_rate = 0.0_f64;
    let mut current_monthly_payment_base = 0.0_f64;
    let mut has_active_segment = false;
    let mut recompute_monthly_payment_at_month_start = false;
    let mut rounded_interest_carry = 0.0_f64;

    apply_extra_payments_on_date(
        &mut remaining_principal,
        input.start_date,
        &extra_payment_events,
        &mut applied_extra_payments,
    );

    for month in 1..=total_months {
        let payment_date = payment_dates[(month - 1) as usize];
        let regular_period_start = if month == 1 {
            start_month_anchor
        } else {
            payment_dates[(month - 2) as usize]
        };

        let mut principal_for_regular_period = remaining_principal;
        let mut arrears_interest_signed = 0.0_f64;
        let mut had_extra_payment_before_scheduled_payment = false;

        if month == 1 {
            if input.start_date <= start_month_anchor {
                let (arrears_interest, principal_after_arrears, had_arrears_extra_payment) =
                    simulate_period_daily_with_events(
                        principal_for_regular_period,
                        input.start_date,
                        start_month_anchor,
                        input.base_annual_interest_rate_pct,
                        &rate_overrides,
                        &extra_payment_events,
                        &mut applied_extra_payments,
                        interest_basis_mode,
                    );
                principal_for_regular_period = principal_after_arrears;
                arrears_interest_signed = arrears_interest;
                had_extra_payment_before_scheduled_payment |= had_arrears_extra_payment;

                if apply_extra_payments_on_date(
                    &mut principal_for_regular_period,
                    start_month_anchor,
                    &extra_payment_events,
                    &mut applied_extra_payments,
                ) {
                    had_extra_payment_before_scheduled_payment = true;
                }
            } else {
                arrears_interest_signed = -accrue_interest_for_period_daily(
                    principal_for_regular_period,
                    start_month_anchor,
                    input.start_date,
                    input.base_annual_interest_rate_pct,
                    &rate_overrides,
                    interest_basis_mode,
                );
            }
        }

        let (interest_payment, principal_before_scheduled_payment, had_regular_extra_payment) =
            simulate_period_daily_with_events(
                principal_for_regular_period,
                regular_period_start,
                payment_date,
                input.base_annual_interest_rate_pct,
                &rate_overrides,
                &extra_payment_events,
                &mut applied_extra_payments,
                interest_basis_mode,
            );
        had_extra_payment_before_scheduled_payment |= had_regular_extra_payment;

        let payment_rate = rate_at_date(
            payment_date,
            input.base_annual_interest_rate_pct,
            &rate_overrides,
        );

        let rate_changed =
            has_active_segment && (payment_rate - current_segment_rate).abs() > 1e-12;
        let should_recompute_monthly_payment = !has_active_segment
            || recompute_monthly_payment_at_month_start
            || had_extra_payment_before_scheduled_payment
            || rate_changed;

        if should_recompute_monthly_payment {
            if has_active_segment {
                segments.push(PaymentSegment {
                    start_month: current_segment_start_month,
                    end_month: month - 1,
                    annual_interest_rate_pct: current_segment_rate,
                    monthly_payment_base: current_monthly_payment_base,
                });
            }

            current_segment_start_month = month;
            current_segment_rate = payment_rate;
            if input.round_monthly_payment_up {
                let remaining_term_months = total_months - month + 1;
                current_monthly_payment_base = round_half_up(compute_monthly_payment(
                    principal_before_scheduled_payment,
                    current_segment_rate,
                    remaining_term_months,
                ));
            } else {
                current_monthly_payment_base = solve_monthly_payment_base_daily(
                    principal_before_scheduled_payment,
                    interest_payment,
                    month,
                    &payment_dates,
                    current_segment_rate,
                    interest_basis_mode,
                );
            }
            has_active_segment = true;
        }

        let total_interest_payment = interest_payment + arrears_interest_signed;
        let (
            interest_payment_for_schedule,
            mut principal_payment_for_schedule,
            principal_payment_for_ledger,
        ) = if input.round_monthly_payment_up {
            let interest_with_carry = total_interest_payment + rounded_interest_carry;
            let total_interest_posted = round_down_towards_zero(interest_with_carry);
            rounded_interest_carry = interest_with_carry - total_interest_posted;
            if rounded_interest_carry.abs() < 1e-12 {
                rounded_interest_carry = 0.0;
            }

            let regular_interest_posted = round_down_towards_zero(interest_payment);
            let mut principal_posted = if month == 1 {
                // First month carries the signed arrears adjustment in payment amount.
                current_monthly_payment_base - regular_interest_posted
            } else {
                current_monthly_payment_base - total_interest_posted
            };
            if principal_posted < 0.0 {
                principal_posted = 0.0;
            }
            if principal_posted > principal_before_scheduled_payment {
                principal_posted = principal_before_scheduled_payment;
            }
            (total_interest_posted, principal_posted, principal_posted)
        } else {
            let mut principal_payment = current_monthly_payment_base - interest_payment;
            if principal_payment < 0.0 {
                principal_payment = 0.0;
            }
            if principal_payment > principal_before_scheduled_payment {
                principal_payment = principal_before_scheduled_payment;
            }
            (total_interest_payment, principal_payment, principal_payment)
        };

        remaining_principal = principal_before_scheduled_payment - principal_payment_for_ledger;
        if remaining_principal.abs() < 1e-8 {
            remaining_principal = 0.0;
        }
        if month == total_months && remaining_principal > 1e-8 {
            principal_payment_for_schedule += remaining_principal;
            remaining_principal = 0.0;
        }

        let had_same_day_extra_payment = apply_extra_payments_on_date(
            &mut remaining_principal,
            payment_date,
            &extra_payment_events,
            &mut applied_extra_payments,
        );
        recompute_monthly_payment_at_month_start = had_same_day_extra_payment;

        let fees_payment = input.monthly_fees;
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

    if has_active_segment {
        segments.push(PaymentSegment {
            start_month: current_segment_start_month,
            end_month: total_months,
            annual_interest_rate_pct: current_segment_rate,
            monthly_payment_base: current_monthly_payment_base,
        });
    }

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
    let total_extra_payments: f64 = applied_extra_payments
        .iter()
        .map(|entry| entry.applied_amount)
        .sum();
    let total_repayment = total_repayment + total_extra_payments;
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
        applied_extra_payments,
        total_extra_payments,
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

fn normalize_extra_payments(
    extra_payments: &[ExtraPayment],
    start_date: DateYmd,
    last_payment_date: DateYmd,
) -> Result<BTreeMap<DateYmd, f64>, CalcError> {
    let mut normalized = BTreeMap::new();

    for extra_payment in extra_payments {
        if extra_payment.effective_date < start_date
            || extra_payment.effective_date > last_payment_date
        {
            return Err(CalcError::InvalidExtraPaymentDate {
                date: extra_payment.effective_date,
                min_date: start_date,
                max_date: last_payment_date,
            });
        }

        if !extra_payment.amount.is_finite() || extra_payment.amount < 0.0 {
            return Err(CalcError::InvalidExtraPaymentAmount {
                date: extra_payment.effective_date,
            });
        }

        if extra_payment.amount == 0.0 {
            continue;
        }

        let entry = normalized
            .entry(extra_payment.effective_date)
            .or_insert(0.0);
        *entry += extra_payment.amount;
    }

    Ok(normalized)
}

fn normalize_recurring_extra_payments(
    recurring_extra_payments: &[RecurringExtraPayment],
    start_date: DateYmd,
    last_payment_date: DateYmd,
) -> Result<BTreeMap<RecurringExtraPaymentKey, f64>, CalcError> {
    let mut normalized = BTreeMap::new();

    for recurring in recurring_extra_payments {
        if recurring.start_date < start_date || recurring.start_date > last_payment_date {
            return Err(CalcError::InvalidRecurringExtraPaymentStartDate {
                date: recurring.start_date,
                min_date: start_date,
                max_date: last_payment_date,
            });
        }

        if recurring.month == 0 || recurring.month > 12 || recurring.day == 0 || recurring.day > 31
        {
            return Err(CalcError::InvalidRecurringExtraPaymentMonthDay {
                start_date: recurring.start_date,
                month: recurring.month,
                day: recurring.day,
            });
        }

        if !recurring.amount.is_finite() || recurring.amount < 0.0 {
            return Err(CalcError::InvalidRecurringExtraPaymentAmount {
                start_date: recurring.start_date,
                month: recurring.month,
                day: recurring.day,
            });
        }

        if recurring.amount == 0.0 {
            continue;
        }

        let key = RecurringExtraPaymentKey {
            start_date: recurring.start_date,
            month: recurring.month,
            day: recurring.day,
        };
        let entry = normalized.entry(key).or_insert(0.0);
        *entry += recurring.amount;
    }

    Ok(normalized)
}

fn build_extra_payment_events(
    one_time_extra_payments: &BTreeMap<DateYmd, f64>,
    recurring_extra_payments: &BTreeMap<RecurringExtraPaymentKey, f64>,
    start_date: DateYmd,
    last_payment_date: DateYmd,
) -> BTreeMap<DateYmd, Vec<ExtraPaymentEvent>> {
    let mut events: BTreeMap<DateYmd, Vec<ExtraPaymentEvent>> = BTreeMap::new();

    for (date, amount) in one_time_extra_payments {
        if *amount <= 0.0 {
            continue;
        }

        events.entry(*date).or_default().push(ExtraPaymentEvent {
            amount: *amount,
            source: AppliedExtraPaymentSource::OneTime,
        });
    }

    for (key, amount) in recurring_extra_payments {
        if *amount <= 0.0 {
            continue;
        }

        let mut year = start_date.year.max(key.start_date.year);
        while year <= last_payment_date.year {
            let day = key.day.min(last_day_of_month(year, key.month));
            let Some(candidate_date) = DateYmd::from_ymd_opt(year, key.month, day) else {
                year += 1;
                continue;
            };

            if candidate_date >= key.start_date
                && candidate_date >= start_date
                && candidate_date <= last_payment_date
            {
                events
                    .entry(candidate_date)
                    .or_default()
                    .push(ExtraPaymentEvent {
                        amount: *amount,
                        source: AppliedExtraPaymentSource::Recurring {
                            start_date: key.start_date,
                            month: key.month,
                            day: key.day,
                        },
                    });
            }

            year += 1;
        }
    }

    for per_day_events in events.values_mut() {
        per_day_events.sort_by_key(|event| match event.source {
            AppliedExtraPaymentSource::OneTime => 0_u8,
            AppliedExtraPaymentSource::Recurring { .. } => 1_u8,
        });
    }

    events
}

fn apply_extra_payments_on_date(
    principal: &mut f64,
    date: DateYmd,
    extra_payments: &BTreeMap<DateYmd, Vec<ExtraPaymentEvent>>,
    applied_extra_payments: &mut Vec<AppliedExtraPaymentEntry>,
) -> bool {
    if *principal <= 0.0 {
        return false;
    }

    let Some(events_on_date) = extra_payments.get(&date) else {
        return false;
    };

    let mut had_any = false;

    for event in events_on_date {
        if *principal <= 0.0 {
            break;
        }

        if event.amount <= 0.0 {
            continue;
        }

        let applied_amount = principal.min(event.amount);
        if applied_amount <= 0.0 {
            continue;
        }

        *principal -= applied_amount;
        if principal.abs() < 1e-8 {
            *principal = 0.0;
        }

        applied_extra_payments.push(AppliedExtraPaymentEntry {
            effective_date: date,
            applied_amount,
            source: event.source,
        });
        had_any = true;
    }

    had_any
}

fn simulate_period_daily_with_events(
    principal_start: f64,
    period_start: DateYmd,
    period_end: DateYmd,
    base_annual_interest_rate_pct: f64,
    rate_overrides: &BTreeMap<DateYmd, f64>,
    extra_payments: &BTreeMap<DateYmd, Vec<ExtraPaymentEvent>>,
    applied_extra_payments: &mut Vec<AppliedExtraPaymentEntry>,
    interest_basis_mode: InterestBasisMode,
) -> (f64, f64, bool) {
    if period_end <= period_start {
        return (0.0, principal_start, false);
    }

    let mut interest = 0.0;
    let mut principal = principal_start;
    let mut cursor = period_start;
    let mut segment_rate = rate_at_date(cursor, base_annual_interest_rate_pct, rate_overrides);
    let mut had_extra_payment = false;

    loop {
        let next_rate_change_date = rate_overrides
            .range(cursor..period_end)
            .find_map(|(date, _)| if *date > cursor { Some(*date) } else { None });
        let next_extra_payment_date =
            extra_payments
                .range(cursor..period_end)
                .find_map(|(date, per_day_events)| {
                    if *date > cursor && per_day_events.iter().any(|event| event.amount > 0.0) {
                        Some(*date)
                    } else {
                        None
                    }
                });

        let next_event_date = match (next_rate_change_date, next_extra_payment_date) {
            (Some(rate_date), Some(extra_date)) => Some(rate_date.min(extra_date)),
            (Some(rate_date), None) => Some(rate_date),
            (None, Some(extra_date)) => Some(extra_date),
            (None, None) => None,
        };

        let segment_end = next_event_date.unwrap_or(period_end);
        interest += accrue_interest_with_day_count(
            principal,
            cursor,
            segment_end,
            segment_rate,
            interest_basis_mode,
        );

        if segment_end >= period_end {
            break;
        }

        if let Some(override_rate) = rate_overrides.get(&segment_end).copied() {
            segment_rate = override_rate;
        }
        if apply_extra_payments_on_date(
            &mut principal,
            segment_end,
            extra_payments,
            applied_extra_payments,
        ) {
            had_extra_payment = true;
        }

        cursor = segment_end;
    }

    (interest, principal, had_extra_payment)
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
    interest_basis_mode: InterestBasisMode,
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

        interest += accrue_interest_with_day_count(
            principal,
            segment_start,
            *change_date,
            segment_rate,
            interest_basis_mode,
        );

        segment_start = *change_date;
        segment_rate = *override_rate;
    }

    interest += accrue_interest_with_day_count(
        principal,
        segment_start,
        period_end,
        segment_rate,
        interest_basis_mode,
    );
    interest
}

fn accrue_interest_with_day_count(
    principal: f64,
    from: DateYmd,
    to: DateYmd,
    annual_rate_pct: f64,
    interest_basis_mode: InterestBasisMode,
) -> f64 {
    if to <= from || principal <= 0.0 || annual_rate_pct == 0.0 {
        return 0.0;
    }

    match interest_basis_mode {
        InterestBasisMode::Act365Fixed => {
            let days = (to.days_since_epoch() - from.days_since_epoch()).max(0) as f64;
            principal * (annual_rate_pct / 100.0) * (days / 365.0)
        }
        InterestBasisMode::ActActual => {
            let mut interest = 0.0;
            let mut cursor = from;
            while cursor < to {
                let next_year_start =
                    DateYmd::from_ymd_opt(cursor.year + 1, 1, 1).expect("valid next-year date");
                let segment_end = if next_year_start < to {
                    next_year_start
                } else {
                    to
                };
                let segment_days =
                    (segment_end.days_since_epoch() - cursor.days_since_epoch()).max(0) as f64;
                let denominator = if is_leap_year(cursor.year) {
                    366.0
                } else {
                    365.0
                };
                interest += principal * (annual_rate_pct / 100.0) * (segment_days / denominator);
                cursor = segment_end;
            }
            interest
        }
        InterestBasisMode::ThirtyE360 => {
            let days_30e_360 = day_count_30e_360(from, to) as f64;
            principal * (annual_rate_pct / 100.0) * (days_30e_360 / 360.0)
        }
        InterestBasisMode::Apr12Monthly => {
            let actual_days = (to.days_since_epoch() - from.days_since_epoch()).max(0) as f64;
            principal * (annual_rate_pct / 100.0 / 12.0) * (actual_days / 30.0)
        }
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

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
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

fn solve_monthly_payment_base_daily(
    principal_before_current_payment: f64,
    current_period_interest: f64,
    current_month: u32,
    payment_dates: &[DateYmd],
    annual_interest_rate_pct: f64,
    interest_basis_mode: InterestBasisMode,
) -> f64 {
    if principal_before_current_payment <= 0.0 {
        return 0.0;
    }

    let residual_at_zero = remaining_principal_after_constant_payment_base(
        principal_before_current_payment,
        current_period_interest,
        0.0,
        current_month,
        payment_dates,
        annual_interest_rate_pct,
        interest_basis_mode,
    );
    if residual_at_zero <= 1e-8 {
        return 0.0;
    }

    let mut low = 0.0;
    let mut high = (principal_before_current_payment + current_period_interest).max(1.0);
    let mut high_residual = remaining_principal_after_constant_payment_base(
        principal_before_current_payment,
        current_period_interest,
        high,
        current_month,
        payment_dates,
        annual_interest_rate_pct,
        interest_basis_mode,
    );

    let mut expansion_steps = 0;
    while high_residual > 1e-8 && expansion_steps < 96 {
        high *= 2.0;
        high_residual = remaining_principal_after_constant_payment_base(
            principal_before_current_payment,
            current_period_interest,
            high,
            current_month,
            payment_dates,
            annual_interest_rate_pct,
            interest_basis_mode,
        );
        expansion_steps += 1;
    }

    if high_residual > 1e-8 {
        return high;
    }

    for _ in 0..96 {
        let mid = (low + high) * 0.5;
        let residual = remaining_principal_after_constant_payment_base(
            principal_before_current_payment,
            current_period_interest,
            mid,
            current_month,
            payment_dates,
            annual_interest_rate_pct,
            interest_basis_mode,
        );
        if residual > 1e-8 {
            low = mid;
        } else {
            high = mid;
        }
    }

    high
}

fn remaining_principal_after_constant_payment_base(
    principal_before_current_payment: f64,
    current_period_interest: f64,
    monthly_payment_base: f64,
    current_month: u32,
    payment_dates: &[DateYmd],
    annual_interest_rate_pct: f64,
    interest_basis_mode: InterestBasisMode,
) -> f64 {
    let total_months = payment_dates.len() as u32;
    let mut principal = principal_before_current_payment.max(0.0);

    for month in current_month..=total_months {
        let interest_payment = if month == current_month {
            current_period_interest
        } else {
            let period_start = payment_dates[(month - 2) as usize];
            let period_end = payment_dates[(month - 1) as usize];
            accrue_interest_with_day_count(
                principal,
                period_start,
                period_end,
                annual_interest_rate_pct,
                interest_basis_mode,
            )
        };

        let mut principal_payment = monthly_payment_base - interest_payment;
        if principal_payment < 0.0 {
            principal_payment = 0.0;
        }
        if principal_payment > principal {
            principal_payment = principal;
        }

        principal -= principal_payment;
        if principal.abs() < 1e-8 {
            principal = 0.0;
        }
    }

    principal
}

fn round_half_up(value: f64) -> f64 {
    if !value.is_finite() {
        return value;
    }
    if value >= 0.0 {
        (value + 0.5).floor()
    } else {
        (value - 0.5).ceil()
    }
}

fn round_down_towards_zero(value: f64) -> f64 {
    value.trunc()
}

fn day_count_30e_360(from: DateYmd, to: DateYmd) -> i64 {
    let y1 = from.year as i64;
    let m1 = from.month as i64;
    let d1 = from.day.min(30) as i64;

    let y2 = to.year as i64;
    let m2 = to.month as i64;
    let d2 = to.day.min(30) as i64;

    let days = 360 * (y2 - y1) + 30 * (m2 - m1) + (d2 - d1);
    days.max(0)
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
