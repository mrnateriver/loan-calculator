#[path = "../src/model.rs"]
mod model;

use approx::assert_relative_eq;
use model::{
    CalcError, DateYmd, ExtraPayment, InterestBasisMode, LoanInput, RateOverride, calculate_metrics,
};

fn sample_input() -> LoanInput {
    LoanInput {
        loan_amount: 300_000.0,
        one_time_fees: 8_000.0,
        monthly_fees: 120.0,
        round_monthly_payment_up: false,
        interest_basis_mode: InterestBasisMode::Act365Fixed,
        base_annual_interest_rate_pct: 6.0,
        term_years: 30,
        start_date: DateYmd::from_ymd_opt(2026, 9, 12).expect("valid date"),
        payment_day: 15,
        rate_overrides: vec![],
        extra_payments: vec![],
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
    input.loan_amount = 3_750_000.0;
    input.monthly_fees = 75.0;
    input.base_annual_interest_rate_pct = 4.85;
    input.start_date = DateYmd::from_ymd_opt(2027, 2, 18).expect("valid start date");
    input.payment_day = 20;
    input.round_monthly_payment_up = true;

    let metrics = calculate_metrics(&input, 1).expect("calculation should succeed");
    let first = metrics
        .repayment_schedule
        .first()
        .expect("schedule should include first payment");
    let second = &metrics.repayment_schedule[1];
    let third = &metrics.repayment_schedule[2];

    assert_relative_eq!(first.interest_payment, 14_948.0, epsilon = 1e-9);
    assert!(first.principal_payment > second.principal_payment);
    assert!(first.total_payment > second.total_payment);
    assert!(second.total_payment >= third.total_payment);
    assert_relative_eq!(
        first.total_payment,
        first.interest_payment + first.principal_payment + first.fees_payment,
        epsilon = 1e-9
    );
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
fn interest_basis_modes_change_first_period_interest_as_expected() {
    let mut input = sample_input();
    input.loan_amount = 1_000_000.0;
    input.base_annual_interest_rate_pct = 7.3;
    input.start_date = DateYmd::from_ymd_opt(2027, 12, 12).expect("valid start date");
    input.payment_day = 15;
    input.round_monthly_payment_up = false;

    let first_payment_date = DateYmd::from_ymd_opt(2028, 1, 15).expect("valid date");
    let total_days =
        (first_payment_date.days_since_epoch() - input.start_date.days_since_epoch()) as f64;
    let days_2027 = (DateYmd::from_ymd_opt(2028, 1, 1)
        .expect("valid date")
        .days_since_epoch()
        - input.start_date.days_since_epoch()) as f64;
    let days_2028 = total_days - days_2027;

    let expected_act_365 = input.loan_amount * 0.073 * (total_days / 365.0);
    let expected_act_act = input.loan_amount * 0.073 * ((days_2027 / 365.0) + (days_2028 / 366.0));
    let expected_30e_360 = input.loan_amount * 0.073 * (33.0 / 360.0);
    let expected_apr_12 = input.loan_amount * (0.073 / 12.0) * (total_days / 30.0);

    input.interest_basis_mode = InterestBasisMode::Act365Fixed;
    let act_365 = calculate_metrics(&input, 1).expect("ACT/365 should succeed");
    input.interest_basis_mode = InterestBasisMode::ActActual;
    let act_act = calculate_metrics(&input, 1).expect("ACT/ACT should succeed");
    input.interest_basis_mode = InterestBasisMode::ThirtyE360;
    let thirty_e_360 = calculate_metrics(&input, 1).expect("30E/360 should succeed");
    input.interest_basis_mode = InterestBasisMode::Apr12Monthly;
    let apr_12 = calculate_metrics(&input, 1).expect("APR/12 monthly should succeed");

    assert_relative_eq!(
        act_365.repayment_schedule[0].interest_payment,
        expected_act_365,
        epsilon = 1e-6
    );
    assert_relative_eq!(
        act_act.repayment_schedule[0].interest_payment,
        expected_act_act,
        epsilon = 1e-6
    );
    assert_relative_eq!(
        thirty_e_360.repayment_schedule[0].interest_payment,
        expected_30e_360,
        epsilon = 1e-6
    );
    assert_relative_eq!(
        apr_12.repayment_schedule[0].interest_payment,
        expected_apr_12,
        epsilon = 1e-6
    );
}

#[test]
fn single_override_changes_payment_from_its_start_month() {
    let baseline_schedule =
        calculate_metrics(&sample_input(), 61).expect("baseline schedule should succeed");
    let override_date = baseline_schedule.repayment_schedule[60].payment_date;

    let mut input = sample_input();
    input.rate_overrides.push(RateOverride {
        effective_date: override_date,
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
    let baseline = calculate_metrics(&sample_input(), 1).expect("baseline should succeed");
    let month_25_date = baseline.repayment_schedule[24].payment_date;
    let month_49_date = baseline.repayment_schedule[48].payment_date;
    let month_97_date = baseline.repayment_schedule[96].payment_date;

    let mut input = sample_input();
    input.rate_overrides = vec![
        RateOverride {
            effective_date: month_25_date,
            annual_interest_rate_pct: 4.5,
        },
        RateOverride {
            effective_date: month_49_date,
            annual_interest_rate_pct: 8.0,
        },
        RateOverride {
            effective_date: month_97_date,
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
        effective_date: override_input.start_date,
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
    let baseline = calculate_metrics(&sample_input(), 1).expect("baseline should succeed");
    let month_61_date = baseline.repayment_schedule[60].payment_date;
    let month_121_date = baseline.repayment_schedule[120].payment_date;

    let mut input = sample_input();
    input.rate_overrides = vec![
        RateOverride {
            effective_date: month_61_date,
            annual_interest_rate_pct: 0.0,
        },
        RateOverride {
            effective_date: month_121_date,
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
    let start_date = sample_input().start_date;
    let duplicate_date = DateYmd::from_ymd_opt(2026, 12, 1).expect("valid date");
    let before_start_date = DateYmd::from_ymd_opt(2026, 9, 1).expect("valid date");
    let after_last_payment_date = DateYmd::from_ymd_opt(2056, 10, 16).expect("valid date");

    let mut duplicate = sample_input();
    duplicate.rate_overrides = vec![
        RateOverride {
            effective_date: duplicate_date,
            annual_interest_rate_pct: 5.0,
        },
        RateOverride {
            effective_date: duplicate_date,
            annual_interest_rate_pct: 6.0,
        },
    ];

    let err = calculate_metrics(&duplicate, 1).expect_err("should reject duplicate dates");
    assert_eq!(err, CalcError::DuplicateOverrideDate(duplicate_date));

    let mut before_start = sample_input();
    before_start.rate_overrides = vec![RateOverride {
        effective_date: before_start_date,
        annual_interest_rate_pct: 5.0,
    }];

    let err = calculate_metrics(&before_start, 1).expect_err("should reject override before start");
    assert_eq!(
        err,
        CalcError::InvalidOverrideDate {
            date: before_start_date,
            min_date: start_date,
            max_date: DateYmd::from_ymd_opt(2056, 9, 15).expect("valid end date"),
        }
    );

    let mut beyond_term = sample_input();
    beyond_term.rate_overrides = vec![RateOverride {
        effective_date: after_last_payment_date,
        annual_interest_rate_pct: 5.0,
    }];

    let err = calculate_metrics(&beyond_term, 1).expect_err("should reject out-of-range override");
    assert_eq!(
        err,
        CalcError::InvalidOverrideDate {
            date: after_last_payment_date,
            min_date: start_date,
            max_date: DateYmd::from_ymd_opt(2056, 9, 15).expect("valid end date"),
        }
    );

    let mut negative_rate = sample_input();
    negative_rate.rate_overrides = vec![RateOverride {
        effective_date: duplicate_date,
        annual_interest_rate_pct: -1.0,
    }];

    let err =
        calculate_metrics(&negative_rate, 1).expect_err("should reject negative override APR");
    assert_eq!(
        err,
        CalcError::InvalidOverrideRate {
            date: duplicate_date
        }
    );

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
fn mid_cycle_override_splits_interest_by_days_and_apr_applies_on_payment_date() {
    let mut input = sample_input();
    let override_date = DateYmd::from_ymd_opt(2026, 10, 1).expect("valid date");
    input.rate_overrides = vec![RateOverride {
        effective_date: override_date,
        annual_interest_rate_pct: 12.0,
    }];

    let metrics = calculate_metrics(&input, 1).expect("calculation should succeed");
    let first = metrics
        .repayment_schedule
        .first()
        .expect("schedule should include first payment");

    let low_days = (override_date.days_since_epoch() - input.start_date.days_since_epoch()) as f64;
    let high_days =
        (first.payment_date.days_since_epoch() - override_date.days_since_epoch()) as f64;
    let expected_interest = input.loan_amount
        * ((input.base_annual_interest_rate_pct / 100.0) * (low_days / 365.0)
            + (12.0 / 100.0) * (high_days / 365.0));

    assert_relative_eq!(first.interest_payment, expected_interest, epsilon = 1e-6);
    assert_relative_eq!(
        first.effective_annual_interest_rate_pct,
        12.0,
        epsilon = 1e-12
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
fn rounded_mode_rounds_interest_and_principal_and_totals_follow_schedule_sum() {
    let mut rounded_input = sample_input();
    rounded_input.round_monthly_payment_up = true;

    let rounded = calculate_metrics(&rounded_input, 1).expect("rounded calculation should succeed");

    assert!(
        rounded
            .repayment_schedule
            .iter()
            .all(|row| row.interest_payment.fract().abs() < 1e-9)
    );
    assert!(
        rounded
            .repayment_schedule
            .iter()
            .all(|row| row.principal_payment.fract().abs() < 1e-9)
    );
    assert!(rounded.repayment_schedule.iter().all(|row| {
        (row.total_payment - (row.interest_payment + row.principal_payment + row.fees_payment))
            .abs()
            < 1e-9
    }));

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
    let rounded_total_interest: f64 = rounded
        .repayment_schedule
        .iter()
        .map(|row| row.interest_payment)
        .sum();

    assert_relative_eq!(rounded.total_repayment, rounded_total_paid, epsilon = 1e-9);
    assert_relative_eq!(
        rounded.total_monthly_fees,
        rounded_total_fees,
        epsilon = 1e-9
    );
    assert_relative_eq!(
        rounded.total_interest,
        rounded_total_interest,
        epsilon = 1e-9
    );
    assert_relative_eq!(
        rounded.loan_cost,
        rounded.total_paid_all_in - rounded_input.loan_amount,
        epsilon = 1e-9
    );

    let rounded_principal_total: f64 = rounded
        .repayment_schedule
        .iter()
        .map(|row| row.principal_payment)
        .sum::<f64>()
        + rounded.total_extra_payments;
    assert_relative_eq!(
        rounded_principal_total,
        rounded_input.loan_amount,
        epsilon = 1e-6
    );
}

#[test]
fn rounded_mode_uses_truncated_interest_with_carry_forward() {
    let mut input = sample_input();
    input.round_monthly_payment_up = true;
    input.interest_basis_mode = InterestBasisMode::Act365Fixed;

    let metrics = calculate_metrics(&input, 1).expect("rounded calculation should succeed");

    let mut principal = input.loan_amount;
    let mut carry = 0.0;
    let mut period_start = input.start_date;
    let rate = input.base_annual_interest_rate_pct / 100.0;

    for entry in metrics.repayment_schedule.iter().take(36) {
        let days = (entry.payment_date.days_since_epoch() - period_start.days_since_epoch()) as f64;
        let exact_interest = principal * rate * (days / 365.0);
        let posted_interest = (exact_interest + carry).trunc();
        carry = exact_interest + carry - posted_interest;

        assert_relative_eq!(entry.interest_payment, posted_interest, epsilon = 1e-9);

        principal -= entry.principal_payment;
        period_start = entry.payment_date;
    }
}

#[test]
fn extra_payment_between_due_dates_recalculates_next_scheduled_payment() {
    let baseline = calculate_metrics(&sample_input(), 2).expect("baseline should succeed");

    let mut with_extra = sample_input();
    with_extra.extra_payments = vec![ExtraPayment {
        effective_date: DateYmd::from_ymd_opt(2026, 11, 1).expect("valid date"),
        amount: 10_000.0,
    }];

    let metrics = calculate_metrics(&with_extra, 2).expect("calculation should succeed");
    let baseline_month_two = &baseline.repayment_schedule[1];
    let month_two = &metrics.repayment_schedule[1];

    assert_eq!(metrics.applied_extra_payments.len(), 1);
    assert_eq!(
        metrics.applied_extra_payments[0].effective_date,
        DateYmd::from_ymd_opt(2026, 11, 1).expect("valid date")
    );
    assert_relative_eq!(
        metrics.applied_extra_payments[0].applied_amount,
        10_000.0,
        epsilon = 1e-9
    );
    assert_relative_eq!(metrics.total_extra_payments, 10_000.0, epsilon = 1e-9);
    assert!(
        month_two.total_payment < baseline_month_two.total_payment,
        "next scheduled payment should be recalculated after extra principal prepayment"
    );
    assert!(
        month_two.interest_payment < baseline_month_two.interest_payment,
        "daily interest should drop after principal is reduced mid-cycle"
    );
}

#[test]
fn extra_payment_on_payment_date_applies_after_scheduled_payment() {
    let baseline = calculate_metrics(&sample_input(), 2).expect("baseline should succeed");
    let first_payment_date = baseline.repayment_schedule[0].payment_date;

    let mut with_extra = sample_input();
    with_extra.extra_payments = vec![ExtraPayment {
        effective_date: first_payment_date,
        amount: 5_000.0,
    }];

    let metrics = calculate_metrics(&with_extra, 2).expect("calculation should succeed");

    assert_relative_eq!(
        metrics.repayment_schedule[0].total_payment,
        baseline.repayment_schedule[0].total_payment,
        epsilon = 1e-6
    );
    assert!(
        metrics.repayment_schedule[1].total_payment < baseline.repayment_schedule[1].total_payment,
        "payment-date extra should impact following scheduled payment, not current one"
    );
    assert_relative_eq!(metrics.total_extra_payments, 5_000.0, epsilon = 1e-9);
}

#[test]
fn duplicate_extra_payment_dates_are_summed() {
    let mut input = sample_input();
    let date = DateYmd::from_ymd_opt(2026, 11, 1).expect("valid date");
    input.extra_payments = vec![
        ExtraPayment {
            effective_date: date,
            amount: 1_500.0,
        },
        ExtraPayment {
            effective_date: date,
            amount: 2_500.0,
        },
    ];

    let metrics = calculate_metrics(&input, 2).expect("calculation should succeed");
    assert_eq!(metrics.applied_extra_payments.len(), 1);
    assert_relative_eq!(
        metrics.applied_extra_payments[0].applied_amount,
        4_000.0,
        epsilon = 1e-9
    );
    assert_relative_eq!(metrics.total_extra_payments, 4_000.0, epsilon = 1e-9);
}

#[test]
fn rejects_invalid_extra_payment_inputs() {
    let mut before_start = sample_input();
    before_start.extra_payments = vec![ExtraPayment {
        effective_date: DateYmd::from_ymd_opt(2026, 9, 1).expect("valid date"),
        amount: 1_000.0,
    }];
    let err = calculate_metrics(&before_start, 1).expect_err("should reject extra payment date");
    assert_eq!(
        err,
        CalcError::InvalidExtraPaymentDate {
            date: DateYmd::from_ymd_opt(2026, 9, 1).expect("valid date"),
            min_date: before_start.start_date,
            max_date: DateYmd::from_ymd_opt(2056, 9, 15).expect("valid end date"),
        }
    );

    let mut negative = sample_input();
    let valid_date = DateYmd::from_ymd_opt(2026, 11, 1).expect("valid date");
    negative.extra_payments = vec![ExtraPayment {
        effective_date: valid_date,
        amount: -5.0,
    }];
    let err = calculate_metrics(&negative, 1).expect_err("should reject negative amount");
    assert_eq!(
        err,
        CalcError::InvalidExtraPaymentAmount { date: valid_date }
    );
}
