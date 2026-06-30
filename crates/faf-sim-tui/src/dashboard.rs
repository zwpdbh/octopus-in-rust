use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, ExecutableCommand};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table, Wrap};
use ratatui::{Frame, Terminal};

use faf_sim::planner::mcts::train::{EpisodeSummary, FineTuneSummary, GreedyEvalSummary};

use crate::observer::{DashboardEvent, DashboardObserver};

/// Maximum number of recent episodes kept for rolling statistics and sparklines.
const HISTORY_LEN: usize = 200;
/// Minimum terminal height we try to support.
const MIN_HEIGHT: u16 = 12;

/// Run a training closure with the terminal dashboard active.
///
/// The closure receives a [`TrainingObserver`] that forwards progress events to
/// the dashboard renderer. When the closure returns, the dashboard is torn down
/// and the result is returned to the caller.
pub struct TrainingDashboard;

impl TrainingDashboard {
    /// Run `training` while displaying the live training dashboard.
    pub fn run<F, R>(training: F) -> R
    where
        F: FnOnce(DashboardObserver) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (event_sender, event_receiver) = mpsc::channel::<DashboardEvent>();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let observer = DashboardObserver::new(event_sender, Arc::clone(&stop_flag));

        let (result_sender, result_receiver) = mpsc::channel::<R>();
        let training_handle = std::thread::spawn(move || {
            let result = training(observer);
            let _ = result_sender.send(result);
        });

        run_tui(event_receiver, stop_flag, training_handle);

        result_receiver.recv().expect("training thread result")
    }
}

type DashboardBackend = ratatui::backend::CrosstermBackend<io::Stdout>;

