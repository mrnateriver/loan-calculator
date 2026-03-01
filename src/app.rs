#[cfg(not(test))]
use std::fs;
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::model::{
    AppliedExtraPaymentSource, DateYmd, ExtraPayment, InterestBasisMode, LoanInput, LoanMetrics,
    RateOverride, RecurringExtraPayment, calculate_metrics,
};

const TEXT_FIELD_COUNT: usize = 7;
const FIELD_COUNT: usize = 9;
#[cfg(not(test))]
const STATE_FILE_PATH: &str = ".loan-calculator.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    LoanAmount,
    OneTimeFees,
    MonthlyFees,
    InterestRate,
    TermYears,
    StartDate,
    PaymentDay,
    InterestBasis,
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
        FieldId::InterestBasis,
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
            FieldId::InterestBasis => "Interest Basis",
            FieldId::RoundPaymentsUp => "Integer Payments",
        }
    }

    pub fn is_integer(self) -> bool {
        matches!(self, FieldId::TermYears | FieldId::PaymentDay)
    }

    pub fn is_date(self) -> bool {
        matches!(self, FieldId::StartDate)
    }

    pub fn is_text_input(self) -> bool {
        !matches!(self, FieldId::InterestBasis | FieldId::RoundPaymentsUp)
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
            FieldId::InterestBasis | FieldId::RoundPaymentsUp => {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowEditPopupMode {
    None,
    ActionSelect,
    AprEdit,
    ExtraEdit,
    RecurringExtraEdit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowActionOption {
    AddExtraPayment,
    AddAprChange,
    AddRecurringExtraPayment,
}

impl RowActionOption {
    pub const ALL: [RowActionOption; 3] = [
        RowActionOption::AddExtraPayment,
        RowActionOption::AddAprChange,
        RowActionOption::AddRecurringExtraPayment,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RowActionOption::AddExtraPayment => "Add extra payment",
            RowActionOption::AddAprChange => "Add APR change",
            RowActionOption::AddRecurringExtraPayment => "Add recurring extra payment",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            RowActionOption::AddExtraPayment => {
                "Create or edit an extra principal payment on a specific date."
            }
            RowActionOption::AddAprChange => {
                "Create or edit an APR override that takes effect from a specific date."
            }
            RowActionOption::AddRecurringExtraPayment => {
                "Create or edit a yearly recurring extra principal payment."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetConfirmOption {
    Cancel,
    ConfirmReset,
}

impl ResetConfirmOption {
    pub const ALL: [ResetConfirmOption; 2] =
        [ResetConfirmOption::Cancel, ResetConfirmOption::ConfirmReset];

    pub fn label(self) -> &'static str {
        match self {
            ResetConfirmOption::Cancel => "Cancel",
            ResetConfirmOption::ConfirmReset => "Reset all data",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ResetConfirmOption::Cancel => "Close dialog and keep current inputs and schedule.",
            ResetConfirmOption::ConfirmReset => {
                "Reset all inputs, APR overrides, extra payments, and selections."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScheduleDisplayRow {
    Payment {
        schedule_index: usize,
        month_index: u32,
        payment_date: DateYmd,
    },
    AprChangeMarker {
        effective_date: DateYmd,
        annual_interest_rate_pct: f64,
        target_month: u32,
    },
    ExtraPaymentMarker {
        effective_date: DateYmd,
        amount: f64,
        target_month: u32,
    },
    RecurringExtraPaymentMarker {
        effective_date: DateYmd,
        amount: f64,
        target_month: u32,
        recurring_start_date: DateYmd,
        recurring_month: u32,
        recurring_day: u32,
    },
}

impl ScheduleDisplayRow {
    pub fn date(self) -> DateYmd {
        match self {
            ScheduleDisplayRow::Payment { payment_date, .. } => payment_date,
            ScheduleDisplayRow::AprChangeMarker { effective_date, .. } => effective_date,
            ScheduleDisplayRow::ExtraPaymentMarker { effective_date, .. } => effective_date,
            ScheduleDisplayRow::RecurringExtraPaymentMarker { effective_date, .. } => {
                effective_date
            }
        }
    }

    pub fn target_month(self) -> u32 {
        match self {
            ScheduleDisplayRow::Payment { month_index, .. } => month_index,
            ScheduleDisplayRow::AprChangeMarker { target_month, .. } => target_month,
            ScheduleDisplayRow::ExtraPaymentMarker { target_month, .. } => target_month,
            ScheduleDisplayRow::RecurringExtraPaymentMarker { target_month, .. } => target_month,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RecurringExtraPaymentKey {
    start_date: DateYmd,
    month: u32,
    day: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScheduleRowSelectionPreference {
    Payment,
    Apr,
    Extra,
    Recurring,
}

#[derive(Debug, Clone)]
pub struct App {
    pub inputs: [String; TEXT_FIELD_COUNT],
    pub metrics: Option<LoanMetrics>,
    pub error: Option<String>,
    pub row_edit_popup_mode: RowEditPopupMode,
    pub row_action_selected_index: usize,
    pub apr_edit_date_input_buffer: String,
    pub apr_edit_apr_input_buffer: String,
    pub apr_edit_active_row: usize,
    pub extra_edit_date_input_buffer: String,
    pub extra_edit_amount_input_buffer: String,
    pub extra_edit_active_row: usize,
    pub recurring_edit_start_date_input_buffer: String,
    pub recurring_edit_annual_date_input_buffer: String,
    pub recurring_edit_amount_input_buffer: String,
    pub recurring_edit_active_row: usize,
    recurring_edit_source_key: Option<RecurringExtraPaymentKey>,
    pub is_reset_confirm_popup_open: bool,
    pub reset_confirm_selected_index: usize,
    pub is_interest_basis_popup_open: bool,
    pub interest_basis_popup_selected_index: usize,
    pub selected_month: u32,
    pub round_payments_up: bool,
    pub interest_basis_mode: InterestBasisMode,
    pub focus_area: FocusArea,
    pub schedule_rows: Vec<ScheduleDisplayRow>,
    pub schedule_selected_index: usize,
    pub schedule_scroll_offset: usize,
    active_field_idx: usize,
    schedule_viewport_rows: usize,
    rate_overrides: BTreeMap<DateYmd, f64>,
    extra_payments: BTreeMap<DateYmd, f64>,
    recurring_extra_payments: BTreeMap<RecurringExtraPaymentKey, f64>,
}

impl Default for App {
    fn default() -> Self {
        let mut app = Self {
            inputs: default_inputs(),
            metrics: None,
            error: None,
            row_edit_popup_mode: RowEditPopupMode::None,
            row_action_selected_index: 0,
            apr_edit_date_input_buffer: String::new(),
            apr_edit_apr_input_buffer: String::new(),
            apr_edit_active_row: 0,
            extra_edit_date_input_buffer: String::new(),
            extra_edit_amount_input_buffer: String::new(),
            extra_edit_active_row: 0,
            recurring_edit_start_date_input_buffer: String::new(),
            recurring_edit_annual_date_input_buffer: String::new(),
            recurring_edit_amount_input_buffer: String::new(),
            recurring_edit_active_row: 0,
            recurring_edit_source_key: None,
            is_reset_confirm_popup_open: false,
            reset_confirm_selected_index: 0,
            is_interest_basis_popup_open: false,
            interest_basis_popup_selected_index: 0,
            selected_month: 1,
            round_payments_up: false,
            interest_basis_mode: InterestBasisMode::Act365Fixed,
            focus_area: FocusArea::Inputs,
            schedule_rows: Vec::new(),
            schedule_selected_index: 0,
            schedule_scroll_offset: 0,
            active_field_idx: 0,
            schedule_viewport_rows: 1,
            rate_overrides: BTreeMap::new(),
            extra_payments: BTreeMap::new(),
            recurring_extra_payments: BTreeMap::new(),
        };
        app.restore_state_from_disk_silently();
        app.recalculate();
        app
    }
}

impl App {
    pub fn is_any_popup_open(&self) -> bool {
        self.row_edit_popup_mode != RowEditPopupMode::None
            || self.is_interest_basis_popup_open
            || self.is_reset_confirm_popup_open
    }

    pub fn active_field(&self) -> FieldId {
        FieldId::ALL[self.active_field_idx]
    }

    pub fn field_value(&self, field: FieldId) -> &str {
        assert!(field.is_text_input(), "checkbox field has no string input");
        &self.inputs[field.index()]
    }

    pub fn field_display_value(&self, field: FieldId) -> String {
        match field {
            FieldId::InterestBasis => self.interest_basis_mode.label().to_string(),
            FieldId::RoundPaymentsUp => {
                if self.round_payments_up {
                    "[x]".to_string()
                } else {
                    "[ ]".to_string()
                }
            }
            _ => self.field_value(field).to_string(),
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
        let row_count = self.selectable_row_count();
        if row_count == 0 || self.schedule_selected_index == 0 {
            return;
        }

        self.schedule_selected_index = 0;
        self.sync_selected_month_from_selection();
        self.ensure_schedule_selection_visible();
        self.recalculate();
    }

    pub fn move_schedule_selection_to_end(&mut self) {
        let row_count = self.selectable_row_count();
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
        let row_count = self.selectable_row_count();
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

    pub fn is_row_action_popup_open(&self) -> bool {
        self.row_edit_popup_mode == RowEditPopupMode::ActionSelect
    }

    pub fn is_apr_edit_popup_open(&self) -> bool {
        self.row_edit_popup_mode == RowEditPopupMode::AprEdit
    }

    pub fn is_extra_edit_popup_open(&self) -> bool {
        self.row_edit_popup_mode == RowEditPopupMode::ExtraEdit
    }

    pub fn is_recurring_extra_edit_popup_open(&self) -> bool {
        self.row_edit_popup_mode == RowEditPopupMode::RecurringExtraEdit
    }

    pub fn open_row_edit_popup(&mut self) {
        self.clamp_schedule_selection();
        self.sync_selected_month_from_selection();

        match self.selected_schedule_row() {
            Some(ScheduleDisplayRow::AprChangeMarker { .. }) => {
                self.open_apr_edit_popup_for_selected_row()
            }
            Some(ScheduleDisplayRow::ExtraPaymentMarker { .. }) => {
                self.open_extra_edit_popup_for_selected_row()
            }
            Some(ScheduleDisplayRow::RecurringExtraPaymentMarker { .. }) => {
                self.open_recurring_extra_edit_popup_for_selected_row()
            }
            _ => self.open_row_action_popup_for_selected_row(),
        }
    }

    pub fn close_row_edit_popup(&mut self) {
        self.row_edit_popup_mode = RowEditPopupMode::None;
    }

    pub fn open_row_action_popup_for_selected_row(&mut self) {
        self.is_interest_basis_popup_open = false;
        self.is_reset_confirm_popup_open = false;
        self.row_action_selected_index = 0;
        self.row_edit_popup_mode = RowEditPopupMode::ActionSelect;
    }

    pub fn row_action_move_up(&mut self) {
        if self.row_action_selected_index == 0 {
            return;
        }

        self.row_action_selected_index -= 1;
    }

    pub fn row_action_move_down(&mut self) {
        if self.row_action_selected_index + 1 >= RowActionOption::ALL.len() {
            return;
        }

        self.row_action_selected_index += 1;
    }

    pub fn row_action_selected_option(&self) -> RowActionOption {
        RowActionOption::ALL
            .get(self.row_action_selected_index)
            .copied()
            .unwrap_or(RowActionOption::AddExtraPayment)
    }

    pub fn apply_row_action_popup_selection(&mut self) {
        match self.row_action_selected_option() {
            RowActionOption::AddExtraPayment => self.open_extra_edit_popup_for_selected_row(),
            RowActionOption::AddAprChange => self.open_apr_edit_popup_for_selected_row(),
            RowActionOption::AddRecurringExtraPayment => {
                self.open_recurring_extra_edit_popup_for_selected_row()
            }
        }
    }

    pub fn open_apr_edit_popup_for_selected_row(&mut self) {
        self.is_interest_basis_popup_open = false;
        self.is_reset_confirm_popup_open = false;
        self.sync_apr_edit_popup_inputs();
        self.apr_edit_active_row = 0;
        self.row_edit_popup_mode = RowEditPopupMode::AprEdit;
    }

    pub fn apr_edit_move_up(&mut self) {
        if self.apr_edit_active_row == 0 {
            return;
        }

        self.apr_edit_active_row -= 1;
    }

    pub fn apr_edit_move_down(&mut self) {
        if self.apr_edit_active_row >= 4 {
            return;
        }

        self.apr_edit_active_row += 1;
    }

    pub fn apr_edit_input_char(&mut self, c: char) {
        match self.apr_edit_active_row {
            0 => {
                if c.is_ascii_digit() || c == '-' {
                    self.apr_edit_date_input_buffer.push(c);
                }
            }
            1 => {
                if !c.is_ascii_digit() && c != '.' {
                    return;
                }

                if c == '.' {
                    if self.apr_edit_apr_input_buffer.contains('.') {
                        return;
                    }
                    if self.apr_edit_apr_input_buffer.is_empty() {
                        self.apr_edit_apr_input_buffer.push('0');
                    }
                }

                self.apr_edit_apr_input_buffer.push(c);
            }
            _ => {}
        }
    }

    pub fn apr_edit_input_backspace(&mut self) {
        match self.apr_edit_active_row {
            0 => {
                self.apr_edit_date_input_buffer.pop();
            }
            1 => {
                self.apr_edit_apr_input_buffer.pop();
            }
            _ => {}
        }
    }

    pub fn activate_apr_edit_row_on_enter(&mut self) {
        match self.apr_edit_active_row {
            2 => self.apply_apr_edit_from_dialog(),
            3 => self.clear_apr_edit_from_dialog(),
            4 => self.close_row_edit_popup(),
            _ => {}
        }
    }

    pub fn open_extra_edit_popup_for_selected_row(&mut self) {
        self.is_interest_basis_popup_open = false;
        self.is_reset_confirm_popup_open = false;
        self.sync_extra_edit_popup_inputs();
        self.extra_edit_active_row = 0;
        self.row_edit_popup_mode = RowEditPopupMode::ExtraEdit;
    }

    pub fn extra_edit_move_up(&mut self) {
        if self.extra_edit_active_row == 0 {
            return;
        }

        self.extra_edit_active_row -= 1;
    }

    pub fn extra_edit_move_down(&mut self) {
        if self.extra_edit_active_row >= 4 {
            return;
        }

        self.extra_edit_active_row += 1;
    }

    pub fn extra_edit_input_char(&mut self, c: char) {
        match self.extra_edit_active_row {
            0 => {
                if c.is_ascii_digit() || c == '-' {
                    self.extra_edit_date_input_buffer.push(c);
                }
            }
            1 => {
                if !c.is_ascii_digit() && c != '.' {
                    return;
                }

                if c == '.' {
                    if self.extra_edit_amount_input_buffer.contains('.') {
                        return;
                    }
                    if self.extra_edit_amount_input_buffer.is_empty() {
                        self.extra_edit_amount_input_buffer.push('0');
                    }
                }

                self.extra_edit_amount_input_buffer.push(c);
            }
            _ => {}
        }
    }

    pub fn extra_edit_input_backspace(&mut self) {
        match self.extra_edit_active_row {
            0 => {
                self.extra_edit_date_input_buffer.pop();
            }
            1 => {
                self.extra_edit_amount_input_buffer.pop();
            }
            _ => {}
        }
    }

    pub fn activate_extra_edit_row_on_enter(&mut self) {
        match self.extra_edit_active_row {
            2 => self.apply_extra_edit_from_dialog(),
            3 => self.clear_extra_edit_from_dialog(),
            4 => self.close_row_edit_popup(),
            _ => {}
        }
    }

    pub fn open_recurring_extra_edit_popup_for_selected_row(&mut self) {
        self.is_interest_basis_popup_open = false;
        self.is_reset_confirm_popup_open = false;
        self.sync_recurring_extra_edit_popup_inputs();
        self.recurring_edit_active_row = 0;
        self.row_edit_popup_mode = RowEditPopupMode::RecurringExtraEdit;
    }

    pub fn recurring_extra_edit_move_up(&mut self) {
        if self.recurring_edit_active_row == 0 {
            return;
        }
        self.recurring_edit_active_row -= 1;
    }

    pub fn recurring_extra_edit_move_down(&mut self) {
        if self.recurring_edit_active_row >= 5 {
            return;
        }
        self.recurring_edit_active_row += 1;
    }

    pub fn recurring_extra_edit_input_char(&mut self, c: char) {
        match self.recurring_edit_active_row {
            0 => {
                if c.is_ascii_digit() || c == '-' {
                    self.recurring_edit_start_date_input_buffer.push(c);
                }
            }
            1 => {
                if c.is_ascii_digit() || c == '-' {
                    self.recurring_edit_annual_date_input_buffer.push(c);
                }
            }
            2 => {
                if !c.is_ascii_digit() && c != '.' {
                    return;
                }
                if c == '.' {
                    if self.recurring_edit_amount_input_buffer.contains('.') {
                        return;
                    }
                    if self.recurring_edit_amount_input_buffer.is_empty() {
                        self.recurring_edit_amount_input_buffer.push('0');
                    }
                }
                self.recurring_edit_amount_input_buffer.push(c);
            }
            _ => {}
        }
    }

    pub fn recurring_extra_edit_input_backspace(&mut self) {
        match self.recurring_edit_active_row {
            0 => {
                self.recurring_edit_start_date_input_buffer.pop();
            }
            1 => {
                self.recurring_edit_annual_date_input_buffer.pop();
            }
            2 => {
                self.recurring_edit_amount_input_buffer.pop();
            }
            _ => {}
        }
    }

    pub fn activate_recurring_extra_edit_row_on_enter(&mut self) {
        match self.recurring_edit_active_row {
            3 => self.apply_recurring_extra_edit_from_dialog(),
            4 => self.clear_recurring_extra_edit_from_dialog(),
            5 => self.close_row_edit_popup(),
            _ => {}
        }
    }

    pub fn open_interest_basis_popup(&mut self) {
        self.row_edit_popup_mode = RowEditPopupMode::None;
        self.is_reset_confirm_popup_open = false;
        self.is_interest_basis_popup_open = true;
        self.interest_basis_popup_selected_index = InterestBasisMode::ALL
            .iter()
            .position(|mode| *mode == self.interest_basis_mode)
            .unwrap_or(0);
    }

    pub fn close_interest_basis_popup(&mut self) {
        self.is_interest_basis_popup_open = false;
    }

    pub fn open_reset_confirm_popup(&mut self) {
        self.row_edit_popup_mode = RowEditPopupMode::None;
        self.is_interest_basis_popup_open = false;
        self.is_reset_confirm_popup_open = true;
        self.reset_confirm_selected_index = 0;
    }

    pub fn close_reset_confirm_popup(&mut self) {
        self.is_reset_confirm_popup_open = false;
    }

    pub fn reset_confirm_move_up(&mut self) {
        if self.reset_confirm_selected_index == 0 {
            return;
        }
        self.reset_confirm_selected_index -= 1;
    }

    pub fn reset_confirm_move_down(&mut self) {
        if self.reset_confirm_selected_index + 1 >= ResetConfirmOption::ALL.len() {
            return;
        }
        self.reset_confirm_selected_index += 1;
    }

    pub fn reset_confirm_selected_option(&self) -> ResetConfirmOption {
        ResetConfirmOption::ALL
            .get(self.reset_confirm_selected_index)
            .copied()
            .unwrap_or(ResetConfirmOption::Cancel)
    }

    pub fn apply_reset_confirm_selection(&mut self) {
        match self.reset_confirm_selected_option() {
            ResetConfirmOption::Cancel => self.close_reset_confirm_popup(),
            ResetConfirmOption::ConfirmReset => self.reset(),
        }
    }

    pub fn interest_basis_popup_move_up(&mut self) {
        if self.interest_basis_popup_selected_index == 0 {
            return;
        }

        self.interest_basis_popup_selected_index -= 1;
    }

    pub fn interest_basis_popup_move_down(&mut self) {
        if self.interest_basis_popup_selected_index + 1 >= InterestBasisMode::ALL.len() {
            return;
        }

        self.interest_basis_popup_selected_index += 1;
    }

    pub fn apply_interest_basis_popup_selection(&mut self) {
        let Some(mode) = InterestBasisMode::ALL
            .get(self.interest_basis_popup_selected_index)
            .copied()
        else {
            return;
        };

        self.interest_basis_mode = mode;
        self.persist_state_silently();
        self.recalculate();
        self.close_interest_basis_popup();
    }

    pub fn interest_basis_popup_selected_mode(&self) -> InterestBasisMode {
        InterestBasisMode::ALL
            .get(self.interest_basis_popup_selected_index)
            .copied()
            .unwrap_or(self.interest_basis_mode)
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
        self.persist_state_silently();
    }

    pub fn backspace(&mut self) {
        if !self.active_field().is_text_input() {
            return;
        }

        self.inputs[self.active_field().index()].pop();
        self.persist_state_silently();
    }

    pub fn toggle_round_payments_up(&mut self) {
        self.round_payments_up = !self.round_payments_up;
        self.persist_state_silently();
        self.recalculate();
    }

    pub fn reset(&mut self) {
        self.inputs = default_inputs();
        self.active_field_idx = 0;
        self.error = None;
        self.row_edit_popup_mode = RowEditPopupMode::None;
        self.is_reset_confirm_popup_open = false;
        self.reset_confirm_selected_index = 0;
        self.is_interest_basis_popup_open = false;
        self.row_action_selected_index = 0;
        self.apr_edit_date_input_buffer.clear();
        self.apr_edit_apr_input_buffer.clear();
        self.apr_edit_active_row = 0;
        self.extra_edit_date_input_buffer.clear();
        self.extra_edit_amount_input_buffer.clear();
        self.extra_edit_active_row = 0;
        self.recurring_edit_start_date_input_buffer.clear();
        self.recurring_edit_annual_date_input_buffer.clear();
        self.recurring_edit_amount_input_buffer.clear();
        self.recurring_edit_active_row = 0;
        self.recurring_edit_source_key = None;
        self.interest_basis_popup_selected_index = 0;
        self.selected_month = 1;
        self.round_payments_up = false;
        self.interest_basis_mode = InterestBasisMode::Act365Fixed;
        self.focus_area = FocusArea::Inputs;
        self.schedule_rows.clear();
        self.schedule_selected_index = 0;
        self.schedule_scroll_offset = 0;
        self.rate_overrides.clear();
        self.extra_payments.clear();
        self.recurring_extra_payments.clear();
        self.persist_state_silently();
        self.recalculate();
    }

    pub fn recalculate(&mut self) {
        self.normalize_rate_state();

        match self.build_input() {
            Ok(input) => match calculate_metrics(&input, self.selected_month) {
                Ok(metrics) => {
                    self.metrics = Some(metrics);
                    self.error = None;
                    self.rebuild_schedule_rows();
                }
                Err(err) => {
                    self.metrics = None;
                    self.schedule_rows.clear();
                    self.error = Some(err.to_string());
                }
            },
            Err(err) => {
                self.metrics = None;
                self.schedule_rows.clear();
                self.error = Some(err);
            }
        }

        self.clamp_schedule_selection();
        self.sync_selected_month_from_selection();
        self.ensure_schedule_selection_visible();

        if let Some(metrics) = self.metrics.as_ref() {
            if metrics.selected_month != self.selected_month {
                if let Ok(input) = self.build_input() {
                    if let Ok(updated) = calculate_metrics(&input, self.selected_month) {
                        self.metrics = Some(updated);
                        self.rebuild_schedule_rows();
                        self.clamp_schedule_selection();
                        self.sync_selected_month_from_selection();
                        self.ensure_schedule_selection_visible();
                    }
                }
            }
        }
    }

    pub fn override_for_date(&self, date: DateYmd) -> Option<f64> {
        self.rate_overrides.get(&date).copied()
    }

    pub fn extra_payment_for_date(&self, date: DateYmd) -> Option<f64> {
        self.extra_payments.get(&date).copied()
    }

    pub fn effective_rate_for_date(&self, date: DateYmd) -> Option<f64> {
        let mut effective = parse_f64(
            FieldId::InterestRate,
            self.field_value(FieldId::InterestRate),
        )
        .ok()?;

        for (_, override_rate) in self.rate_overrides.range(..=date) {
            effective = *override_rate;
        }

        Some(effective)
    }

    pub fn prior_effective_rate_before_date(&self, date: DateYmd) -> Option<f64> {
        let mut effective = parse_f64(
            FieldId::InterestRate,
            self.field_value(FieldId::InterestRate),
        )
        .ok()?;

        for (_, override_rate) in self.rate_overrides.range(..date) {
            effective = *override_rate;
        }

        Some(effective)
    }

    fn normalize_rate_state(&mut self) {
        if let Some((start_date, last_payment_date)) = self.override_date_range_from_inputs() {
            self.rate_overrides.retain(|effective_date, _| {
                *effective_date >= start_date && *effective_date <= last_payment_date
            });
            self.extra_payments.retain(|effective_date, amount| {
                *effective_date >= start_date
                    && *effective_date <= last_payment_date
                    && amount.is_finite()
                    && *amount > 0.0
            });
            self.recurring_extra_payments.retain(|key, amount| {
                key.start_date >= start_date
                    && key.start_date <= last_payment_date
                    && (1..=12).contains(&key.month)
                    && (1..=31).contains(&key.day)
                    && amount.is_finite()
                    && *amount > 0.0
            });
        }

        if self.schedule_rows.is_empty() {
            if let Some(max_rows) = self.term_months_from_input() {
                let max_index = max_rows.saturating_sub(1) as usize;
                self.schedule_selected_index = self.schedule_selected_index.min(max_index);
            } else {
                self.clamp_schedule_selection();
            }
        } else {
            self.clamp_schedule_selection();
        }

        self.sync_selected_month_from_selection();
    }

    fn selectable_row_count(&self) -> usize {
        if !self.schedule_rows.is_empty() {
            return self.schedule_rows.len();
        }

        if self.metrics.is_some() {
            return 0;
        }

        if let Some(months) = self.term_months_from_input() {
            return months as usize;
        }

        1
    }

    fn clamp_schedule_selection(&mut self) {
        let row_count = self.selectable_row_count();
        if row_count == 0 {
            self.schedule_selected_index = 0;
        } else {
            self.schedule_selected_index = self.schedule_selected_index.min(row_count - 1);
        }
    }

    fn ensure_schedule_selection_visible(&mut self) {
        let row_count = self.selectable_row_count();
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
        if let Some(row) = self
            .schedule_rows
            .get(self.schedule_selected_index)
            .copied()
        {
            self.selected_month = row.target_month();
        } else {
            self.selected_month = self.schedule_selected_index.saturating_add(1) as u32;
        }
    }

    fn sync_apr_edit_popup_inputs(&mut self) {
        let Some(row) = self
            .schedule_rows
            .get(self.schedule_selected_index)
            .copied()
        else {
            self.apr_edit_date_input_buffer.clear();
            self.apr_edit_apr_input_buffer.clear();
            return;
        };

        let selected_date = row.date();
        self.apr_edit_date_input_buffer = selected_date.format_yyyy_mm_dd();

        let apr = match row {
            ScheduleDisplayRow::Payment { payment_date, .. } => {
                self.rate_overrides.get(&payment_date).copied()
            }
            ScheduleDisplayRow::AprChangeMarker {
                annual_interest_rate_pct,
                ..
            } => Some(annual_interest_rate_pct),
            ScheduleDisplayRow::ExtraPaymentMarker { .. }
            | ScheduleDisplayRow::RecurringExtraPaymentMarker { .. } => {
                self.rate_overrides.get(&selected_date).copied()
            }
        };

        if let Some(apr) = apr {
            self.apr_edit_apr_input_buffer = format_rate_for_input(apr);
        } else {
            self.apr_edit_apr_input_buffer.clear();
        }
    }

    fn sync_extra_edit_popup_inputs(&mut self) {
        let Some(row) = self
            .schedule_rows
            .get(self.schedule_selected_index)
            .copied()
        else {
            self.extra_edit_date_input_buffer.clear();
            self.extra_edit_amount_input_buffer.clear();
            return;
        };

        let selected_date = row.date();
        self.extra_edit_date_input_buffer = selected_date.format_yyyy_mm_dd();

        let amount = match row {
            ScheduleDisplayRow::ExtraPaymentMarker { amount, .. } => Some(amount),
            _ => self.extra_payments.get(&selected_date).copied(),
        };

        if let Some(amount) = amount {
            self.extra_edit_amount_input_buffer = format_rate_for_input(amount);
        } else {
            self.extra_edit_amount_input_buffer.clear();
        }
    }

    fn sync_recurring_extra_edit_popup_inputs(&mut self) {
        let Some(row) = self
            .schedule_rows
            .get(self.schedule_selected_index)
            .copied()
        else {
            self.recurring_edit_start_date_input_buffer.clear();
            self.recurring_edit_annual_date_input_buffer.clear();
            self.recurring_edit_amount_input_buffer.clear();
            self.recurring_edit_source_key = None;
            return;
        };

        let selected_date = row.date();
        let selected_key = match row {
            ScheduleDisplayRow::RecurringExtraPaymentMarker {
                recurring_start_date,
                recurring_month,
                recurring_day,
                ..
            } => Some(RecurringExtraPaymentKey {
                start_date: recurring_start_date,
                month: recurring_month,
                day: recurring_day,
            }),
            _ => None,
        };

        let key = selected_key.or_else(|| {
            self.recurring_extra_payments
                .keys()
                .find(|key| {
                    key.month == selected_date.month
                        && key
                            .day
                            .min(last_day_of_month(selected_date.year, key.month))
                            == selected_date.day
                        && selected_date >= key.start_date
                })
                .copied()
        });
        self.recurring_edit_source_key = key;

        if let Some(key) = key {
            self.recurring_edit_start_date_input_buffer = key.start_date.format_yyyy_mm_dd();
            self.recurring_edit_annual_date_input_buffer =
                format!("{:02}-{:02}", key.month, key.day);
            let amount = self
                .recurring_extra_payments
                .get(&key)
                .copied()
                .unwrap_or(0.0);
            if amount > 0.0 {
                self.recurring_edit_amount_input_buffer = format_rate_for_input(amount);
            } else {
                self.recurring_edit_amount_input_buffer.clear();
            }
        } else {
            self.recurring_edit_start_date_input_buffer = selected_date.format_yyyy_mm_dd();
            self.recurring_edit_annual_date_input_buffer =
                format!("{:02}-{:02}", selected_date.month, selected_date.day);
            self.recurring_edit_amount_input_buffer.clear();
        }
    }

    fn apply_apr_edit_from_dialog(&mut self) {
        let date = match parse_date_input_for_row_edit(&self.apr_edit_date_input_buffer) {
            Ok(date) => date,
            Err(err) => {
                self.error = Some(err);
                return;
            }
        };

        let apr_trimmed = self.apr_edit_apr_input_buffer.trim();
        if apr_trimmed.is_empty() {
            self.error = Some("APR is required".to_string());
            return;
        }

        let parsed = match apr_trimmed.parse::<f64>() {
            Ok(parsed) if parsed.is_finite() && parsed >= 0.0 => parsed,
            _ => {
                self.error = Some("APR must be a non-negative number".to_string());
                return;
            }
        };

        self.rate_overrides.insert(date, parsed);
        self.error = None;
        self.persist_state_silently();
        self.recalculate();
        self.select_schedule_row_by_date(date, ScheduleRowSelectionPreference::Apr);
        self.close_row_edit_popup();
    }

    fn clear_apr_edit_from_dialog(&mut self) {
        let date = match parse_date_input_for_row_edit(&self.apr_edit_date_input_buffer) {
            Ok(date) => date,
            Err(err) => {
                self.error = Some(err);
                return;
            }
        };

        self.rate_overrides.remove(&date);
        self.error = None;
        self.persist_state_silently();
        self.recalculate();
        self.select_schedule_row_by_date(date, ScheduleRowSelectionPreference::Payment);
        self.close_row_edit_popup();
    }

    fn apply_extra_edit_from_dialog(&mut self) {
        let date = match parse_date_input_for_row_edit(&self.extra_edit_date_input_buffer) {
            Ok(date) => date,
            Err(err) => {
                self.error = Some(err);
                return;
            }
        };

        let amount_trimmed = self.extra_edit_amount_input_buffer.trim();
        if amount_trimmed.is_empty() {
            self.error = Some("Extra payment amount is required".to_string());
            return;
        }

        let parsed = match amount_trimmed.parse::<f64>() {
            Ok(parsed) if parsed.is_finite() && parsed >= 0.0 => parsed,
            _ => {
                self.error = Some("Extra payment must be a non-negative number".to_string());
                return;
            }
        };

        if parsed == 0.0 {
            self.extra_payments.remove(&date);
        } else {
            self.extra_payments.insert(date, parsed);
        }

        self.error = None;
        self.persist_state_silently();
        self.recalculate();
        let preference = if parsed == 0.0 {
            ScheduleRowSelectionPreference::Payment
        } else {
            ScheduleRowSelectionPreference::Extra
        };
        self.select_schedule_row_by_date(date, preference);
        self.close_row_edit_popup();
    }

    fn clear_extra_edit_from_dialog(&mut self) {
        let date = match parse_date_input_for_row_edit(&self.extra_edit_date_input_buffer) {
            Ok(date) => date,
            Err(err) => {
                self.error = Some(err);
                return;
            }
        };

        self.extra_payments.remove(&date);
        self.error = None;
        self.persist_state_silently();
        self.recalculate();
        self.select_schedule_row_by_date(date, ScheduleRowSelectionPreference::Payment);
        self.close_row_edit_popup();
    }

    fn apply_recurring_extra_edit_from_dialog(&mut self) {
        let start_date =
            match parse_date_input_for_row_edit(&self.recurring_edit_start_date_input_buffer) {
                Ok(date) => date,
                Err(err) => {
                    self.error = Some(err);
                    return;
                }
            };
        let (month, day) =
            match parse_annual_mm_dd_input(&self.recurring_edit_annual_date_input_buffer) {
                Ok(month_day) => month_day,
                Err(err) => {
                    self.error = Some(err);
                    return;
                }
            };

        let amount_trimmed = self.recurring_edit_amount_input_buffer.trim();
        if amount_trimmed.is_empty() {
            self.error = Some("Recurring extra payment amount is required".to_string());
            return;
        }
        let parsed = match amount_trimmed.parse::<f64>() {
            Ok(parsed) if parsed.is_finite() && parsed >= 0.0 => parsed,
            _ => {
                self.error =
                    Some("Recurring extra payment must be a non-negative number".to_string());
                return;
            }
        };

        if let Some(source_key) = self.recurring_edit_source_key {
            if source_key.start_date != start_date
                || source_key.month != month
                || source_key.day != day
            {
                self.recurring_extra_payments.remove(&source_key);
            }
        }

        let key = RecurringExtraPaymentKey {
            start_date,
            month,
            day,
        };

        if parsed == 0.0 {
            self.recurring_extra_payments.remove(&key);
        } else {
            self.recurring_extra_payments.insert(key, parsed);
        }

        self.error = None;
        self.persist_state_silently();
        self.recalculate();

        if parsed == 0.0 {
            self.select_schedule_row_by_date(start_date, ScheduleRowSelectionPreference::Payment);
        } else if let Some((start_range, end_range)) = self.override_date_range_from_inputs() {
            if let Some(occurrence_date) =
                first_recurring_occurrence_in_range(key, start_range, end_range)
            {
                self.select_schedule_row_by_date(
                    occurrence_date,
                    ScheduleRowSelectionPreference::Recurring,
                );
            } else {
                self.select_schedule_row_by_date(
                    start_date,
                    ScheduleRowSelectionPreference::Recurring,
                );
            }
        } else {
            self.select_schedule_row_by_date(start_date, ScheduleRowSelectionPreference::Recurring);
        }

        self.close_row_edit_popup();
    }

    fn clear_recurring_extra_edit_from_dialog(&mut self) {
        let parsed_start =
            parse_date_input_for_row_edit(&self.recurring_edit_start_date_input_buffer).ok();
        let parsed_month_day =
            parse_annual_mm_dd_input(&self.recurring_edit_annual_date_input_buffer).ok();

        let key = self.recurring_edit_source_key.or_else(|| {
            parsed_start
                .zip(parsed_month_day)
                .map(|(start_date, (month, day))| RecurringExtraPaymentKey {
                    start_date,
                    month,
                    day,
                })
        });

        let Some(key) = key else {
            self.error = Some("Recurring extra payment key is invalid".to_string());
            return;
        };

        self.recurring_extra_payments.remove(&key);
        self.error = None;
        self.persist_state_silently();
        self.recalculate();
        self.select_schedule_row_by_date(key.start_date, ScheduleRowSelectionPreference::Payment);
        self.close_row_edit_popup();
    }

    pub fn selected_schedule_row(&self) -> Option<ScheduleDisplayRow> {
        self.schedule_rows
            .get(self.schedule_selected_index)
            .copied()
    }

    fn rebuild_schedule_rows(&mut self) {
        self.schedule_rows.clear();

        let Some(metrics) = self.metrics.as_ref() else {
            return;
        };

        for (schedule_index, entry) in metrics.repayment_schedule.iter().enumerate() {
            self.schedule_rows.push(ScheduleDisplayRow::Payment {
                schedule_index,
                month_index: entry.month_index,
                payment_date: entry.payment_date,
            });
        }

        for (effective_date, annual_interest_rate_pct) in &self.rate_overrides {
            let target_month = metrics
                .repayment_schedule
                .iter()
                .find(|entry| entry.payment_date > *effective_date)
                .map(|entry| entry.month_index)
                .or_else(|| {
                    metrics
                        .repayment_schedule
                        .last()
                        .map(|entry| entry.month_index)
                })
                .unwrap_or(1);

            self.schedule_rows
                .push(ScheduleDisplayRow::AprChangeMarker {
                    effective_date: *effective_date,
                    annual_interest_rate_pct: *annual_interest_rate_pct,
                    target_month,
                });
        }

        for applied_extra in &metrics.applied_extra_payments {
            let target_month = metrics
                .repayment_schedule
                .iter()
                .find(|entry| entry.payment_date > applied_extra.effective_date)
                .map(|entry| entry.month_index)
                .or_else(|| {
                    metrics
                        .repayment_schedule
                        .last()
                        .map(|entry| entry.month_index)
                })
                .unwrap_or(1);

            match applied_extra.source {
                AppliedExtraPaymentSource::OneTime => {
                    self.schedule_rows
                        .push(ScheduleDisplayRow::ExtraPaymentMarker {
                            effective_date: applied_extra.effective_date,
                            amount: applied_extra.applied_amount,
                            target_month,
                        });
                }
                AppliedExtraPaymentSource::Recurring {
                    start_date,
                    month,
                    day,
                } => {
                    self.schedule_rows
                        .push(ScheduleDisplayRow::RecurringExtraPaymentMarker {
                            effective_date: applied_extra.effective_date,
                            amount: applied_extra.applied_amount,
                            target_month,
                            recurring_start_date: start_date,
                            recurring_month: month,
                            recurring_day: day,
                        });
                }
            }
        }

        self.schedule_rows.sort_by(|left, right| {
            let left_date = left.date();
            let right_date = right.date();
            left_date.cmp(&right_date).then_with(|| {
                let left_priority = match left {
                    ScheduleDisplayRow::Payment { .. } => 0_u8,
                    ScheduleDisplayRow::AprChangeMarker { .. } => 1_u8,
                    ScheduleDisplayRow::ExtraPaymentMarker { .. } => 2_u8,
                    ScheduleDisplayRow::RecurringExtraPaymentMarker { .. } => 3_u8,
                };
                let right_priority = match right {
                    ScheduleDisplayRow::Payment { .. } => 0_u8,
                    ScheduleDisplayRow::AprChangeMarker { .. } => 1_u8,
                    ScheduleDisplayRow::ExtraPaymentMarker { .. } => 2_u8,
                    ScheduleDisplayRow::RecurringExtraPaymentMarker { .. } => 3_u8,
                };
                left_priority.cmp(&right_priority)
            })
        });

        let mut running_balance = metrics.purchase_price_estimate;
        self.schedule_rows.retain(|row| {
            let balance_before = running_balance;
            let principal_delta = match row {
                ScheduleDisplayRow::Payment { schedule_index, .. } => {
                    metrics.repayment_schedule[*schedule_index].principal_payment
                }
                ScheduleDisplayRow::ExtraPaymentMarker { amount, .. }
                | ScheduleDisplayRow::RecurringExtraPaymentMarker { amount, .. } => *amount,
                ScheduleDisplayRow::AprChangeMarker { .. } => 0.0,
            };

            if principal_delta > 0.0 {
                running_balance = (running_balance - principal_delta).max(0.0);
                if running_balance.abs() < 1e-9 {
                    running_balance = 0.0;
                }
            }

            let is_extra_marker = matches!(
                row,
                ScheduleDisplayRow::ExtraPaymentMarker { .. }
                    | ScheduleDisplayRow::RecurringExtraPaymentMarker { .. }
            );
            let is_payoff_extra_marker =
                is_extra_marker && balance_before > 1e-9 && running_balance <= 1e-9;
            if is_payoff_extra_marker {
                return true;
            }

            running_balance > 1e-9
        });
    }

    fn select_schedule_row_by_date(
        &mut self,
        date: DateYmd,
        preference: ScheduleRowSelectionPreference,
    ) {
        if self.schedule_rows.is_empty() {
            return;
        }

        let mut exact_payment = None;
        let mut exact_apr_marker = None;
        let mut exact_extra_marker = None;
        let mut exact_recurring_marker = None;
        let mut next_by_date = None;

        for (index, row) in self.schedule_rows.iter().enumerate() {
            if next_by_date.is_none() && row.date() >= date {
                next_by_date = Some(index);
            }

            if row.date() == date {
                match row {
                    ScheduleDisplayRow::Payment { .. } => exact_payment = Some(index),
                    ScheduleDisplayRow::AprChangeMarker { .. } => exact_apr_marker = Some(index),
                    ScheduleDisplayRow::ExtraPaymentMarker { .. } => {
                        exact_extra_marker = Some(index)
                    }
                    ScheduleDisplayRow::RecurringExtraPaymentMarker { .. } => {
                        exact_recurring_marker = Some(index)
                    }
                }
            }
        }

        let fallback = next_by_date.unwrap_or(self.schedule_rows.len().saturating_sub(1));
        let target_index = match preference {
            ScheduleRowSelectionPreference::Payment => exact_payment
                .or(exact_apr_marker)
                .or(exact_extra_marker)
                .or(exact_recurring_marker)
                .unwrap_or(fallback),
            ScheduleRowSelectionPreference::Apr => exact_apr_marker
                .or(exact_payment)
                .or(exact_extra_marker)
                .or(exact_recurring_marker)
                .unwrap_or(fallback),
            ScheduleRowSelectionPreference::Extra => exact_extra_marker
                .or(exact_payment)
                .or(exact_apr_marker)
                .or(exact_recurring_marker)
                .unwrap_or(fallback),
            ScheduleRowSelectionPreference::Recurring => exact_recurring_marker
                .or(exact_payment)
                .or(exact_extra_marker)
                .or(exact_apr_marker)
                .unwrap_or(fallback),
        };

        if target_index == self.schedule_selected_index {
            return;
        }

        self.schedule_selected_index = target_index;
        self.sync_selected_month_from_selection();
        self.ensure_schedule_selection_visible();
        self.recalculate();
    }

    fn override_date_range_from_inputs(&self) -> Option<(DateYmd, DateYmd)> {
        let total_months = self.term_months_from_input()?;
        let start_date = try_parse_date(self.field_value(FieldId::StartDate))?;
        let payment_day =
            parse_u32(FieldId::PaymentDay, self.field_value(FieldId::PaymentDay)).ok()?;
        if payment_day == 0 || payment_day > 31 {
            return None;
        }

        let (last_payment_year, last_payment_month) =
            add_months(start_date.year, start_date.month, total_months as i32);
        let last_payment_day =
            payment_day.min(last_day_of_month(last_payment_year, last_payment_month));
        let last_payment_date =
            DateYmd::from_ymd_opt(last_payment_year, last_payment_month, last_payment_day)?;

        Some((start_date, last_payment_date))
    }

    fn restore_state_from_disk_silently(&mut self) {
        #[cfg(test)]
        {
            return;
        }

        #[cfg(not(test))]
        {
            let Ok(raw_state) = fs::read_to_string(STATE_FILE_PATH) else {
                return;
            };

            let mut restored_inputs = self.inputs.clone();
            if let Some(value) = extract_json_string_value(&raw_state, "loan_amount") {
                restored_inputs[FieldId::LoanAmount.index()] = value;
            }
            if let Some(value) = extract_json_string_value(&raw_state, "one_time_fees") {
                restored_inputs[FieldId::OneTimeFees.index()] = value;
            }
            if let Some(value) = extract_json_string_value(&raw_state, "monthly_fees") {
                restored_inputs[FieldId::MonthlyFees.index()] = value;
            }
            if let Some(value) = extract_json_string_value(&raw_state, "interest_rate") {
                restored_inputs[FieldId::InterestRate.index()] = value;
            }
            if let Some(value) = extract_json_string_value(&raw_state, "term_years") {
                restored_inputs[FieldId::TermYears.index()] = value;
            }
            if let Some(value) = extract_json_string_value(&raw_state, "start_date") {
                restored_inputs[FieldId::StartDate.index()] = value;
            }
            if let Some(value) = extract_json_string_value(&raw_state, "payment_day") {
                restored_inputs[FieldId::PaymentDay.index()] = value;
            }

            self.inputs = restored_inputs;

            if let Some(round_flag) = extract_json_bool_value(&raw_state, "round_payments_up") {
                self.round_payments_up = round_flag;
            }
            if let Some(mode) = extract_json_string_value(&raw_state, "interest_basis_mode")
                .and_then(|raw| InterestBasisMode::from_persisted_key(raw.trim()))
            {
                self.interest_basis_mode = mode;
            }

            if let Some(overrides_obj) = extract_json_object_value(&raw_state, "rate_overrides") {
                self.rate_overrides = parse_override_map_json(overrides_obj);
            }
            if let Some(extra_obj) = extract_json_object_value(&raw_state, "extra_payments") {
                self.extra_payments = parse_override_map_json(extra_obj);
            }
            if let Some(recurring_obj) =
                extract_json_object_value(&raw_state, "recurring_extra_payments")
            {
                self.recurring_extra_payments = parse_recurring_extra_map_json(recurring_obj);
            }
        }
    }

    fn persist_state_silently(&self) {
        #[cfg(test)]
        {
            return;
        }

        #[cfg(not(test))]
        {
            let serialized = self.to_persisted_state_json();
            let _ = fs::write(STATE_FILE_PATH, serialized);
        }
    }

    #[cfg(not(test))]
    fn to_persisted_state_json(&self) -> String {
        let mut json = String::from("{\n");
        json.push_str(&format!(
            "  \"loan_amount\": \"{}\",\n",
            escape_json_string(self.field_value(FieldId::LoanAmount))
        ));
        json.push_str(&format!(
            "  \"one_time_fees\": \"{}\",\n",
            escape_json_string(self.field_value(FieldId::OneTimeFees))
        ));
        json.push_str(&format!(
            "  \"monthly_fees\": \"{}\",\n",
            escape_json_string(self.field_value(FieldId::MonthlyFees))
        ));
        json.push_str(&format!(
            "  \"interest_rate\": \"{}\",\n",
            escape_json_string(self.field_value(FieldId::InterestRate))
        ));
        json.push_str(&format!(
            "  \"term_years\": \"{}\",\n",
            escape_json_string(self.field_value(FieldId::TermYears))
        ));
        json.push_str(&format!(
            "  \"start_date\": \"{}\",\n",
            escape_json_string(self.field_value(FieldId::StartDate))
        ));
        json.push_str(&format!(
            "  \"payment_day\": \"{}\",\n",
            escape_json_string(self.field_value(FieldId::PaymentDay))
        ));
        json.push_str(&format!(
            "  \"round_payments_up\": {},\n",
            if self.round_payments_up {
                "true"
            } else {
                "false"
            }
        ));
        json.push_str(&format!(
            "  \"interest_basis_mode\": \"{}\",\n",
            self.interest_basis_mode.persisted_key()
        ));
        json.push_str("  \"rate_overrides\": {\n");

        let override_len = self.rate_overrides.len();
        for (idx, (date, apr)) in self.rate_overrides.iter().enumerate() {
            let comma = if idx + 1 == override_len { "" } else { "," };
            json.push_str(&format!("    \"{}\": {}{}\n", date, apr, comma));
        }
        json.push_str("  },\n");
        json.push_str("  \"extra_payments\": {\n");
        let extra_len = self.extra_payments.len();
        for (idx, (date, amount)) in self.extra_payments.iter().enumerate() {
            let comma = if idx + 1 == extra_len { "" } else { "," };
            json.push_str(&format!("    \"{}\": {}{}\n", date, amount, comma));
        }
        json.push_str("  },\n");
        json.push_str("  \"recurring_extra_payments\": {\n");
        let recurring_len = self.recurring_extra_payments.len();
        for (idx, (key, amount)) in self.recurring_extra_payments.iter().enumerate() {
            let comma = if idx + 1 == recurring_len { "" } else { "," };
            json.push_str(&format!(
                "    \"{}|{:02}-{:02}\": {}{}\n",
                key.start_date, key.month, key.day, amount, comma
            ));
        }
        json.push_str("  }\n");
        json.push('}');
        json
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
            .map(|(effective_date, annual_interest_rate_pct)| RateOverride {
                effective_date: *effective_date,
                annual_interest_rate_pct: *annual_interest_rate_pct,
            })
            .collect();
        let extra_payments = self
            .extra_payments
            .iter()
            .map(|(effective_date, amount)| ExtraPayment {
                effective_date: *effective_date,
                amount: *amount,
            })
            .collect();
        let recurring_extra_payments = self
            .recurring_extra_payments
            .iter()
            .map(|(key, amount)| RecurringExtraPayment {
                start_date: key.start_date,
                month: key.month,
                day: key.day,
                amount: *amount,
            })
            .collect();

        Ok(LoanInput {
            loan_amount,
            one_time_fees,
            monthly_fees,
            round_monthly_payment_up: self.round_payments_up,
            interest_basis_mode: self.interest_basis_mode,
            base_annual_interest_rate_pct,
            term_years,
            start_date,
            payment_day,
            rate_overrides,
            extra_payments,
            recurring_extra_payments,
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

fn parse_date_input_for_row_edit(value: &str) -> Result<DateYmd, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Effective date is required".to_string());
    }

    DateYmd::parse_yyyy_mm_dd(trimmed)
        .ok_or_else(|| "Effective date must be in YYYY-MM-DD format".to_string())
}

fn parse_annual_mm_dd_input(value: &str) -> Result<(u32, u32), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Annual date is required".to_string());
    }

    let Some((month_raw, day_raw)) = trimmed.split_once('-') else {
        return Err("Annual date must be in MM-DD format".to_string());
    };

    let month = month_raw
        .parse::<u32>()
        .map_err(|_| "Annual date month must be a whole number".to_string())?;
    let day = day_raw
        .parse::<u32>()
        .map_err(|_| "Annual date day must be a whole number".to_string())?;
    if month == 0 || month > 12 || day == 0 || day > 31 {
        return Err(
            "Annual date must be in MM-DD format with month 1..12 and day 1..31".to_string(),
        );
    }

    Ok((month, day))
}

#[cfg(not(test))]
fn escape_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(not(test))]
fn extract_json_string_value(content: &str, key: &str) -> Option<String> {
    let key_pattern = format!("\"{key}\"");
    let key_start = content.find(&key_pattern)?;
    let after_key = &content[key_start + key_pattern.len()..];
    let colon_idx = after_key.find(':')?;
    let mut value_part = &after_key[colon_idx + 1..];
    value_part = value_part.trim_start();
    if !value_part.starts_with('"') {
        return None;
    }

    let mut result = String::new();
    let mut escaped = false;
    for ch in value_part[1..].chars() {
        if escaped {
            match ch {
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                other => result.push(other),
            }
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            return Some(result);
        }
        result.push(ch);
    }

    None
}

#[cfg(not(test))]
fn extract_json_bool_value(content: &str, key: &str) -> Option<bool> {
    let key_pattern = format!("\"{key}\"");
    let key_start = content.find(&key_pattern)?;
    let after_key = &content[key_start + key_pattern.len()..];
    let colon_idx = after_key.find(':')?;
    let value_part = after_key[colon_idx + 1..].trim_start();

    if value_part.starts_with("true") {
        Some(true)
    } else if value_part.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(not(test))]
fn extract_json_object_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let key_pattern = format!("\"{key}\"");
    let key_start = content.find(&key_pattern)?;
    let after_key = &content[key_start + key_pattern.len()..];
    let colon_idx = after_key.find(':')?;
    let value_part = after_key[colon_idx + 1..].trim_start();
    let open_idx = value_part.find('{')?;

    let mut depth = 0usize;
    let mut end_idx = None;
    for (idx, ch) in value_part[open_idx..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end_idx = Some(open_idx + idx + 1);
                    break;
                }
            }
            _ => {}
        }
    }

    let end_idx = end_idx?;
    Some(&value_part[open_idx..end_idx])
}

#[cfg(not(test))]
fn parse_override_map_json(raw_object: &str) -> BTreeMap<DateYmd, f64> {
    let mut map = BTreeMap::new();
    let Some(open_brace) = raw_object.find('{') else {
        return map;
    };
    let Some(close_brace) = raw_object.rfind('}') else {
        return map;
    };
    if close_brace <= open_brace {
        return map;
    }

    let body = &raw_object[open_brace + 1..close_brace];
    for raw_entry in body.split(',') {
        let entry = raw_entry.trim();
        if entry.is_empty() {
            continue;
        }

        let Some(colon_idx) = entry.find(':') else {
            continue;
        };
        let raw_key = entry[..colon_idx].trim();
        let raw_value = entry[colon_idx + 1..].trim();
        if raw_key.len() < 2 || !raw_key.starts_with('"') || !raw_key.ends_with('"') {
            continue;
        }

        let date_str = &raw_key[1..raw_key.len() - 1];
        let Some(date) = DateYmd::parse_yyyy_mm_dd(date_str) else {
            continue;
        };

        let Ok(apr) = raw_value.parse::<f64>() else {
            continue;
        };
        if !apr.is_finite() || apr < 0.0 {
            continue;
        }

        map.insert(date, apr);
    }

    map
}

#[cfg(not(test))]
fn parse_recurring_extra_map_json(raw_object: &str) -> BTreeMap<RecurringExtraPaymentKey, f64> {
    let mut map = BTreeMap::new();
    let Some(open_brace) = raw_object.find('{') else {
        return map;
    };
    let Some(close_brace) = raw_object.rfind('}') else {
        return map;
    };
    if close_brace <= open_brace {
        return map;
    }

    let body = &raw_object[open_brace + 1..close_brace];
    for raw_entry in body.split(',') {
        let entry = raw_entry.trim();
        if entry.is_empty() {
            continue;
        }

        let Some(colon_idx) = entry.find(':') else {
            continue;
        };
        let raw_key = entry[..colon_idx].trim();
        let raw_value = entry[colon_idx + 1..].trim();
        if raw_key.len() < 2 || !raw_key.starts_with('"') || !raw_key.ends_with('"') {
            continue;
        }

        let key_str = &raw_key[1..raw_key.len() - 1];
        let Some((start_date_raw, month_day_raw)) = key_str.split_once('|') else {
            continue;
        };
        let Some((month_raw, day_raw)) = month_day_raw.split_once('-') else {
            continue;
        };

        let Some(start_date) = DateYmd::parse_yyyy_mm_dd(start_date_raw) else {
            continue;
        };
        let Ok(month) = month_raw.parse::<u32>() else {
            continue;
        };
        let Ok(day) = day_raw.parse::<u32>() else {
            continue;
        };
        if month == 0 || month > 12 || day == 0 || day > 31 {
            continue;
        }
        let Ok(amount) = raw_value.parse::<f64>() else {
            continue;
        };
        if !amount.is_finite() || amount <= 0.0 {
            continue;
        }

        let key = RecurringExtraPaymentKey {
            start_date,
            month,
            day,
        };
        let entry = map.entry(key).or_insert(0.0);
        *entry += amount;
    }

    map
}

fn first_recurring_occurrence_in_range(
    key: RecurringExtraPaymentKey,
    range_start: DateYmd,
    range_end: DateYmd,
) -> Option<DateYmd> {
    let mut year = range_start.year.max(key.start_date.year);
    while year <= range_end.year {
        let day = key.day.min(last_day_of_month(year, key.month));
        let Some(candidate) = DateYmd::from_ymd_opt(year, key.month, day) else {
            year += 1;
            continue;
        };
        if candidate >= key.start_date && candidate >= range_start && candidate <= range_end {
            return Some(candidate);
        }
        year += 1;
    }
    None
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

fn last_day_of_month(year: i32, month: u32) -> u32 {
    for day in (28..=31).rev() {
        if DateYmd::from_ymd_opt(year, month, day).is_some() {
            return day;
        }
    }

    28
}

#[cfg(test)]
mod tests {
    use super::{App, ResetConfirmOption, RowActionOption, ScheduleDisplayRow};
    use crate::model::{DateYmd, InterestBasisMode};

    #[test]
    fn term_reduction_prunes_overrides_and_clamps_selected_month() {
        let mut app = App::default();
        app.inputs[5] = "2026-09-12".to_string();
        app.inputs[6] = "15".to_string();
        app.recalculate();

        let keep_date = DateYmd::from_ymd_opt(2036, 9, 15).expect("valid date");
        let pruned_date = DateYmd::from_ymd_opt(2046, 9, 15).expect("valid date");
        app.rate_overrides.insert(keep_date, 7.0);
        app.rate_overrides.insert(pruned_date, 8.0);
        app.schedule_selected_index = 239;
        app.inputs[4] = "10".to_string();

        app.recalculate();

        assert_eq!(app.schedule_selected_index, 119);
        assert_eq!(app.selected_month, 120);
        assert!(app.rate_overrides.contains_key(&keep_date));
        assert!(!app.rate_overrides.contains_key(&pruned_date));
    }

    #[test]
    fn payment_row_opens_action_popup_and_selection_opens_apr_dialog() {
        let mut app = App::default();
        app.focus_schedule();
        app.move_schedule_selection(14);
        app.open_row_edit_popup();

        assert!(app.is_row_action_popup_open());
        assert_eq!(
            app.row_action_selected_option(),
            RowActionOption::AddExtraPayment
        );

        app.row_action_move_down();
        assert_eq!(
            app.row_action_selected_option(),
            RowActionOption::AddAprChange
        );
        app.row_action_move_down();
        assert_eq!(
            app.row_action_selected_option(),
            RowActionOption::AddRecurringExtraPayment
        );
        app.row_action_move_up();
        app.apply_row_action_popup_selection();
        assert!(app.is_apr_edit_popup_open());
    }

    #[test]
    fn non_payment_date_override_creates_marker_row_targeting_next_month() {
        let mut app = App::default();
        app.inputs[5] = "2026-09-12".to_string();
        app.inputs[6] = "15".to_string();
        app.recalculate();

        let marker_date = DateYmd::from_ymd_opt(2026, 11, 1).expect("valid date");
        app.rate_overrides.insert(marker_date, 8.0);
        app.recalculate();

        let marker_index = app
            .schedule_rows
            .iter()
            .position(|row| {
                matches!(
                    row,
                    ScheduleDisplayRow::AprChangeMarker { effective_date, target_month, .. }
                        if *effective_date == marker_date && *target_month == 2
                )
            })
            .expect("marker row should exist");

        app.schedule_selected_index = marker_index;
        app.sync_selected_month_from_selection();
        app.recalculate();

        let metrics = app.metrics.as_ref().expect("metrics should exist");
        assert_eq!(app.selected_month, 2);
        assert_eq!(metrics.selected_month, 2);
    }

    #[test]
    fn payment_date_override_creates_marker_row() {
        let mut app = App::default();
        app.inputs[5] = "2026-09-12".to_string();
        app.inputs[6] = "15".to_string();
        app.recalculate();

        let month_two_payment_date = DateYmd::from_ymd_opt(2026, 11, 15).expect("valid date");
        app.rate_overrides.insert(month_two_payment_date, 7.0);
        app.recalculate();

        assert!(app.schedule_rows.iter().any(|row| {
            matches!(
                row,
                ScheduleDisplayRow::AprChangeMarker { effective_date, .. }
                    if *effective_date == month_two_payment_date
            )
        }));
    }

    #[test]
    fn marker_rows_open_corresponding_dialog_directly() {
        let mut app = App::default();
        app.inputs[5] = "2026-09-12".to_string();
        app.inputs[6] = "15".to_string();
        let apr_date = DateYmd::from_ymd_opt(2026, 11, 1).expect("valid date");
        let extra_date = DateYmd::from_ymd_opt(2026, 12, 1).expect("valid date");
        let recurring_date = DateYmd::from_ymd_opt(2026, 11, 20).expect("valid date");
        app.rate_overrides.insert(apr_date, 7.35);
        app.extra_payments.insert(extra_date, 2500.0);
        app.recurring_extra_payments.insert(
            super::RecurringExtraPaymentKey {
                start_date: DateYmd::from_ymd_opt(2026, 9, 12).expect("valid date"),
                month: 11,
                day: 20,
            },
            1750.0,
        );
        app.recalculate();
        app.focus_schedule();

        let apr_marker_index = app
            .schedule_rows
            .iter()
            .position(|row| {
                matches!(row, ScheduleDisplayRow::AprChangeMarker { effective_date, .. } if *effective_date == apr_date)
            })
            .expect("APR marker row should exist");
        app.schedule_selected_index = apr_marker_index;
        app.open_row_edit_popup();
        assert!(app.is_apr_edit_popup_open());
        assert_eq!(app.apr_edit_date_input_buffer, apr_date.format_yyyy_mm_dd());
        assert_eq!(app.apr_edit_apr_input_buffer, "7.35");

        app.close_row_edit_popup();

        let extra_marker_index = app
            .schedule_rows
            .iter()
            .position(|row| {
                matches!(row, ScheduleDisplayRow::ExtraPaymentMarker { effective_date, .. } if *effective_date == extra_date)
            })
            .expect("Extra marker row should exist");
        app.schedule_selected_index = extra_marker_index;
        app.open_row_edit_popup();
        assert!(app.is_extra_edit_popup_open());
        assert_eq!(
            app.extra_edit_date_input_buffer,
            extra_date.format_yyyy_mm_dd()
        );
        assert_eq!(app.extra_edit_amount_input_buffer, "2500");

        app.close_row_edit_popup();

        let recurring_marker_index = app
            .schedule_rows
            .iter()
            .position(|row| {
                matches!(
                    row,
                    ScheduleDisplayRow::RecurringExtraPaymentMarker { effective_date, .. }
                        if *effective_date == recurring_date
                )
            })
            .expect("Recurring marker row should exist");
        app.schedule_selected_index = recurring_marker_index;
        app.open_row_edit_popup();
        assert!(app.is_recurring_extra_edit_popup_open());
        assert_eq!(app.recurring_edit_start_date_input_buffer, "2026-09-12");
        assert_eq!(app.recurring_edit_annual_date_input_buffer, "11-20");
        assert_eq!(app.recurring_edit_amount_input_buffer, "1750");
    }

    #[test]
    fn apr_dialog_applies_and_clears_override() {
        let mut app = App::default();
        app.focus_schedule();
        app.move_schedule_selection(14);
        let selected_date = app
            .selected_schedule_row()
            .expect("selected row should exist")
            .date();

        app.open_row_edit_popup();
        app.row_action_move_down();
        app.apply_row_action_popup_selection();
        assert!(app.is_apr_edit_popup_open());

        app.apr_edit_apr_input_buffer = "7.25".to_string();
        app.apr_edit_active_row = 2;
        app.activate_apr_edit_row_on_enter();

        assert_eq!(app.override_for_date(selected_date), Some(7.25));
        assert!(!app.is_apr_edit_popup_open());

        app.open_row_edit_popup();
        app.row_action_move_down();
        app.apply_row_action_popup_selection();
        app.apr_edit_active_row = 3;
        app.activate_apr_edit_row_on_enter();
        assert_eq!(app.override_for_date(selected_date), None);
        assert!(!app.is_apr_edit_popup_open());
    }

    #[test]
    fn extra_dialog_replaces_existing_amount_and_can_clear() {
        let mut app = App::default();
        app.focus_schedule();
        app.move_schedule_selection(14);
        let selected_date = app
            .selected_schedule_row()
            .expect("selected row should exist")
            .date();
        app.extra_payments.insert(selected_date, 1500.0);
        app.recalculate();

        app.open_row_edit_popup();
        assert!(app.is_row_action_popup_open());
        app.apply_row_action_popup_selection();
        assert!(app.is_extra_edit_popup_open());
        assert_eq!(app.extra_edit_amount_input_buffer, "1500");

        app.extra_edit_amount_input_buffer = "2500".to_string();
        app.extra_edit_active_row = 2;
        app.activate_extra_edit_row_on_enter();
        assert_eq!(app.extra_payment_for_date(selected_date), Some(2500.0));
        assert!(!app.is_extra_edit_popup_open());

        app.open_row_edit_popup();
        app.apply_row_action_popup_selection();
        app.extra_edit_amount_input_buffer = "0".to_string();
        app.extra_edit_active_row = 2;
        app.activate_extra_edit_row_on_enter();
        assert_eq!(app.extra_payment_for_date(selected_date), None);
    }

    #[test]
    fn recurring_dialog_can_add_edit_and_clear_rule() {
        let mut app = App::default();
        app.focus_schedule();
        app.move_schedule_selection(14);
        let selected_date = app
            .selected_schedule_row()
            .expect("selected row should exist")
            .date();

        app.open_row_edit_popup();
        assert!(app.is_row_action_popup_open());
        app.row_action_move_down();
        app.row_action_move_down();
        app.apply_row_action_popup_selection();
        assert!(app.is_recurring_extra_edit_popup_open());

        app.recurring_edit_start_date_input_buffer = selected_date.format_yyyy_mm_dd();
        app.recurring_edit_annual_date_input_buffer =
            format!("{:02}-{:02}", selected_date.month, selected_date.day);
        app.recurring_edit_amount_input_buffer = "1200".to_string();
        app.recurring_edit_active_row = 3;
        app.activate_recurring_extra_edit_row_on_enter();
        assert!(!app.is_recurring_extra_edit_popup_open());
        assert_eq!(app.recurring_extra_payments.len(), 1);

        let key = super::RecurringExtraPaymentKey {
            start_date: selected_date,
            month: selected_date.month,
            day: selected_date.day,
        };
        assert_eq!(
            app.recurring_extra_payments.get(&key).copied(),
            Some(1200.0)
        );

        app.open_row_edit_popup();
        app.row_action_move_down();
        app.row_action_move_down();
        app.apply_row_action_popup_selection();
        app.recurring_edit_active_row = 4;
        app.activate_recurring_extra_edit_row_on_enter();
        assert_eq!(app.recurring_extra_payments.get(&key), None);
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

    #[test]
    fn reset_confirmation_popup_cancel_and_confirm() {
        let mut app = App::default();
        app.inputs[0] = "123456".to_string();
        app.round_payments_up = true;
        app.rate_overrides
            .insert(DateYmd::from_ymd_opt(2026, 10, 1).expect("valid date"), 7.5);

        app.open_reset_confirm_popup();
        assert!(app.is_reset_confirm_popup_open);
        assert_eq!(
            app.reset_confirm_selected_option(),
            ResetConfirmOption::Cancel
        );

        app.apply_reset_confirm_selection();
        assert!(!app.is_reset_confirm_popup_open);
        assert_eq!(app.inputs[0], "123456");
        assert!(app.round_payments_up);
        assert_eq!(app.rate_overrides.len(), 1);

        app.open_reset_confirm_popup();
        app.reset_confirm_move_down();
        assert_eq!(
            app.reset_confirm_selected_option(),
            ResetConfirmOption::ConfirmReset
        );
        app.apply_reset_confirm_selection();
        assert!(!app.is_reset_confirm_popup_open);
        assert_eq!(app.inputs[0], "300000");
        assert!(!app.round_payments_up);
        assert!(app.rate_overrides.is_empty());
    }

    #[test]
    fn interest_basis_popup_selection_applies_mode() {
        let mut app = App::default();
        assert_eq!(app.interest_basis_mode, InterestBasisMode::Act365Fixed);
        assert!(!app.is_interest_basis_popup_open);

        app.open_interest_basis_popup();
        assert!(app.is_interest_basis_popup_open);
        assert_eq!(
            app.interest_basis_popup_selected_mode(),
            InterestBasisMode::Act365Fixed
        );

        app.interest_basis_popup_move_down();
        app.interest_basis_popup_move_down();
        assert_eq!(
            app.interest_basis_popup_selected_mode(),
            InterestBasisMode::ThirtyE360
        );

        assert_eq!(app.interest_basis_mode, InterestBasisMode::Act365Fixed);
        app.apply_interest_basis_popup_selection();
        assert_eq!(app.interest_basis_mode, InterestBasisMode::ThirtyE360);
        assert!(!app.is_interest_basis_popup_open);
    }
}
