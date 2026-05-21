use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthChar;

use crate::soul::KimiSoul;

pub struct ShellUI {
    soul: Option<KimiSoul>,
    soul_arc: Option<Arc<tokio::sync::Mutex<Option<KimiSoul>>>>,
    input: String,
    cursor_position: usize,
    messages: Vec<(String, String)>,
    mode: ShellMode,
    exit: bool,
    thinking: bool,
    state: AppState,
    // Cached values for drawing when soul is running
    cached_model_name: String,
    cached_plan_mode: bool,
    cached_session_id: String,
    cached_work_dir: String,
    // Completion state
    completions: Vec<String>,
    completion_index: usize,
    show_completions: bool,
    // Ctrl-C tip
    show_ctrl_c_tip: bool,
    ctrl_c_tip_timer: Option<Instant>,
    // Welcome panel inside TUI
    show_welcome: bool,
    // Input area width for cursor calculation
    last_frame_width: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellMode {
    Agent,
    Shell,
}

enum AppState {
    Idle,
    Running(tokio::task::JoinHandle<crate::exception::Result<String>>),
}

impl ShellUI {
    pub fn new(soul: KimiSoul) -> Self {
        let cached_model_name = soul
            .llm
            .as_ref()
            .map(|l| l.model_name.clone())
            .unwrap_or_else(|| "no model".to_string());
        let cached_plan_mode = soul.plan_mode;
        let cached_session_id = soul.session.id.clone();
        let cached_work_dir = std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        Self {
            soul: Some(soul),
            soul_arc: None,
            input: String::new(),
            cursor_position: 0,
            messages: Vec::new(),
            mode: ShellMode::Agent,
            exit: false,
            thinking: false,
            state: AppState::Idle,
            cached_model_name,
            cached_plan_mode,
            cached_session_id,
            cached_work_dir,
            completions: Vec::new(),
            completion_index: 0,
            show_completions: false,
            show_ctrl_c_tip: false,
            ctrl_c_tip_timer: None,
            show_welcome: true,
            last_frame_width: 80,
        }
    }

    pub async fn run(&mut self, initial_prompt: Option<String>) -> io::Result<bool> {
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        crossterm::execute!(
            stdout,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        if let Some(prompt) = initial_prompt {
            self.input = prompt;
            self.cursor_position = self.input.len();
            self.submit_input().await;
        }

        let result = self.run_loop(&mut terminal).await;

        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        println!("Bye!");

        result
    }

    fn welcome_lines(&self) -> Vec<Line<'static>> {
        let version = crate::constant::get_version();
        let dir = self.cached_work_dir.clone();
        let session = self.cached_session_id.clone();
        let model = self.cached_model_name.clone();

        let style = Style::default().fg(Color::White);
        let label_style = Style::default().fg(Color::Cyan);

        vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  Welcome to Kimi Code CLI!",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" v{}", version), Style::default().fg(Color::Gray)),
            ]),
            Line::from(Span::styled(
                "  Send /help for help information.",
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Directory: ", label_style),
                Span::styled(dir, style),
            ]),
            Line::from(vec![
                Span::styled("  Session:   ", label_style),
                Span::styled(session, style),
            ]),
            Line::from(vec![
                Span::styled("  Model:     ", label_style),
                Span::styled(model, style),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  Tip: Spot a bug or have feedback? Type /feedback right in this session.",
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
        ]
    }

    async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<bool> {
        let mut reader = crossterm::event::EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(50));

        while !self.exit {
            terminal.draw(|f| self.draw(f))?;

            self.check_task_completion().await;
            self.update_ctrl_c_tip();

            tokio::select! {
                _ = tick.tick() => {},
                Some(Ok(event)) = reader.next() => {
                    if let Event::Key(key) = event {
                        self.handle_key_event(key).await;
                    }
                }
            }
        }