/// Restores the terminal state when dropped, even on panic.
struct TerminalGuard {
    terminal: Terminal<DashboardBackend>,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

/// Start the TUI renderer and block until training completes or the user
/// requests a stop.
fn run_tui(
    receiver: Receiver<DashboardEvent>,
    stop_flag: Arc<AtomicBool>,
    training_handle: JoinHandle<()>,
) {
    let mut stdout = io::stdout();
    let _ = stdout.execute(EnterAlternateScreen);
    let _ = enable_raw_mode();

    let terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout))
        .expect("failed to create terminal");
    let mut guard = TerminalGuard { terminal };

    let mut state = DashboardState::default();
    let mut last_render = Instant::now();
    let render_interval = Duration::from_millis(100);

    loop {
        // Drain all pending training events.
        while let Ok(event) = receiver.try_recv() {
            state.apply(event);
        }

        // Handle terminal input without blocking.
        while let Ok(true) = event::poll(Duration::from_secs(0)) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            stop_flag.store(true, Ordering::Relaxed);
                        }
                        _ => {}
                    }
                }
            }
        }

        if last_render.elapsed() >= render_interval {
            let _ = guard.terminal.draw(|frame| render(frame, &state));
            last_render = Instant::now();
        }

        if training_handle.is_finished() {
            // Drain any remaining events sent just before the thread finished.
            while let Ok(event) = receiver.try_recv() {
                state.apply(event);
            }
            let _ = guard.terminal.draw(|frame| render(frame, &state));
            break;
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Mutable dashboard state updated from training events.
#[derive(Default)]
struct DashboardState {
    start_time: Option<Instant>,
    current_episode: usize,
    total_episodes: usize,
    best_time: Option<f64>,
    current_epsilon: f32,
    recent_episodes: Vec<EpisodeSummary>,
    greedy_evals: Vec<GreedyEvalSummary>,
    fine_tune: Option<FineTuneSummary>,
}

impl DashboardState {
    fn apply(&mut self, event: DashboardEvent) {
        match event {
            DashboardEvent::Episode(summary) => {
                if self.start_time.is_none() {
                    self.start_time = Some(Instant::now());
                }
                self.current_episode = summary.episode;
                self.total_episodes = summary.total_episodes;
                self.current_epsilon = summary.epsilon;
                if summary.reached_goal {
                    self.best_time = match self.best_time {
                        Some(t) => Some(t.min(summary.completion_time)),
                        None => Some(summary.completion_time),
                    };
                }
                self.recent_episodes.push(summary);
                if self.recent_episodes.len() > HISTORY_LEN {
                    self.recent_episodes.remove(0);
                }
            }
            DashboardEvent::GreedyEval(summary) => {
                self.greedy_evals.push(summary);
                if self.greedy_evals.len() > 10 {
                    self.greedy_evals.remove(0);
                }
                if summary.reached_goal {
                    if let Some(time) = summary.completion_time {
                        self.best_time = match self.best_time {
                            Some(t) => Some(t.min(time)),
                            None => Some(time),
                        };
                    }
                }
            }
            DashboardEvent::FineTuneEpoch(summary) => {
                self.fine_tune = Some(summary);
            }
        }
    }

    fn elapsed(&self) -> Duration {
        self.start_time.map_or(Duration::ZERO, |t| t.elapsed())
    }

    fn recent_window(&self, n: usize) -> &[EpisodeSummary] {
        let start = self.recent_episodes.len().saturating_sub(n);
        &self.recent_episodes[start..]
    }

    fn goal_rate(&self, window: usize) -> f32 {
        let slice = self.recent_window(window);
        if slice.is_empty() {
            0.0
        } else {
            slice.iter().filter(|e| e.reached_goal).count() as f32 / slice.len() as f32
        }
    }

    fn avg_loss(&self, window: usize) -> Option<f32> {
        let slice: Vec<_> = self
            .recent_window(window)
            .iter()
            .filter_map(|e| e.loss)
            .collect();
        if slice.is_empty() {
            None
        } else {
            Some(slice.iter().sum::<f32>() / slice.len() as f32)
        }
    }

    fn avg_steps(&self, window: usize) -> f32 {
        let slice = self.recent_window(window);
        if slice.is_empty() {
            0.0
        } else {
            slice.iter().map(|e| e.steps as f32).sum::<f32>() / slice.len() as f32
        }
    }

    fn episodes_per_second(&self) -> f32 {
        let elapsed = self.elapsed().as_secs_f32();
        if elapsed > 0.0 {
            self.current_episode as f32 / elapsed
        } else {
            0.0
        }
    }

    fn eta_seconds(&self) -> Option<u64> {
        if self.total_episodes == 0 || self.current_episode == 0 {
            return None;
        }
        let eps_per_sec = self.episodes_per_second();
        if eps_per_sec <= 0.0 {
            return None;
        }
        let remaining = self.total_episodes.saturating_sub(self.current_episode) as f32;
        Some((remaining / eps_per_sec) as u64)
    }

    fn progress_ratio(&self) -> f64 {
        if self.total_episodes == 0 {
            0.0
        } else {
            (self.current_episode as f64 / self.total_episodes as f64).min(1.0)
        }
    }

    fn loss_sparkline(&self) -> Vec<u64> {
        self.recent_episodes
            .iter()
            .filter_map(|e| e.loss)
            .map(|l| (l * 1000.0) as u64)
            .collect()
    }

    fn time_sparkline(&self) -> Vec<u64> {
        self.recent_episodes
            .iter()
            .filter(|e| e.reached_goal)
            .map(|e| e.completion_time as u64)
            .collect()
    }
}

fn render(frame: &mut Frame, state: &DashboardState) {
    let area = frame.area();
    if area.height < MIN_HEIGHT {
        frame.render_widget(
            Paragraph::new("Terminal too small for dashboard")
                .style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title / status
            Constraint::Min(8),    // Body
            Constraint::Length(7), // Bottom: evals + controls
        ])
        .split(area);

    render_header(frame, state, main_layout[0]);
    render_body(frame, state, main_layout[1]);
    render_footer(frame, state, main_layout[2]);
}

fn render_header(frame: &mut Frame, state: &DashboardState, area: Rect) {
    let title = if state.fine_tune.is_some() {
        "faf-sim train — fine-tuning"
    } else {
        "faf-sim train — REINFORCE"
    };

    let elapsed = format_duration(state.elapsed().as_secs());
    let eta = state
        .eta_seconds()
        .map(format_duration)
        .unwrap_or_else(|| "---".to_string());

    let progress_text = if state.total_episodes == 0 {
        format!("episode {} | elapsed {}", state.current_episode, elapsed)
    } else {
        format!(
            "episode {}/{} | elapsed {} | ETA {}",
            state.current_episode, state.total_episodes, elapsed, eta
        )
    };

    let progress = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(title))
        .gauge_style(Style::default().fg(Color::Cyan))
        .ratio(state.progress_ratio())
        .label(progress_text);

    frame.render_widget(progress, area);
}

