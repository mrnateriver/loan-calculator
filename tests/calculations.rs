#[path = "../src/model.rs"]
mod model;

use approx::assert_relative_eq;
use model::{CalcError, DateYmd, LoanInput, RateOverride, calculate_metrics};

fn sample_input() -> LoanInput {
    LoanInput {
        loan_amount: 300_000.0,
        one_time_fees: 8_000.0,
        monthly_fees: 120.0,
        round_monthly_payment_up: false,
        base_annual_interest_rate_pct: 6.0,
        term_years: 30,
        start_date: DateYmd::from_ymd_opt(2026, 9, 12).expect("valid date"),
        payment_day: 15,
        rate_overrides: vec![],
    }
}

#[test]
fn fixed_rate_metrics_still_match_baseline_values() {
    let input = sample_input();
    let metrics = calculate_metrics(&input, 1).expect("calculation should succeed");

    assert!(metrics.first_monthly_payment_base.is_finite());
    assert!(metrics.first_monthly_payment_base > 0.0);
    assert!(metrics.selected_monthly_payment_base.is_finite());
    assert!(metrics.selected_monthly_payment_with_fees > metrics.selected_monthly_payment_base);
    assert_relative_eq!(
        metrics.selected_month_effective_rate_pct,
        6.0,
        epsilon = 1e-9
    );
    assert_eq!(metrics.next_change_month, None);
    assert!(metrics.total_interest > 0.0);
    assert!(metrics.total_monthly_fees > 0.0);
    assert!(metrics.total_repayment > metrics.total_interest);
    assert!(metrics.total_paid_all_in > metrics.total_repayment);
    assert!(metrics.loan_cost > 0.0);
    assert_eq!(metrics.repayment_schedule.len(), 360);
    assert!(
        metrics
            .repayment_schedule
            .iter()
            .all(|row| (row.effective_annual_interest_rate_pct - 6.0).abs() < 1e-12)
    );
}

#[test]
fn first_payment_date_and_first_period_interest_use_day_count() {
    let input = sample_input();
    let metrics = calculate_metrics(&input, 1).expect("calculation should succeed");

    let first = metrics
        .repayment_schedule
        .first()
        .expect("schedule should include first payment");

    assert_eq!(
        first.payment_date,
        DateYmd::from_ymd_opt(2026, 10, 15).expect("valid expected date")
    );

    // 2026-09-12 -> 2026-10-15 is 33 days with exclusive end date arithmetic.
    let expected_interest =
        input.loan_amount * (input.base_annual_interest_rate_pct / 100.0) * 33.0 / 365.0;
    assert_relative_eq!(first.interest_payment, expected_interest, epsilon = 1e-6);
}

#[test]
fn payment_day_clamps_to_last_day_for_short_months() {
    let mut input = sample_input();
    input.start_date = DateYmd::from_ymd_opt(2026, 1, 20).expect("valid start date");
    input.payment_day = 31;

    let metrics = calculate_metrics(&input, 1).expect("calculation should succeed");

    assert_eq!(
        metrics.repayment_schedule[0].payment_date,
        DateYmd::from_ymd_opt(2026, 2, 28).expect("valid expected date")
    );
    assert_eq!(
        metrics.repayment_schedule[1].payment_date,
        DateYmd::from_ymd_opt(2026, 3, 31).expect("valid expected date")
    );
    assert_eq!(
        metrics.repayment_schedule[2].payment_date,
        DateYmd::from_ymd_opt(2026, 4, 30).expect("valid expected date")
    );
}

#[test]
fn first_payment_uses_regular_principal_plus_arrears_interest_surcharge() {
    let mut input = sample_input();
    input.loan_amount = 4_204_720.0;
    input.monthly_fees = 60.0;
    input.base_annual_interest_rate_pct = 5.24;
    input.start_date = DateYmd::from_ymd_opt(2025, 9, 12).expect("valid start date");
    input.payment_day = 15;
    input.round_monthly_payment_up = true;

    let metrics = calculate_metrics(&input, 1).expect("calculation should succeed");
    let first = metrics
        .repayment_schedule
        .first()
        .expect("schedule should include first payment");
    let second = &metrics.repayment_schedule[1];

    assert_relative_eq!(
        first.interest_payment,
        19_920.004_997_260_276,
        epsilon = 1e-6
    );
    assert_relative_eq!(first.principal_payment, 5_084.0, epsilon = 1e-2);
    assert_relative_eq!(first.total_payment, 25_064.0, epsilon = 1e-9);
    assert!(first.total_payment > second.total_payment);
}

#[test]
fn start_after_payment_day_applies_signed_first_period_credit() {
    let mut input = sample_input();
    input.start_date = DateYmd::from_ymd_opt(2026, 9, 20).expect("valid start date");
    input.payment_day = 15;
    input.round_monthly_payment_up = false;

    let metrics = calculate_metrics(&input, 1).expect("calculation should succeed");
    let first = metrics
        .repayment_schedule
        .first()
        .expect("schedule should include first payment");
    let second = &metrics.repayment_schedule[1];

    assert!(
        first.total_payment < second.total_payment,
        "signed arrears should credit the first payment when start date is after payment day"
    );
    assert!(
        first.interest_payment < second.interest_payment,
        "first period interest should reflect fewer effective accrual days after signed normalization"
    );
}

