mod app;
mod model;
mod ui;

use std::{io, time::Duration};

use app::{App, FieldId};
use color_eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

fn main() -> Result<()> {
    color_eyre::install()?;

    let mut terminal = init_terminal()?;
    let app = App::default();
    let run_result = run_app(&mut terminal, app);
    restore_terminal(&mut terminal)?;

    run_result
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, mut app: App) -> Result<()> {
    let tick_rate = Duration::from_millis(200);

    loop {
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        if !event::poll(tick_rate)? {
            continue;
        }

        let Event::Key(key_event) = event::read()? else {
            continue;
        };

        if key_event.kind != KeyEventKind::Press {
            continue;
        }

        if key_event.code == KeyCode::Char('q') && key_event.modifiers.is_empty() {
            return Ok(());
        }

        if app.is_row_action_popup_open() {
            handle_row_action_popup_key(&mut app, key_event);
        } else if app.is_apr_edit_popup_open() {
            handle_apr_edit_popup_key(&mut app, key_event);
        } else if app.is_extra_edit_popup_open() {
            handle_extra_edit_popup_key(&mut app, key_event);
        } else if app.is_interest_basis_popup_open {
            handle_interest_basis_popup_key(&mut app, key_event);
        } else {
            handle_main_key(&mut app, key_event);
        }
    }
}

fn handle_main_key(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Char('r') if key_event.modifiers.is_empty() => app.reset(),
        KeyCode::Char('R') if key_event.modifiers == KeyModifiers::SHIFT => app.reset(),
        KeyCode::Left => app.focus_inputs(),
        KeyCode::Right => app.focus_schedule(),
        KeyCode::Up => app.navigate_up(),
        KeyCode::Down => app.navigate_down(),
        KeyCode::Char('k') if key_event.modifiers.is_empty() => app.navigate_up(),
        KeyCode::Char('j') if key_event.modifiers.is_empty() => app.navigate_down(),
        KeyCode::PageUp if app.is_schedule_focused() => app.move_schedule_selection_by_page(-1),
        KeyCode::PageDown if app.is_schedule_focused() => app.move_schedule_selection_by_page(1),
        KeyCode::Home if app.is_schedule_focused() => app.move_schedule_selection_to_start(),
        KeyCode::End if app.is_schedule_focused() => app.move_schedule_selection_to_end(),
        KeyCode::Tab => app.toggle_focus_area(),
        KeyCode::BackTab => app.toggle_focus_area(),
        KeyCode::Char(' ')
            if !app.is_schedule_focused() && app.active_field() == FieldId::RoundPaymentsUp =>
        {
            app.toggle_round_payments_up()
        }
        KeyCode::Char(' ')
            if !app.is_schedule_focused() && app.active_field() == FieldId::InterestBasis =>
        {
            app.open_interest_basis_popup()
        }
        KeyCode::Enter if app.is_schedule_focused() => app.open_row_edit_popup(),
        KeyCode::Enter
            if !app.is_schedule_focused() && app.active_field() == FieldId::RoundPaymentsUp =>
        {
            app.toggle_round_payments_up()
        }
        KeyCode::Enter
            if !app.is_schedule_focused() && app.active_field() == FieldId::InterestBasis =>
        {
            app.open_interest_basis_popup()
        }
        KeyCode::Enter => app.recalculate(),
        KeyCode::Backspace if !app.is_schedule_focused() => app.backspace(),
        KeyCode::Char(c)
            if !app.is_schedule_focused()
                && (key_event.modifiers.is_empty()
                    || key_event.modifiers == KeyModifiers::SHIFT) =>
        {
            app.input_char(c);
        }
        _ => {}
    }
}

fn handle_row_action_popup_key(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc => app.close_row_edit_popup(),
        KeyCode::Enter => app.apply_row_action_popup_selection(),
        KeyCode::Up => app.row_action_move_up(),
        KeyCode::Down => app.row_action_move_down(),
        KeyCode::Char('k') if key_event.modifiers.is_empty() => app.row_action_move_up(),
        KeyCode::Char('j') if key_event.modifiers.is_empty() => app.row_action_move_down(),
        _ => {}
    }
}

fn handle_apr_edit_popup_key(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc => app.close_row_edit_popup(),
        KeyCode::Enter => app.activate_apr_edit_row_on_enter(),
        KeyCode::Up => app.apr_edit_move_up(),
        KeyCode::Down => app.apr_edit_move_down(),
        KeyCode::Char('k') if key_event.modifiers.is_empty() => app.apr_edit_move_up(),
        KeyCode::Char('j') if key_event.modifiers.is_empty() => app.apr_edit_move_down(),
        KeyCode::Backspace => app.apr_edit_input_backspace(),
        KeyCode::Char(c)
            if key_event.modifiers.is_empty() || key_event.modifiers == KeyModifiers::SHIFT =>
        {
            app.apr_edit_input_char(c);
        }
        _ => {}
    }
}

fn handle_extra_edit_popup_key(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc => app.close_row_edit_popup(),
        KeyCode::Enter => app.activate_extra_edit_row_on_enter(),
        KeyCode::Up => app.extra_edit_move_up(),
        KeyCode::Down => app.extra_edit_move_down(),
        KeyCode::Char('k') if key_event.modifiers.is_empty() => app.extra_edit_move_up(),
        KeyCode::Char('j') if key_event.modifiers.is_empty() => app.extra_edit_move_down(),
        KeyCode::Backspace => app.extra_edit_input_backspace(),
        KeyCode::Char(c)
            if key_event.modifiers.is_empty() || key_event.modifiers == KeyModifiers::SHIFT =>
        {
            app.extra_edit_input_char(c);
        }
        _ => {}
    }
}

fn handle_interest_basis_popup_key(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc => app.close_interest_basis_popup(),
        KeyCode::Enter => app.apply_interest_basis_popup_selection(),
        KeyCode::Up => app.interest_basis_popup_move_up(),
        KeyCode::Down => app.interest_basis_popup_move_down(),
        KeyCode::Char('k') if key_event.modifiers.is_empty() => app.interest_basis_popup_move_up(),
        KeyCode::Char('j') if key_event.modifiers.is_empty() => {
            app.interest_basis_popup_move_down()
        }
        _ => {}
    }
}
