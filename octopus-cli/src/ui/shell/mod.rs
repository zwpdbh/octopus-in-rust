use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    // Completion state
    completions: Vec<(String, String)>,
    completion_index: usize,
    show_completions: bool,
    // Ctrl-C tips shown below input area (stack on repeated presses)
    ctrl_c_tips: Vec<String>,
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
            completions: Vec::new(), // Vec<(name, description)>
            completion_index: 0,
            show_completions: false,
            ctrl_c_tips: Vec::new(),
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
        )?;
        terminal.show_cursor()?;

        println!("Bye!");

        result
    }

    fn welcome_lines(&self) -> Vec<Line<'static>> {
        let version = crate::constant::get_version();
        let model = self.cached_model_name.clone();

        let gray = Style::default().fg(Color::Gray);
        let yellow = Style::default().fg(Color::Yellow);
        let white = Style::default().fg(Color::White);

        vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Model: ", gray),
                Span::styled(model, white),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Tip: Spot a bug or have feedback? Type /feedback right in this session – every report makes Kimi better.", gray),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    format!("  New version available: {}. Run `cargo install --path octopus-cli` to upgrade.", version),
                    yellow,
                ),
            ]),
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
                        // Ctrl-C never exits; stack tips below input area
                        self.ctrl_c_tips.push(
                            "Tip: press Ctrl-D or send 'exit' to quit".to_string(),
                        );
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
            .filter(|(name, _desc)| name.starts_with(prefix))
            .collect();

        if self.completions.is_empty() {
            self.show_completions = false;
        } else {
            self.show_completions = true;
            self.completion_index = 0;
        }
    }

    fn get_available_commands(&self) -> Vec<(String, String)> {
        let mut commands: Vec<(String, String)> = Vec::new();

        // Hardcoded fallback commands when soul is not available (e.g. during task execution)
        let fallback = vec![
            ("add-dir".to_string(), "Add a directory to the workspace. Usage: /add-dir <path>. Run without args to list added dirs".to_string()),
            ("afk".to_string(), "Toggle afk mode (auto-dismiss AskUserQuestion, auto-approve tool calls)".to_string()),
            ("changelog".to_string(), "Show release notes".to_string()),
            ("clear".to_string(), "Clear the context".to_string()),
            ("compact".to_string(), "Compact the context (optionally with a custom focus, e.g. /compact keep db discussions)".to_string()),
            ("debug".to_string(), "Debug the context".to_string()),
            ("exit".to_string(), "Exit the CLI".to_string()),
            ("feedback".to_string(), "Submit feedback to make Kimi Code CLI better".to_string()),
            ("fork".to_string(), "Fork the current session (copy all history to a new session)".to_string()),
            ("help".to_string(), "Show help information".to_string()),
            ("hooks".to_string(), "List configured hooks".to_string()),
            ("mcp".to_string(), "Show MCP servers and tools".to_string()),
            ("model".to_string(), "Show or switch the current model".to_string()),
            ("new".to_string(), "Start a new session".to_string()),
            ("plan".to_string(), "Toggle plan mode. Usage: /plan [on|off|view|clear]".to_string()),
            ("sessions".to_string(), "List sessions and resume optionally".to_string()),
            ("theme".to_string(), "Switch terminal color theme (dark/light)".to_string()),
            ("title".to_string(), "Set or show the session title".to_string()),
            ("undo".to_string(), "Undo: fork the session at a previous turn and retry".to_string()),
            ("version".to_string(), "Show version information".to_string()),
            ("vis".to_string(), "Open Kimi Agent Tracing Visualizer in browser".to_string()),
            ("web".to_string(), "Open Kimi Code Web UI in browser".to_string()),
            ("yolo".to_string(), "Toggle YOLO mode (auto-approve all actions)".to_string()),
        ];
        commands.extend(fallback);

        // Add commands from soul's slash registry if available
        if let Some(ref soul) = self.soul {
            for (name, desc, aliases) in soul.list_slash_commands() {
                commands.push((name, desc.clone()));
                for alias in aliases {
                    commands.push((alias, desc.clone()));
                }
            }
        }

        commands.sort_by(|a, b| a.0.cmp(&b.0));
        commands.dedup_by(|a, b| a.0 == b.0);
        commands
    }

    fn accept_completion(&mut self) {
        if self.completions.is_empty() {
            return;
        }
        let idx = self.completion_index.min(self.completions.len() - 1);
        let (name, _desc) = &self.completions[idx];
        self.input = format!("/{}", name);
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
        self.ctrl_c_tips.clear();

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

        // Build chat content first so we can size the chat area to its content
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

        let chat_content_height = text_lines.len() as u16;
        let input_height = self.calculate_input_height();
        let completion_height = if self.show_completions {
            (self.completions.len() as u16).min(10)
        } else {
            0
        };
        let tip_height = self.ctrl_c_tips.len() as u16;
        // Chat gets exactly the space its content needs (capped at half terminal),
        // then a bottom spacer absorbs extra space so input isn't pushed to bottom.
        let chat_height = chat_content_height.max(3).min(frame.area().height / 2);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(chat_height),
                Constraint::Length(input_height),
                Constraint::Length(completion_height),
                Constraint::Length(tip_height),
                Constraint::Min(0),
            ])
            .split(frame.area());

        let mut chunk_idx = 0;

        // Chat area
        let chat_area = chunks[chunk_idx];
        chunk_idx += 1;

        let chat = Paragraph::new(Text::from(text_lines))
            .wrap(Wrap { trim: false })
            .scroll((0, 0));
        frame.render_widget(chat, chat_area);

        // Input area: top border with embedded title, bottom border as separator
        let input_area = chunks[chunk_idx];
        chunk_idx += 1;
        let mode_indicator = match self.mode {
            ShellMode::Agent => "input",
            ShellMode::Shell => "shell",
        };
        let prompt = match self.mode {
            ShellMode::Agent => "",
            ShellMode::Shell => "$ ",
        };

        let title_text = format!("── {} ──", mode_indicator);
        let input_block = Block::default()
            .title_top(Line::from(Span::styled(
                title_text,
                Style::default().fg(Color::DarkGray),
            )))
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray));

        // Render multiline input
        let input_with_prompt = format!("{}{}", prompt, self.input);
        let input_paragraph = Paragraph::new(input_with_prompt.clone()).block(input_block);
        frame.render_widget(input_paragraph, input_area);

        // Set cursor position
        let cursor_pos = self.calculate_cursor_position(input_area, prompt);
        frame.set_cursor_position(cursor_pos);

        // Completion menu (rendered below input)
        let comp_area = chunks[chunk_idx];
        if self.show_completions && !self.completions.is_empty() && comp_area.height > 0 {
            let cmd_col_width = 18usize;
            let comp_lines: Vec<Line> = self
                .completions
                .iter()
                .enumerate()
                .map(|(i, (name, desc))| {
                    let is_selected = i == self.completion_index;
                    let prefix = if is_selected { "› " } else { "  " };
                    let cmd_text = format!("{}/{}", prefix, name);
                    let cmd_visual_width = cmd_text.width();
                    let padding =
                        " ".repeat(cmd_col_width.saturating_sub(cmd_visual_width));

                    let cmd_style = if is_selected {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };

                    let max_desc_width =
                        (comp_area.width as usize).saturating_sub(cmd_col_width + 2);
                    let desc_text = if max_desc_width > 0 && desc.width() > max_desc_width {
                        let mut truncated = String::new();
                        let mut w = 0usize;
                        for ch in desc.chars() {
                            let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
                            if w + cw + 3 > max_desc_width {
                                break;
                            }
                            truncated.push(ch);
                            w += cw;
                        }
                        format!("{}...", truncated)
                    } else {
                        desc.clone()
                    };

                    Line::from(vec![
                        Span::styled(cmd_text, cmd_style),
                        Span::styled(padding, cmd_style),
                        Span::styled(desc_text, Style::default().fg(Color::DarkGray)),
                    ])
                })
                .collect();

            let comp_widget = Paragraph::new(Text::from(comp_lines));
            frame.render_widget(comp_widget, comp_area);
        }
        chunk_idx += 1;

        // Ctrl-C tips (rendered below completion area)
        if tip_height > 0 {
            let tip_area = chunks[chunk_idx];
            let tip_lines: Vec<Line> = self
                .ctrl_c_tips
                .iter()
                .map(|t| Line::from(Span::styled(t.clone(), Style::default().fg(Color::Yellow))))
                .collect();
            let tip_widget = Paragraph::new(Text::from(tip_lines));
            frame.render_widget(tip_widget, tip_area);
        }

    }

    fn calculate_input_height(&self) -> u16 {
        let prompt_len = match self.mode {
            ShellMode::Agent => 0,
            ShellMode::Shell => "$ ".len(),
        };
        let available_width = (self.last_frame_width as usize).saturating_sub(prompt_len);
        if available_width == 0 {
            return 3;
        }

        let text_with_prompt = format!(
            "{}{}",
            match self.mode {
                ShellMode::Agent => "",
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

        // +2 for top and bottom borders, min 5 (2 borders + 3 content rows)
        (lines_needed as u16 + 2).clamp(5, 12)
    }

    fn calculate_cursor_position(&self, input_area: Rect, prompt: &str) -> (u16, u16) {
        let available_width = input_area.width as usize;

        let mut cursor_row = input_area.y + 1;
        let mut cursor_col = input_area.x;

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
