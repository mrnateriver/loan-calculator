use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, FieldId, RowRatePopupField, ScheduleDisplayRow};
use crate::model::LoanMetrics;

pub fn render(frame: &mut Frame, app: &mut App) {
    let [content_area, help_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(frame.area());

    let min_schedule_height = 8;
    let top_height = if content_area.height <= min_schedule_height {
        content_area.height.saturating_sub(1)
    } else {
        let candidate = content_area.height.saturating_mul(38) / 100;
        candidate.clamp(8, content_area.height - min_schedule_height)
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

    if app.is_row_rate_popup_open {
        render_row_rate_popup(frame, app);
    }
}

fn render_form(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::with_capacity(FieldId::ALL.len() + 5);

    for field in FieldId::ALL {
        let is_active = field == app.active_field() && !app.is_row_rate_popup_open;
        let marker = if is_active { ">" } else { " " };
        let value = app.field_display_value(field);

        let style = if is_active {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        lines.push(Line::styled(
            format!("{} {:<28} {}", marker, format!("{}:", field.label()), value),
            style,
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "Rate Overrides: {}",
        app.override_count()
    )));
    lines.push(Line::from(format!(
        "Selected Row Month: M{} ({})",
        app.selected_month,
        app.format_schedule_month(app.selected_month)
    )));

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
        Some(metrics) => summary_lines(metrics),
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
                "{:<10} {:>8} {:>12} {:>12} {:>12} {:>10}",
                "Date", "APR(%)", "Payment", "Interest", "Principal", "Fees"
            ),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

        for (visible_idx, row) in app.schedule_rows[offset..end].iter().enumerate() {
            let absolute_idx = offset + visible_idx;
            let is_selected = absolute_idx == app.schedule_selected_index;
            let line = match row {
                ScheduleDisplayRow::Payment { schedule_index, .. } => {
                    let entry = &metrics.repayment_schedule[*schedule_index];
                    format!(
                        "{:<10} {:>8} {:>12} {:>12} {:>12} {:>10}",
                        entry.payment_date.format_yyyy_mm_dd(),
                        format_rate(entry.effective_annual_interest_rate_pct),
                        money(entry.total_payment),
                        money(entry.interest_payment),
                        money(entry.principal_payment),
                        money(entry.fees_payment),
                    )
                }
                ScheduleDisplayRow::AprChangeMarker {
                    effective_date,
                    annual_interest_rate_pct,
                    ..
                } => {
                    format!(
                        "{:<10} {:>8} {:>12} {:>12} {:>12} {:>10}",
                        effective_date.format_yyyy_mm_dd(),
                        format_rate(*annual_interest_rate_pct),
                        "",
                        "",
                        "",
                        "",
                    )
                }
            };

            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
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
    let main_help = if app.is_row_rate_popup_open {
        "APR popup: tab/up/down switch field | type | backspace | enter apply | d clear | esc cancel"
    } else {
        "up/down/j/k: navigate | tab/shift+tab: switch panels | enter on schedule: edit APR | space/enter: toggle | r: reset | q: quit"
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

fn render_row_rate_popup(frame: &mut Frame, app: &App) {
    let popup_area = centered_rect(58, 34, frame.area());
    frame.render_widget(Clear, popup_area);

    let selected_row = app.selected_schedule_row();
    let row_date = selected_row.map(|row| row.date());
    let row_date_display = row_date
        .map(|date| date.format_yyyy_mm_dd())
        .unwrap_or_else(|| "--".to_string());
    let selected_row_label = match selected_row {
        Some(ScheduleDisplayRow::Payment { month_index, .. }) => {
            let month_label = app.format_schedule_month(month_index);
            format!("Payment M{month_index} ({month_label})")
        }
        Some(ScheduleDisplayRow::AprChangeMarker { target_month, .. }) => {
            format!("APR Change (targets M{target_month})")
        }
        None => "None".to_string(),
    };

    let effective_display = row_date
        .and_then(|date| app.effective_rate_for_date(date))
        .map(format_rate)
        .unwrap_or_else(|| "--".to_string());
    let override_display = row_date
        .and_then(|date| app.override_for_date(date))
        .map(format_rate)
        .unwrap_or_else(|| "--".to_string());

    let date_marker = if app.row_rate_popup_active_field == RowRatePopupField::EffectiveDate {
        ">"
    } else {
        " "
    };
    let apr_marker = if app.row_rate_popup_active_field == RowRatePopupField::Apr {
        ">"
    } else {
        " "
    };
    let date_input_display = if app.row_rate_date_input_buffer.is_empty() {
        "<empty>".to_string()
    } else {
        app.row_rate_date_input_buffer.clone()
    };
    let apr_input_display = if app.row_rate_apr_input_buffer.is_empty() {
        "<empty>".to_string()
    } else {
        app.row_rate_apr_input_buffer.clone()
    };

    let popup_lines = vec![
        Line::from(format!("Selected Row:   {selected_row_label}")),
        Line::from(format!("Selected Date:  {row_date_display}")),
        Line::from(format!("Effective APR:  {effective_display}%")),
        Line::from(format!("Override APR:   {override_display}%")),
        Line::from(""),
        Line::from(format!(
            "{date_marker} Effective Date Input: {date_input_display}"
        )),
        Line::from(format!(
            "{apr_marker} APR Input:            {apr_input_display}"
        )),
        Line::from(""),
        Line::styled(
            "Tab/Up/Down switch field | Enter apply | d clear | Esc cancel",
            Style::default().fg(Color::DarkGray),
        ),
    ];

    let popup = Paragraph::new(Text::from(popup_lines))
        .block(
            Block::default()
                .title(" APR Override For Selected Row ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(popup, popup_area);
}

fn summary_lines(metrics: &LoanMetrics) -> Vec<Line<'static>> {
    let next_change = match (
        metrics.next_change_month,
        metrics.next_change_monthly_payment_base,
    ) {
        (Some(month), Some(payment)) => format!("M{month} -> {}", money(payment)),
        _ => "None".to_string(),
    };

    vec![
        Line::from(format!(
            "First Monthly Payment:            {}",
            money(metrics.first_monthly_payment_base)
        )),
        Line::from(format!(
            "Selected Month:                   M{}",
            metrics.selected_month
        )),
        Line::from(format!(
            "Selected Month APR:               {:.3}%",
            metrics.selected_month_effective_rate_pct
        )),
        Line::from(format!(
            "Payment at Selected Month:        {}",
            money(metrics.selected_monthly_payment_base)
        )),
        Line::from(format!(
            "Effective Monthly (+fees):        {}",
            money(metrics.selected_monthly_payment_with_fees)
        )),
        Line::from(format!("Next Change:                      {}", next_change)),
        Line::from(format!(
            "Total Interest:                   {}",
            money(metrics.total_interest)
        )),
        Line::from(format!(
            "Total Repayment:                  {}",
            money(metrics.total_repayment)
        )),
        Line::from(format!(
            "Loan Cost:                        {}",
            money(metrics.loan_cost)
        )),
    ]
}

fn money(value: f64) -> String {
    format!("{value:.2}")
}

fn format_rate(value: f64) -> String {
    format!("{value:.3}")
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

fn centered_rect(width_percent: u16, height_percent: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height_percent) / 2),
        Constraint::Percentage(height_percent),
        Constraint::Percentage((100 - height_percent) / 2),
    ])
    .flex(Flex::Center)
    .split(area);

    let horizontal = Layout::horizontal([
        Constraint::Percentage((100 - width_percent) / 2),
        Constraint::Percentage(width_percent),
        Constraint::Percentage((100 - width_percent) / 2),
    ])
    .flex(Flex::Center)
    .split(vertical[1]);

    horizontal[1]
}