        Ok(true)
    }

    async fn check_task_completion(&mut self) {
        if let AppState::Running(handle) = &mut self.state {
            if handle.is_finished() {
                match handle.await {
                    Ok(result) => {
                        if let Some(arc) = self.soul_arc.take() {
                            if let Ok(mut guard) = arc.try_lock() {
                                self.soul = guard.take();
                            }
                        }
                        self.state = AppState::Idle;
                        self.thinking = false;
                        match result {
                            Ok(response) => {
                                self.messages.push(("assistant".to_string(), response));
                            }
                            Err(e) => {
                                self.messages
                                    .push(("error".to_string(), format!("Error: {}", e)));
                            }
                        }
                    }
                    Err(e) if e.is_cancelled() => {
                        if let Some(arc) = self.soul_arc.take() {
                            if let Ok(mut guard) = arc.try_lock() {
                                self.soul = guard.take();
                            }
                        }
                        self.state = AppState::Idle;
                        self.thinking = false;
                        self.messages
                            .push(("system".to_string(), "Interrupted by user".to_string()));
                    }
                    Err(e) => {
                        if let Some(arc) = self.soul_arc.take() {
                            if let Ok(mut guard) = arc.try_lock() {
                                self.soul = guard.take();
                            }
                        }
                        self.state = AppState::Idle;
                        self.thinking = false;
                        self.messages
                            .push(("error".to_string(), format!("Task panicked: {}", e)));
                    }
                }
            }
        }
    }

    fn update_ctrl_c_tip(&mut self) {
        if self.show_ctrl_c_tip {
            if let Some(timer) = self.ctrl_c_tip_timer {
                if timer.elapsed() > Duration::from_secs(3) {
                    self.show_ctrl_c_tip = false;
                    self.ctrl_c_tip_timer = None;
                }
            }
        }
    }

    async fn handle_key_event(&mut self, key: KeyEvent) {
        // If completions are showing, handle navigation first
        if self.show_completions {
            match key.code {
                KeyCode::Up => {
                    if self.completion_index > 0 {
                        self.completion_index -= 1;
                    }
                    return;
                }
                KeyCode::Down => {
                    if self.completion_index + 1 < self.completions.len() {
                        self.completion_index += 1;
                    }
                    return;
                }
                KeyCode::Enter | KeyCode::Tab => {
                    self.accept_completion();
                    return;
                }
                KeyCode::Esc => {
                    self.show_completions = false;
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    self.insert_char('\n');
                    self.refresh_completions();
                } else {
                    self.submit_input().await;
                }
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_char('\n');
                self.refresh_completions();
            }
            KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_mode();
            }
            KeyCode::BackTab => {
                if let Some(ref mut soul) = self.soul {
                    soul.toggle_plan_mode();
                    self.cached_plan_mode = soul.plan_mode;
                }
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // TODO: open external editor
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match &self.state {
                    AppState::Idle => {
                        if self.show_ctrl_c_tip {
                            self.exit = true;
                        } else {
                            self.show_ctrl_c_tip = true;
                            self.ctrl_c_tip_timer = Some(Instant::now());
                        }
                    }
                    AppState::Running(handle) => {
                        handle.abort();
                        // check_task_completion will handle the cancellation
                    }
                }
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.input.is_empty() {
                    self.exit = true;
                }
            }
            KeyCode::Char('\t') => {
                self.handle_tab();
            }
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
                    self.insert_char(c);
                    self.refresh_completions();
                }
            }
            KeyCode::Backspace => {
                self.delete_char_before_cursor();
                self.refresh_completions();
            }
            KeyCode::Delete => {
                self.delete_char_at_cursor();
                self.refresh_completions();
            }
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor_position < self.input.len() {
                    self.cursor_position += 1;
                }
            }
            KeyCode::Home => {
                self.cursor_position = 0;
            }
            KeyCode::End => {
                self.cursor_position = self.input.len();
            }
            KeyCode::Up => {
                // TODO: history navigation
            }
            KeyCode::Down => {
                // TODO: history navigation
            }
            _ => {}
        }
    }

    fn handle_tab(&mut self) {
        if self.input.starts_with('/') && !self.show_completions {
            self.refresh_completions();
            if !self.completions.is_empty() {
                self.show_completions = true;
                self.completion_index = 0;
            }
        } else if self.show_completions && !self.completions.is_empty() {
            self.completion_index = (self.completion_index + 1) % self.completions.len();
        } else if self.completions.len() == 1 {
            self.accept_completion();
        }
    }

    fn refresh_completions(&mut self) {
        if !self.input.starts_with('/') {
            self.show_completions = false;
            self.completions.clear();
            return;
        }

        let prefix = &self.input[1..];
        let commands = self.get_available_commands();
        self.completions = commands
            .into_iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .collect();

        if self.completions.is_empty() {
            self.show_completions = false;
        } else if self.completions.len() == 1 {
            // Auto-accept single match
            self.accept_completion();
        }
    }

    fn get_available_commands(&self) -> Vec<String> {
        let mut commands = vec![
            "clear".to_string(),
            "reset".to_string(),
            "yolo".to_string(),
            "afk".to_string(),
            "plan".to_string(),
            "compact".to_string(),
            "exit".to_string(),
            "help".to_string(),
            "model".to_string(),
        ];

        // Add commands from soul's slash registry if available
        if let Some(ref soul) = self.soul {
            for (name, _desc, aliases) in soul.list_slash_commands() {
                commands.push(name);
                commands.extend(aliases);
            }
        }

        commands.sort();
        commands.dedup();
        commands
    }

    fn accept_completion(&mut self) {
        if self.completions.is_empty() {
            return;
        }
        let idx = self.completion_index.min(self.completions.len() - 1);
        let completion = &self.completions[idx];
        self.input = format!("/{}", completion);
        self.cursor_position = self.input.len();
        self.show_completions = false;
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    fn delete_char_before_cursor(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.input.remove(self.cursor_position);
        }
    }

    fn delete_char_at_cursor(&mut self) {
        if self.cursor_position < self.input.len() {
            self.input.remove(self.cursor_position);
        }
    }

    fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            ShellMode::Agent => ShellMode::Shell,
            ShellMode::Shell => ShellMode::Agent,
        };
    }

    async fn submit_input(&mut self) {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return;
        }

        self.show_welcome = false;
        self.show_completions = false;
        self.completions.clear();

        // Handle /exit
        if input == "/exit" {
            self.exit = true;
            return;
        }

        // Handle slash commands
        if let Some(ref mut soul) = self.soul {
            if let Some(call) = crate::soul::slash::parse_slash_command_call(&input) {
                if let Some(cmd) = soul.slash_registry.get(&call.name) {
                    let func = cmd.func.clone();
                    (func)(soul, &call.args).await;
                    self.input.clear();
                    self.cursor_position = 0;
                    return;
                }
            }
        }

        let mode_str = match self.mode {
            ShellMode::Agent => "agent",
            ShellMode::Shell => "shell",
        };

        self.messages.push((mode_str.to_string(), input.clone()));
        self.input.clear();
        self.cursor_position = 0;
        self.thinking = true;

        // Run the soul in a background task
        if let Some(soul) = self.soul.take() {
            // Update cache before moving
            self.cached_model_name = soul
                .llm
                .as_ref()
                .map(|l| l.model_name.clone())
                .unwrap_or_else(|| "no model".to_string());
            self.cached_plan_mode = soul.plan_mode;

            let soul_arc = Arc::new(tokio::sync::Mutex::new(Some(soul)));
            self.soul_arc = Some(Arc::clone(&soul_arc));

            let handle = tokio::spawn(async move {
                let mut guard = soul_arc.lock().await;
                let mut soul = guard.take().unwrap();
                let result = soul.run(&input).await;
                *guard = Some(soul);
                result
            });

            self.state = AppState::Running(handle);
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        self.last_frame_width = frame.area().width;

        let input_height = self.calculate_input_height();
        let completion_height = if self.show_completions {
            (self.completions.len() as u16 + 2).min(6)
        } else {
            0
        };
        let tip_height = if self.show_ctrl_c_tip { 1 } else { 0 };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(completion_height),
                Constraint::Length(input_height),
                Constraint::Length(tip_height),
                Constraint::Length(1),
            ])
            .split(frame.area());

        let mut chunk_idx = 0;

        // Chat area
        let chat_area = chunks[chunk_idx];
        chunk_idx += 1;
        let mut text_lines = Vec::new();

        for (role, content) in &self.messages {
            let prefix = match role.as_str() {
                "agent" => "✨ ",
                "shell" => "$ ",
                "assistant" => "🤖 ",
                "error" => "❌ ",
                "system" => "ℹ️  ",
                _ => "",
            };
            let style = match role.as_str() {
                "agent" => Style::default().fg(Color::Cyan),
                "shell" => Style::default().fg(Color::Green),
                "assistant" => Style::default().fg(Color::White),
                "error" => Style::default().fg(Color::Red),
                "system" => Style::default().fg(Color::Yellow),
                _ => Style::default(),
            };

            for line in content.lines() {
                text_lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(line.to_string(), style),
                ]));
            }
            text_lines.push(Line::from(""));
        }

        if self.thinking {
            text_lines.push(Line::from(vec![
                Span::styled("🤖 ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    "Thinking...",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        if self.show_welcome && self.messages.is_empty() && !self.thinking {
            text_lines.extend(self.welcome_lines());
        }

        let chat = Paragraph::new(Text::from(text_lines))
            .block(Block::default().borders(Borders::ALL).title("Chat"))
            .wrap(Wrap { trim: false })
            .scroll((0, 0));
        frame.render_widget(chat, chat_area);

        // Completion popup
        if self.show_completions && completion_height > 0 {
            let comp_area = chunks[chunk_idx];
            chunk_idx += 1;
            let comp_text: Vec<Line> = self
                .completions
                .iter()
                .enumerate()
                .map(|(i, cmd)| {
                    let style = if i == self.completion_index {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(format!("  /{}", cmd), style))
                })
                .collect();
            let comp_widget = Paragraph::new(Text::from(comp_text))
                .block(Block::default().borders(Borders::ALL).title("Commands"));
            frame.render_widget(comp_widget, comp_area);
        } else {
            chunk_idx += 1;
        }

        // Input area
        let input_area = chunks[chunk_idx];
        chunk_idx += 1;
        let mode_indicator = match self.mode {
            ShellMode::Agent => "── input ──",
            ShellMode::Shell => "─",
        };
        let prompt = match self.mode {
            ShellMode::Agent => "✨ ",
            ShellMode::Shell => "$ ",
        };

        let input_block = Block::default().borders(Borders::ALL).title(mode_indicator);

        // Render multiline input
        let input_with_prompt = format!("{}{}", prompt, self.input);
        let input_paragraph = Paragraph::new(input_with_prompt.clone()).block(input_block);
        frame.render_widget(input_paragraph, input_area);

        // Set cursor position
        let cursor_pos = self.calculate_cursor_position(input_area, prompt);
        frame.set_cursor_position(cursor_pos);

        // Ctrl-C tip
        if self.show_ctrl_c_tip && tip_height > 0 {
            let tip_area = chunks[chunk_idx];
            chunk_idx += 1;
            let tip = Paragraph::new("Tip: press Ctrl-D or send 'exit' to quit")
                .style(Style::default().fg(Color::Yellow));
            frame.render_widget(tip, tip_area);
        } else {
            chunk_idx += 1;
        }

        // Bottom toolbar
        let toolbar_area = chunks[chunk_idx];
        let plan_indicator = if self.cached_plan_mode {
            " · plan"
        } else {
            ""
        };
        let toolbar_text = format!(
            "{} | {}{} | {}",
            match self.mode {
                ShellMode::Agent => "agent",
                ShellMode::Shell => "shell",
            },
            self.cached_model_name,
            plan_indicator,
            self.cached_work_dir
        );
        let toolbar = Paragraph::new(toolbar_text).style(Style::default().fg(Color::Gray));
        frame.render_widget(toolbar, toolbar_area);
    }

    fn calculate_input_height(&self) -> u16 {
        let prompt_len = match self.mode {
            ShellMode::Agent => "✨ ".len(),
            ShellMode::Shell => "$ ".len(),
        };
        let available_width = (self.last_frame_width as usize).saturating_sub(2 + prompt_len);
        if available_width == 0 {
            return 3;
        }

        let text_with_prompt = format!(
            "{}{}",
            match self.mode {
                ShellMode::Agent => "✨ ",
                ShellMode::Shell => "$ ",
            },
            self.input
        );

        let mut lines_needed = 1usize;
        let mut current_line_width = 0usize;
        for c in text_with_prompt.chars() {
            if c == '\n' {
                lines_needed += 1;
                current_line_width = 0;
            } else {
                current_line_width += 1;
                if current_line_width > available_width {
                    lines_needed += 1;
                    current_line_width = 1;
                }
            }
        }

        (lines_needed as u16 + 2).clamp(5, 12)
    }

    fn calculate_cursor_position(&self, input_area: Rect, prompt: &str) -> (u16, u16) {
        let available_width = (input_area.width as usize).saturating_sub(2);

        let mut cursor_row = input_area.y + 1;
        let mut cursor_col = input_area.x + 1;

        let text_before_cursor = format!("{}{}", prompt, &self.input[..self.cursor_position]);

        let mut current_line_width = 0usize;
        for c in text_before_cursor.chars() {
            if c == '\n' {
                cursor_row += 1;
                cursor_col = input_area.x + 1;
                current_line_width = 0;
            } else {
                let w = c.width().unwrap_or(1);
                cursor_col += w as u16;
                current_line_width += w;
                if current_line_width >= available_width {
                    cursor_row += 1;
                    cursor_col = input_area.x + 1 + w as u16;
                    current_line_width = w;
                }
            }
        }

        (cursor_col, cursor_row)
    }
}