fn render_body(frame: &mut Frame, state: &DashboardState, area: Rect) {
    let body_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    render_metrics_table(frame, state, body_layout[0]);
    render_plots(frame, state, body_layout[1]);
}

fn render_metrics_table(frame: &mut Frame, state: &DashboardState, area: Rect) {
    let best = state
        .best_time
        .map(format_time)
        .unwrap_or_else(|| "---".to_string());
    let goal_rate = format!("{:.1}%", state.goal_rate(100) * 100.0);
    let avg_loss = state
        .avg_loss(100)
        .map(|l| format!("{:.4}", l))
        .unwrap_or_else(|| "---".to_string());
    let avg_steps = format!("{:.1}", state.avg_steps(100));
    let eps_per_sec = format!("{:.2}", state.episodes_per_second());

    let rows = [
        metric_row("Best time", best),
        metric_row("Epsilon", format!("{:.4}", state.current_epsilon)),
        metric_row("Goal rate (100ep)", goal_rate),
        metric_row("Avg loss (100ep)", avg_loss),
        metric_row("Avg steps (100ep)", avg_steps),
        metric_row("Episodes/sec", eps_per_sec),
    ];

    let table = Table::new(rows, [Constraint::Length(20), Constraint::Min(10)])
        .block(Block::default().borders(Borders::ALL).title("Metrics"));

    frame.render_widget(table, area);
}

fn metric_row(label: &str, value: String) -> Row<'_> {
    Row::new([
        Cell::from(Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(value, Style::default().fg(Color::White))),
    ])
}

fn render_plots(frame: &mut Frame, state: &DashboardState, area: Rect) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let loss_data = state.loss_sparkline();
    let loss_spark = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Loss (last 200 episodes)"),
        )
        .data(&loss_data)
        .style(Style::default().fg(Color::Magenta));
    frame.render_widget(loss_spark, layout[0]);

    let time_data = state.time_sparkline();
    let time_spark = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Completion time / s (successful episodes)"),
        )
        .data(&time_data)
        .style(Style::default().fg(Color::Green));
    frame.render_widget(time_spark, layout[1]);
}

fn render_footer(frame: &mut Frame, state: &DashboardState, area: Rect) {
    let footer_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let eval_text = if state.greedy_evals.is_empty() {
        "No greedy evaluations yet.".to_string()
    } else {
        state
            .greedy_evals
            .iter()
            .map(|e| {
                let time = if e.reached_goal {
                    format_time(e.completion_time.unwrap_or(0.0))
                } else {
                    "DNF".to_string()
                };
                format!("ep{} {}", e.episode, time)
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };

    let eval_paragraph = Paragraph::new(eval_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Greedy evaluations"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(eval_paragraph, footer_layout[0]);

    let controls = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "q",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::from("  request stop"),
        ]),
        Line::from("Training will stop at the"),
        Line::from("next episode boundary."),
    ])
    .block(Block::default().borders(Borders::ALL).title("Controls"))
    .alignment(Alignment::Left);
    frame.render_widget(controls, footer_layout[1]);
}

fn format_time(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor();
    let secs = seconds - minutes * 60.0;
    format!("{:.0}m {:.1}s", minutes, secs)
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
