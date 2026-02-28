use std::collections::BTreeMap;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateOverride {
    pub start_month: u32,
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
    #[error("rate override month {month} is out of range (1..={max_month})")]
    InvalidOverrideMonth { month: u32, max_month: u32 },
    #[error("duplicate rate override for month {0}")]
    DuplicateOverrideMonth(u32),
    #[error("rate override APR for month {month} must be a finite number >= 0")]
    InvalidOverrideRate { month: u32 },
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

    let total_months = input.term_years.saturating_mul(12);
    if selected_month == 0 || selected_month > total_months {
        return Err(CalcError::InvalidSelectedMonth {
            month: selected_month,
            max_month: total_months,
        });
    }

    let change_points = normalize_rate_overrides(
        input.base_annual_interest_rate_pct,
        &input.rate_overrides,
        total_months,
    )?;

    let mut remaining_principal = input.loan_amount;
    let mut segments = Vec::with_capacity(change_points.len());
    let mut repayment_schedule = Vec::with_capacity(total_months as usize);

    for (idx, (start_month, annual_rate_pct)) in change_points.iter().enumerate() {
        let end_month = if idx + 1 < change_points.len() {
            change_points[idx + 1].0 - 1
        } else {
            total_months
        };

        let remaining_term_months = total_months - *start_month + 1;
        let monthly_payment_base =
            compute_monthly_payment(remaining_principal, *annual_rate_pct, remaining_term_months);

        segments.push(PaymentSegment {
            start_month: *start_month,
            end_month,
            annual_interest_rate_pct: *annual_rate_pct,
            monthly_payment_base,
        });

        let monthly_rate = *annual_rate_pct / 100.0 / 12.0;

        for month in *start_month..=end_month {
            let interest_payment = remaining_principal * monthly_rate;
            let mut principal_payment = monthly_payment_base - interest_payment;
            let mut actual_payment = monthly_payment_base;

            if month == total_months || principal_payment > remaining_principal {
                principal_payment = remaining_principal;
                actual_payment = principal_payment + interest_payment;
            }

            remaining_principal -= principal_payment;
            if remaining_principal.abs() < 1e-8 {
                remaining_principal = 0.0;
            }

            let fees_payment = input.monthly_fees;
            let mut principal_payment_for_schedule = principal_payment;
            let mut total_payment = actual_payment + fees_payment;
            if input.round_monthly_payment_up {
                let rounded_total = total_payment.ceil();
                principal_payment_for_schedule += rounded_total - total_payment;
                total_payment = rounded_total;
            }

            repayment_schedule.push(RepaymentScheduleEntry {
                month_index: month,
                effective_annual_interest_rate_pct: *annual_rate_pct,
                total_payment,
                interest_payment,
                principal_payment: principal_payment_for_schedule,
                fees_payment,
            });
        }
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
    base_annual_interest_rate_pct: f64,
    rate_overrides: &[RateOverride],
    total_months: u32,
) -> Result<Vec<(u32, f64)>, CalcError> {
    let mut overrides = BTreeMap::new();

    for rate_override in rate_overrides {
        if rate_override.start_month == 0 || rate_override.start_month > total_months {
            return Err(CalcError::InvalidOverrideMonth {
                month: rate_override.start_month,
                max_month: total_months,
            });
        }

        if !rate_override.annual_interest_rate_pct.is_finite()
            || rate_override.annual_interest_rate_pct < 0.0
        {
            return Err(CalcError::InvalidOverrideRate {
                month: rate_override.start_month,
            });
        }

        if overrides
            .insert(
                rate_override.start_month,
                rate_override.annual_interest_rate_pct,
            )
            .is_some()
        {
            return Err(CalcError::DuplicateOverrideMonth(rate_override.start_month));
        }
    }

    let mut change_points = BTreeMap::new();
    change_points.insert(1, base_annual_interest_rate_pct);

    for (month, rate) in overrides {
        change_points.insert(month, rate);
    }

    Ok(change_points.into_iter().collect())
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

fn validate_non_negative(field: &'static str, value: f64) -> Result<(), CalcError> {
    if !value.is_finite() || value < 0.0 {
        return Err(CalcError::InvalidNonNegativeField(field));
    }

    Ok(())
}