#[test]
fn single_override_changes_payment_from_its_start_month() {
    let mut input = sample_input();
    input.rate_overrides.push(RateOverride {
        start_month: 61,
        annual_interest_rate_pct: 7.0,
    });

    let baseline = calculate_metrics(&sample_input(), 61).expect("baseline should succeed");
    let before_change = calculate_metrics(&input, 60).expect("before-change should succeed");
    let at_change = calculate_metrics(&input, 61).expect("change-month should succeed");

    assert_relative_eq!(
        before_change.selected_monthly_payment_base,
        before_change.first_monthly_payment_base,
        epsilon = 1e-9
    );
    assert_eq!(before_change.next_change_month, Some(61));

    assert_relative_eq!(
        at_change.selected_month_effective_rate_pct,
        7.0,
        epsilon = 1e-9
    );
    assert!(
        at_change.selected_monthly_payment_base > before_change.selected_monthly_payment_base,
        "payment should increase after rate hike"
    );
    assert!(at_change.total_interest > baseline.total_interest);

    assert_eq!(at_change.segments.len(), 2);
    assert_eq!(at_change.segments[0].start_month, 1);
    assert_eq!(at_change.segments[0].end_month, 60);
    assert_eq!(at_change.segments[1].start_month, 61);
    assert_eq!(at_change.segments[1].end_month, 360);
}

#[test]
fn multiple_overrides_generate_expected_segments() {
    let mut input = sample_input();
    input.rate_overrides = vec![
        RateOverride {
            start_month: 25,
            annual_interest_rate_pct: 4.5,
        },
        RateOverride {
            start_month: 49,
            annual_interest_rate_pct: 8.0,
        },
        RateOverride {
            start_month: 97,
            annual_interest_rate_pct: 5.0,
        },
    ];

    let metrics = calculate_metrics(&input, 50).expect("calculation should succeed");

    assert_eq!(metrics.segments.len(), 4);
    assert_eq!(
        metrics
            .segments
            .iter()
            .map(|segment| (segment.start_month, segment.end_month))
            .collect::<Vec<_>>(),
        vec![(1, 24), (25, 48), (49, 96), (97, 360)]
    );

    assert_relative_eq!(
        metrics.selected_month_effective_rate_pct,
        8.0,
        epsilon = 1e-9
    );
    assert_eq!(metrics.next_change_month, Some(97));
    assert!(metrics.selected_monthly_payment_base.is_finite());
    assert!(metrics.selected_monthly_payment_base > 0.0);

    let apr_at =
        |month: usize| metrics.repayment_schedule[month - 1].effective_annual_interest_rate_pct;
    assert_relative_eq!(apr_at(1), 6.0, epsilon = 1e-12);
    assert_relative_eq!(apr_at(24), 6.0, epsilon = 1e-12);
    assert_relative_eq!(apr_at(25), 4.5, epsilon = 1e-12);
    assert_relative_eq!(apr_at(48), 4.5, epsilon = 1e-12);
    assert_relative_eq!(apr_at(49), 8.0, epsilon = 1e-12);
    assert_relative_eq!(apr_at(96), 8.0, epsilon = 1e-12);
    assert_relative_eq!(apr_at(97), 5.0, epsilon = 1e-12);
    assert_relative_eq!(apr_at(360), 5.0, epsilon = 1e-12);
}

#[test]
fn month_one_override_supersedes_base_rate() {
    let mut override_input = sample_input();
    override_input.base_annual_interest_rate_pct = 5.0;
    override_input.rate_overrides = vec![RateOverride {
        start_month: 1,
        annual_interest_rate_pct: 6.0,
    }];

    let baseline = calculate_metrics(&sample_input(), 1).expect("baseline should succeed");
    let overridden = calculate_metrics(&override_input, 1).expect("override should succeed");

    assert_relative_eq!(
        overridden.first_monthly_payment_base,
        baseline.first_monthly_payment_base,
        epsilon = 1e-9
    );
    assert_relative_eq!(
        overridden.total_interest,
        baseline.total_interest,
        epsilon = 1e-6
    );
}

#[test]
fn zero_interest_segment_is_supported() {
    let mut input = sample_input();
    input.rate_overrides = vec![
        RateOverride {
            start_month: 61,
            annual_interest_rate_pct: 0.0,
        },
        RateOverride {
            start_month: 121,
            annual_interest_rate_pct: 6.0,
        },
    ];

    let metrics = calculate_metrics(&input, 80).expect("calculation should succeed");

    assert_relative_eq!(
        metrics.selected_month_effective_rate_pct,
        0.0,
        epsilon = 1e-9
    );
    assert!(metrics.selected_monthly_payment_base > 0.0);
    assert!(metrics.total_interest > 0.0);
}

