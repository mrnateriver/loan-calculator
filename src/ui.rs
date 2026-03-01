use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, FieldId, ResetConfirmOption, RowActionOption, ScheduleDisplayRow};
use crate::model::{DateYmd, LoanMetrics};

pub fn render(frame: &mut Frame, app: &mut App) {
    let [content_area, help_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(frame.area());

    let top_height = if content_area.height <= 3 {
        content_area.height
    } else {
        // One line per input field + 2 border rows.
        ((FieldId::ALL.len() as u16) + 2).min(content_area.height - 3)
    };

    let [top_area, schedule_area] =
        Layout::vertical([Constraint::Length(top_height), Constraint::Min(3)]).areas(content_area);

    let [form_area, summary_area] =
        Layout::horizontal([Constraint::Percentage(44), Constraint::Percentage(56)])
            .areas(top_area);

    render_form(frame, app, form_area);
    render_summary(frame, app, summary_area);
    render_schedule(frame, app, schedule_area);
    render_help(frame, app, help_area);

    if app.is_reset_confirm_popup_open {
        render_reset_confirm_popup(frame, app);
    } else if app.is_row_action_popup_open() {
        render_row_action_popup(frame, app);
    } else if app.is_apr_edit_popup_open() {
        render_apr_edit_popup(frame, app);
    } else if app.is_extra_edit_popup_open() {
        render_extra_edit_popup(frame, app);
    } else if app.is_recurring_extra_edit_popup_open() {
        render_recurring_extra_edit_popup(frame, app);
    } else if app.is_interest_basis_popup_open {
        render_interest_basis_popup(frame, app);
    }
}

fn render_form(frame: &mut Frame, app: &App, area: Rect) {
    let row_width = area.width.saturating_sub(2) as usize;
    let mut lines = Vec::with_capacity(FieldId::ALL.len());

    for field in FieldId::ALL {
        let is_active = field == app.active_field() && !app.is_any_popup_open();
        let value = app.field_display_value(field);
        let row_text = format!("{:<28} {}", format!("{}:", field.label()), value);
        let padded_row = format!("{:<width$}", row_text, width = row_width);

        let style = if is_active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        lines.push(Line::styled(padded_row, style));
    }

    let form = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(" Inputs ")
                .borders(Borders::ALL)
                .border_style(panel_border_style(Color::Cyan, !app.is_schedule_focused())),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(form, area);
}

fn render_summary(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match app.metrics.as_ref() {
        Some(metrics) => summary_lines(metrics, app.round_payments_up),
        None => vec![Line::from("Press Enter after filling valid values.")],
    };

    let summary = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(" Summary ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(summary, area);
}

fn render_schedule(frame: &mut Frame, app: &mut App, area: Rect) {
    let schedule_block = Block::default()
        .title(" Repayment Schedule ")
        .borders(Borders::ALL)
        .border_style(panel_border_style(Color::Blue, app.is_schedule_focused()));

    let inner = schedule_block.inner(area);
    frame.render_widget(schedule_block, area);

    if inner.height < 3 {
        app.set_schedule_viewport_rows(1);
        return;
    }

    let data_rows_available = inner.height.saturating_sub(3) as usize;
    app.set_schedule_viewport_rows(data_rows_available.max(1));

    let mut lines = Vec::new();

    if let Some(metrics) = app.metrics.as_ref() {
        let total_rows = app.schedule_rows.len();

        let offset = if total_rows == 0 {
            0
        } else {
            app.schedule_scroll_offset.min(total_rows.saturating_sub(1))
        };
        let end = (offset + data_rows_available).min(total_rows);

        lines.push(Line::styled(
            format!(
                "{:<10} {:>8} {:>12} {:>12} {:>12} {:>10} {:>12}",
                "Date", "APR(%)", "Payment", "Interest", "Principal", "Fees", "Saldo"
            ),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

        let mut running_balance = metrics.purchase_price_estimate;
        let mut balance_after_row = Vec::with_capacity(total_rows);
        for row in &app.schedule_rows {
            let principal_delta = match row {
                ScheduleDisplayRow::Payment { schedule_index, .. } => {
                    metrics.repayment_schedule[*schedule_index].principal_payment
                }
                ScheduleDisplayRow::ExtraPaymentMarker { amount, .. }
                | ScheduleDisplayRow::RecurringExtraPaymentMarker { amount, .. } => *amount,
                ScheduleDisplayRow::AprChangeMarker { .. }
                | ScheduleDisplayRow::YearSummary { .. } => 0.0,
            };

            if principal_delta > 0.0 {
                running_balance = (running_balance - principal_delta).max(0.0);
                if running_balance.abs() < 1e-9 {
                    running_balance = 0.0;
                }
            }
            balance_after_row.push(running_balance);
        }

        for (visible_idx, row) in app.schedule_rows[offset..end].iter().enumerate() {
            let absolute_idx = offset + visible_idx;
            let is_selected = absolute_idx == app.schedule_selected_index;
            let saldo_text = if matches!(row, ScheduleDisplayRow::AprChangeMarker { .. }) {
                String::new()
            } else {
                money(balance_after_row[absolute_idx], app.round_payments_up)
            };
            let line = match row {
                ScheduleDisplayRow::Payment { schedule_index, .. } => {
                    let entry = &metrics.repayment_schedule[*schedule_index];
                    format!(
                        "{:<10} {:>8} {:>12} {:>12} {:>12} {:>10} {:>12}",
                        entry.payment_date.format_yyyy_mm_dd(),
                        format_rate(entry.effective_annual_interest_rate_pct),
                        money(entry.total_payment, app.round_payments_up),
                        money(entry.interest_payment, app.round_payments_up),
                        money(entry.principal_payment, app.round_payments_up),
                        money(entry.fees_payment, app.round_payments_up),
                        saldo_text,
                    )
                }
                ScheduleDisplayRow::AprChangeMarker {
                    effective_date,
                    annual_interest_rate_pct,
                    ..
                } => {
                    format!(
                        "{:<10} {:>8} {:>12} {:>12} {:>12} {:>10} {:>12}",
                        effective_date.format_yyyy_mm_dd(),
                        format_rate(*annual_interest_rate_pct),
                        "",
                        "",
                        "",
                        "",
                        "",
                    )
                }
                ScheduleDisplayRow::ExtraPaymentMarker {
                    effective_date,
                    amount,
                    ..
                } => {
                    format!(
                        "{:<10} {:>8} {:>12} {:>12} {:>12} {:>10} {:>12}",
                        effective_date.format_yyyy_mm_dd(),
                        "",
                        "",
                        "",
                        money(*amount, app.round_payments_up),
                        "",
                        saldo_text,
                    )
                }
                ScheduleDisplayRow::RecurringExtraPaymentMarker {
                    effective_date,
                    amount,
                    ..
                } => {
                    format!(
                        "{:<10} {:>8} {:>12} {:>12} {:>12} {:>10} {:>12}",
                        effective_date.format_yyyy_mm_dd(),
                        "",
                        "",
                        "",
                        money(*amount, app.round_payments_up),
                        "",
                        saldo_text,
                    )
                }
                ScheduleDisplayRow::YearSummary {
                    year,
                    payment_sum,
                    interest_sum,
                    principal_sum,
                    fees_sum,
                    saldo_after_december_payment,
                    ..
                } => {
                    format!(
                        "{:<10} {:>8} {:>12} {:>12} {:>12} {:>10} {:>12}",
                        format!("Y{year} Sum"),
                        "",
                        money(*payment_sum, app.round_payments_up),
                        money(*interest_sum, app.round_payments_up),
                        money(*principal_sum, app.round_payments_up),
                        money(*fees_sum, app.round_payments_up),
                        money(*saldo_after_december_payment, app.round_payments_up),
                    )
                }
            };
            let line = if matches!(row, ScheduleDisplayRow::RecurringExtraPaymentMarker { .. }) {
                format!("{line} *")
            } else {
                line
            };

            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if matches!(row, ScheduleDisplayRow::YearSummary { .. }) {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else if matches!(
                row,
                ScheduleDisplayRow::ExtraPaymentMarker { .. }
                    | ScheduleDisplayRow::RecurringExtraPaymentMarker { .. }
            ) {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if let ScheduleDisplayRow::AprChangeMarker {
                effective_date,
                annual_interest_rate_pct,
                ..
            } = row
            {
                let is_higher = app
                    .prior_effective_rate_before_date(*effective_date)
                    .map(|prev| *annual_interest_rate_pct > prev)
                    .unwrap_or(false);
                let color = if is_higher { Color::Red } else { Color::Green };
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            lines.push(Line::styled(line, style));
        }

        let showing_start = if total_rows == 0 { 0 } else { offset + 1 };
        let showing_end = if total_rows == 0 { 0 } else { end };
        lines.push(Line::from(""));
        lines.push(Line::styled(
            format!(
                "Rows {showing_start}-{showing_end} of {total_rows} | Selected M{}",
                app.selected_month
            ),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        lines.push(Line::from(
            "Repayment schedule will appear after calculation.",
        ));
        lines.push(Line::from(""));
        lines.push(Line::styled(
            format!("Selected M{}", app.selected_month),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let schedule = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
    frame.render_widget(schedule, inner);
}

fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let main_help = if app.is_reset_confirm_popup_open {
        "Reset confirm: up/down/j/k select | enter confirm | esc cancel"
    } else if app.is_row_action_popup_open() {
        "Row action: up/down/j/k select | enter choose | esc cancel"
    } else if app.is_apr_edit_popup_open() {
        "APR dialog: up/down/j/k navigate | type | backspace | enter activate row | esc cancel"
    } else if app.is_extra_edit_popup_open() {
        "Extra dialog: up/down/j/k navigate | type | backspace | enter activate row | esc cancel"
    } else if app.is_recurring_extra_edit_popup_open() {
        "Recurring dialog: up/down/j/k navigate | type | backspace | enter activate row | esc cancel"
    } else if app.is_interest_basis_popup_open {
        "Interest basis: up/down/j/k select | enter apply | esc cancel"
    } else {
        "up/down/j/k: navigate | tab/shift+tab: switch panels | enter on schedule: edit row | space/enter: toggle/select | r: reset | q: quit"
    };

    let mut lines = vec![Line::from(main_help)];

    if let Some(error) = &app.error {
        lines.push(Line::styled(
            format!("Error: {error}"),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }

    let help = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title(" Help ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta)),
    );

    frame.render_widget(help, area);
}

fn render_reset_confirm_popup(frame: &mut Frame, app: &App) {
    let options = ResetConfirmOption::ALL;
    let list_height = options.len() as u16 + 2;
    let footer_height = 4;
    let popup_area = centered_rect_exact(48, list_height + footer_height, frame.area());
    frame.render_widget(Clear, popup_area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Length(list_height),
        Constraint::Length(footer_height),
    ])
    .areas(popup_area);

    let row_width = list_area.width.saturating_sub(2) as usize;
    let mut rows = Vec::with_capacity(options.len());
    for (idx, option) in options.iter().enumerate() {
        let text = format!("{:<width$}", option.label(), width = row_width);
        let style = if idx == app.reset_confirm_selected_index {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        rows.push(Line::styled(text, style));
    }

    let list = Paragraph::new(Text::from(rows))
        .block(
            Block::default()
                .title(" Confirm Reset ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(list, list_area);

    let selected = app.reset_confirm_selected_option();
    let footer = Paragraph::new(Text::from(vec![
        Line::from("Reset all inputs and schedule adjustments?"),
        Line::from(selected.description()),
    ]))
    .block(
        Block::default()
            .title(" Selected Option ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, footer_area);
}

fn render_row_action_popup(frame: &mut Frame, app: &App) {
    let options = RowActionOption::ALL;
    let list_height = options.len() as u16 + 2;
    let footer_height = 4;
    let popup_area = centered_rect_exact(44, list_height + footer_height, frame.area());
    frame.render_widget(Clear, popup_area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Length(list_height),
        Constraint::Length(footer_height),
    ])
    .areas(popup_area);

    let row_width = list_area.width.saturating_sub(2) as usize;
    let mut rows = Vec::with_capacity(options.len());
    for (idx, option) in options.iter().enumerate() {
        let text = format!("{:<width$}", option.label(), width = row_width);
        let style = if idx == app.row_action_selected_index {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        rows.push(Line::styled(text, style));
    }

    let list = Paragraph::new(Text::from(rows))
        .block(
            Block::default()
                .title(" Row Action ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(list, list_area);

    let selected_option = app.row_action_selected_option();
    let footer = Paragraph::new(Text::from(vec![Line::from(selected_option.description())]))
        .block(
            Block::default()
                .title(" Selected Option ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, footer_area);
}

fn render_apr_edit_popup(frame: &mut Frame, app: &App) {
    let rows = [
        format!(
            "Effective Date: {}",
            input_or_empty(&app.apr_edit_date_input_buffer)
        ),
        format!(
            "APR (%): {}",
            input_or_empty(&app.apr_edit_apr_input_buffer)
        ),
        "Apply APR change".to_string(),
        "Clear APR change".to_string(),
        "Cancel".to_string(),
    ];
    let list_height = rows.len() as u16 + 2;
    let footer_height = 4;
    let popup_area = centered_rect_exact(62, list_height + footer_height, frame.area());
    frame.render_widget(Clear, popup_area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Length(list_height),
        Constraint::Length(footer_height),
    ])
    .areas(popup_area);

    let row_width = list_area.width.saturating_sub(2) as usize;
    let mut lines = Vec::with_capacity(rows.len());
    for (idx, row_text) in rows.iter().enumerate() {
        let text = format!("{:<width$}", row_text, width = row_width);
        let style = if idx == app.apr_edit_active_row {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::styled(text, style));
    }

    let list = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(" APR Change ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(list, list_area);

    let footer_hint = match app.apr_edit_active_row {
        0 => "Enter date in YYYY-MM-DD.",
        1 => "Enter APR value in percent (e.g. 4.99).",
        2 => "Apply APR change at this date.",
        3 => "Remove APR change at this date.",
        _ => "Close dialog without changes.",
    };

    let apr_info = DateYmd::parse_yyyy_mm_dd(app.apr_edit_date_input_buffer.trim())
        .map(|date| {
            let effective = app
                .effective_rate_for_date(date)
                .map(format_rate)
                .unwrap_or_else(|| "--".to_string());
            let override_rate = app
                .override_for_date(date)
                .map(format_rate)
                .unwrap_or_else(|| "--".to_string());
            format!("Effective APR: {effective}% | Override: {override_rate}%")
        })
        .unwrap_or_else(|| "Effective APR: -- | Override: --".to_string());

    let footer = Paragraph::new(Text::from(vec![
        Line::from(footer_hint),
        Line::from(apr_info),
    ]))
    .block(
        Block::default()
            .title(" Details ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, footer_area);
}

fn render_extra_edit_popup(frame: &mut Frame, app: &App) {
    let rows = [
        format!(
            "Effective Date: {}",
            input_or_empty(&app.extra_edit_date_input_buffer)
        ),
        format!(
            "Extra Payment: {}",
            input_or_empty(&app.extra_edit_amount_input_buffer)
        ),
        "Apply extra payment".to_string(),
        "Clear extra payment".to_string(),
        "Cancel".to_string(),
    ];
    let list_height = rows.len() as u16 + 2;
    let footer_height = 4;
    let popup_area = centered_rect_exact(62, list_height + footer_height, frame.area());
    frame.render_widget(Clear, popup_area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Length(list_height),
        Constraint::Length(footer_height),
    ])
    .areas(popup_area);

    let row_width = list_area.width.saturating_sub(2) as usize;
    let mut lines = Vec::with_capacity(rows.len());
    for (idx, row_text) in rows.iter().enumerate() {
        let text = format!("{:<width$}", row_text, width = row_width);
        let style = if idx == app.extra_edit_active_row {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::styled(text, style));
    }

    let list = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(" Extra Payment ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(list, list_area);

    let footer_hint = match app.extra_edit_active_row {
        0 => "Enter date in YYYY-MM-DD.",
        1 => "Enter payment amount (0 removes existing value).",
        2 => "Apply extra payment for this date.",
        3 => "Remove extra payment at this date.",
        _ => "Close dialog without changes.",
    };

    let extra_info = DateYmd::parse_yyyy_mm_dd(app.extra_edit_date_input_buffer.trim())
        .map(|date| {
            let existing = app
                .extra_payment_for_date(date)
                .map(|value| money(value, app.round_payments_up))
                .unwrap_or_else(|| "--".to_string());
            format!("Existing extra payment: {existing}")
        })
        .unwrap_or_else(|| "Existing extra payment: --".to_string());

    let footer = Paragraph::new(Text::from(vec![
        Line::from(footer_hint),
        Line::from(extra_info),
    ]))
    .block(
        Block::default()
            .title(" Details ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, footer_area);
}

fn render_interest_basis_popup(frame: &mut Frame, app: &App) {
    let total_options = crate::model::InterestBasisMode::ALL.len() as u16;
    let list_height = total_options + 2;
    let footer_height = 4;
    let popup_height = list_height + footer_height;
    let popup_area = centered_rect_exact(56, popup_height, frame.area());
    frame.render_widget(Clear, popup_area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Length(list_height),
        Constraint::Length(footer_height),
    ])
    .areas(popup_area);

    let row_width = list_area.width.saturating_sub(2) as usize;
    let mut rows = Vec::with_capacity(crate::model::InterestBasisMode::ALL.len());

    for (idx, mode) in crate::model::InterestBasisMode::ALL.iter().enumerate() {
        let text = format!("{:<width$}", mode.label(), width = row_width);
        let style = if idx == app.interest_basis_popup_selected_index {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        rows.push(Line::styled(text, style));
    }

    let list = Paragraph::new(Text::from(rows))
        .block(
            Block::default()
                .title(" Interest Basis ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(list, list_area);

    let selected_mode = app.interest_basis_popup_selected_mode();
    let footer = Paragraph::new(Text::from(vec![Line::from(selected_mode.description())]))
        .block(
            Block::default()
                .title(" Selected Option ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, footer_area);
}

fn render_recurring_extra_edit_popup(frame: &mut Frame, app: &App) {
    let rows = [
        format!(
            "Start Date: {}",
            input_or_empty(&app.recurring_edit_start_date_input_buffer)
        ),
        format!(
            "Annual Date (MM-DD): {}",
            input_or_empty(&app.recurring_edit_annual_date_input_buffer)
        ),
        format!(
            "Recurring Extra Payment: {}",
            input_or_empty(&app.recurring_edit_amount_input_buffer)
        ),
        "Apply recurring extra payment".to_string(),
        "Clear recurring extra payment".to_string(),
        "Cancel".to_string(),
    ];
    let list_height = rows.len() as u16 + 2;
    let footer_height = 4;
    let popup_area = centered_rect_exact(66, list_height + footer_height, frame.area());
    frame.render_widget(Clear, popup_area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Length(list_height),
        Constraint::Length(footer_height),
    ])
    .areas(popup_area);

    let row_width = list_area.width.saturating_sub(2) as usize;
    let mut lines = Vec::with_capacity(rows.len());
    for (idx, row_text) in rows.iter().enumerate() {
        let text = format!("{:<width$}", row_text, width = row_width);
        let style = if idx == app.recurring_edit_active_row {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::styled(text, style));
    }

    let list = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(" Recurring Extra Payment ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(list, list_area);

    let footer_hint = match app.recurring_edit_active_row {
        0 => "Enter start date in YYYY-MM-DD.",
        1 => "Enter annual month/day in MM-DD format.",
        2 => "Enter recurring payment amount (0 removes existing value).",
        3 => "Apply recurring extra payment rule.",
        4 => "Remove recurring extra payment rule.",
        _ => "Close dialog without changes.",
    };

    let footer = Paragraph::new(Text::from(vec![Line::from(footer_hint)]))
        .block(
            Block::default()
                .title(" Details ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, footer_area);
}

fn summary_lines(metrics: &LoanMetrics, hide_zero_fraction: bool) -> Vec<Line<'static>> {
    let next_change = match (
        metrics.next_change_month,
        metrics.next_change_monthly_payment_base,
    ) {
        (Some(month), Some(payment)) => {
            format!("M{month} -> {}", money(payment, hide_zero_fraction))
        }
        _ => "None".to_string(),
    };

    vec![
        Line::from(format!("Next Change:                      {}", next_change)),
        Line::from(format!(
            "Total Interest:                   {}",
            money(metrics.total_interest, hide_zero_fraction)
        )),
        Line::from(format!(
            "Total Extra Payments:             {}",
            money(metrics.total_extra_payments, hide_zero_fraction)
        )),
        Line::from(format!(
            "Total Repayment:                  {}",
            money(metrics.total_repayment, hide_zero_fraction)
        )),
        Line::from(format!(
            "Loan Cost:                        {}",
            money(metrics.loan_cost, hide_zero_fraction)
        )),
    ]
}

fn money(value: f64, hide_zero_fraction: bool) -> String {
    let formatted = format!("{value:.2}");
    let (sign, unsigned) = match formatted.strip_prefix('-') {
        Some(stripped) => ("-", stripped),
        None => ("", formatted.as_str()),
    };

    let (integer_part, fractional_part) = match unsigned.split_once('.') {
        Some((int_part, frac_part)) => (int_part, Some(frac_part)),
        None => (unsigned, None),
    };
    let grouped_integer = group_triads(integer_part);

    if hide_zero_fraction && fractional_part == Some("00") {
        format!("{sign}{grouped_integer}")
    } else if let Some(frac_part) = fractional_part {
        format!("{sign}{grouped_integer}.{frac_part}")
    } else {
        format!("{sign}{grouped_integer}")
    }
}

fn group_triads(digits: &str) -> String {
    if digits.len() <= 3 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return digits.to_string();
    }

    let len = digits.len();
    let mut out = String::with_capacity(len + ((len - 1) / 3));
    let mut first_group_len = len % 3;
    if first_group_len == 0 {
        first_group_len = 3;
    }

    out.push_str(&digits[..first_group_len]);
    let mut idx = first_group_len;
    while idx < len {
        out.push(' ');
        out.push_str(&digits[idx..idx + 3]);
        idx += 3;
    }

    out
}

fn format_rate(value: f64) -> String {
    format!("{value:.3}")
}

fn input_or_empty(value: &str) -> String {
    if value.trim().is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    }
}

fn panel_border_style(base_color: Color, is_active: bool) -> Style {
    if is_active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(base_color)
    }
}

fn centered_rect_exact(width: u16, height: u16, area: Rect) -> Rect {
    let popup_width = width.min(area.width).max(1);
    let popup_height = height.min(area.height).max(1);
    let x = area.x + area.width.saturating_sub(popup_width) / 2;
    let y = area.y + area.height.saturating_sub(popup_height) / 2;
    Rect::new(x, y, popup_width, popup_height)
}