#[test]
fn rejects_invalid_override_inputs_and_selected_month() {
    let mut duplicate = sample_input();
    duplicate.rate_overrides = vec![
        RateOverride {
            start_month: 12,
            annual_interest_rate_pct: 5.0,
        },
        RateOverride {
            start_month: 12,
            annual_interest_rate_pct: 6.0,
        },
    ];

    let err = calculate_metrics(&duplicate, 1).expect_err("should reject duplicate months");
    assert_eq!(err, CalcError::DuplicateOverrideMonth(12));

    let mut month_zero = sample_input();
    month_zero.rate_overrides = vec![RateOverride {
        start_month: 0,
        annual_interest_rate_pct: 5.0,
    }];

    let err = calculate_metrics(&month_zero, 1).expect_err("should reject month 0 override");
    assert_eq!(
        err,
        CalcError::InvalidOverrideMonth {
            month: 0,
            max_month: 360
        }
    );

    let mut beyond_term = sample_input();
    beyond_term.rate_overrides = vec![RateOverride {
        start_month: 361,
        annual_interest_rate_pct: 5.0,
    }];

    let err = calculate_metrics(&beyond_term, 1).expect_err("should reject out-of-range override");
    assert_eq!(
        err,
        CalcError::InvalidOverrideMonth {
            month: 361,
            max_month: 360
        }
    );

    let mut negative_rate = sample_input();
    negative_rate.rate_overrides = vec![RateOverride {
        start_month: 12,
        annual_interest_rate_pct: -1.0,
    }];

    let err =
        calculate_metrics(&negative_rate, 1).expect_err("should reject negative override APR");
    assert_eq!(err, CalcError::InvalidOverrideRate { month: 12 });

    let mut invalid_payment_day = sample_input();
    invalid_payment_day.payment_day = 0;

    let err =
        calculate_metrics(&invalid_payment_day, 1).expect_err("should reject invalid payment day");
    assert_eq!(err, CalcError::InvalidPaymentDay);

    let err = calculate_metrics(&sample_input(), 361)
        .expect_err("should reject selected month out of range");
    assert_eq!(
        err,
        CalcError::InvalidSelectedMonth {
            month: 361,
            max_month: 360
        }
    );
}

#[test]
fn repayment_schedule_rows_sum_to_totals() {
    let input = sample_input();
    let metrics = calculate_metrics(&input, 1).expect("calculation should succeed");

    let total_paid: f64 = metrics
        .repayment_schedule
        .iter()
        .map(|row| row.total_payment)
        .sum();
    let total_interest: f64 = metrics
        .repayment_schedule
        .iter()
        .map(|row| row.interest_payment)
        .sum();
    let total_principal: f64 = metrics
        .repayment_schedule
        .iter()
        .map(|row| row.principal_payment)
        .sum();
    let total_fees: f64 = metrics
        .repayment_schedule
        .iter()
        .map(|row| row.fees_payment)
        .sum();

    assert_eq!(
        metrics
            .repayment_schedule
            .first()
            .map(|row| row.month_index),
        Some(1)
    );
    assert_eq!(
        metrics.repayment_schedule.last().map(|row| row.month_index),
        Some(360)
    );
    assert_relative_eq!(total_interest, metrics.total_interest, epsilon = 1e-6);
    assert_relative_eq!(total_fees, metrics.total_monthly_fees, epsilon = 1e-6);
    assert_relative_eq!(total_paid, metrics.total_repayment, epsilon = 1e-6);
    assert_relative_eq!(total_principal, input.loan_amount, epsilon = 1e-5);
}

#[test]
fn rounded_monthly_payments_are_ceiled_and_totals_follow_schedule_sum() {
    let mut rounded_input = sample_input();
    rounded_input.round_monthly_payment_up = true;

    let unrounded =
        calculate_metrics(&sample_input(), 1).expect("baseline calculation should succeed");
    let rounded = calculate_metrics(&rounded_input, 1).expect("rounded calculation should succeed");

    assert!(
        rounded
            .repayment_schedule
            .iter()
            .all(|row| row.total_payment.fract().abs() < 1e-9)
    );

    let rounded_total_paid: f64 = rounded
        .repayment_schedule
        .iter()
        .map(|row| row.total_payment)
        .sum();
    let rounded_total_fees: f64 = rounded
        .repayment_schedule
        .iter()
        .map(|row| row.fees_payment)
        .sum();

    assert_relative_eq!(rounded.total_repayment, rounded_total_paid, epsilon = 1e-9);
    assert_relative_eq!(
        rounded.total_monthly_fees,
        rounded_total_fees,
        epsilon = 1e-9
    );
    assert!(rounded.total_repayment > unrounded.total_repayment);
    assert_relative_eq!(
        rounded.loan_cost,
        rounded.total_paid_all_in - rounded_input.loan_amount,
        epsilon = 1e-9
    );
}
