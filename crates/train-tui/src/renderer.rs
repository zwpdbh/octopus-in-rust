//! Generic training TUI renderer for Burn.
//!
//! Mirrors Burn's built-in TUI but removes the Status panel and gives the
//! Metrics text panel more vertical space. The renderer runs on a dedicated
//! thread so input and redraws stay responsive even when the training thread
//! is CPU-bound.

use burn_train::metric::{MetricDefinition, MetricEntry, MetricId, NumericEntry};
use burn_train::renderer::{
    EvaluationName, EvaluationProgress, MetricState, MetricsRenderer, MetricsRendererEvaluation,
    MetricsRendererTraining, ProgressType, TrainingProgress,
};
use burn_train::Interrupter;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Dataset, Gauge, GraphType, Paragraph, Wrap};
use ratatui::Terminal;
use std::collections::HashMap;
use std::io;
use std::panic::{set_hook, take_hook};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + 'static + Sync + Send>;

/// TUI renderer for training metrics.
///
/// Implements Burn's [`MetricsRenderer`] trait. Use [`TrainTuiRenderer::new`]
/// to create an instance and pass it to the learner.
pub struct TrainTuiRenderer {
    sender: Sender<RenderMessage>,
    kill_signal: Mutex<Receiver<()>>,
}

enum RenderMessage {
    Register(MetricDefinition),
    Update(MetricState),
    Render(TrainingProgress, Vec<ProgressType>),
    End(Option<burn_train::LearnerSummary>),
    Close,
}

struct InnerRenderer {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    metric_definitions: HashMap<MetricId, MetricDefinition>,
    numeric: NumericState,
    text: TextState,
    progress: Option<TrainingProgress>,
    indicators: Vec<ProgressType>,
    selected_tab: usize,
    plot_kind: PlotKind,
    interrupter: Interrupter,
    kill_signal: Sender<()>,
    quit_pending: bool,
    dirty: bool,
    running: bool,
    previous_panic_hook: Option<Arc<PanicHook>>,
}

struct NumericState {
    data: HashMap<MetricId, MetricHistory>,
    order: Vec<MetricId>,
}

struct MetricHistory {
    name: String,
    points: Vec<(f64, f64)>,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    next_x: f64,
}

struct TextState {
    entries: HashMap<MetricId, TextEntry>,
    order: Vec<MetricId>,
}

struct TextEntry {
    name: String,
    formatted: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlotKind {
    Recent,
    Full,
}

const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const RECENT_POINTS: usize = 100;
const MAX_FULL_POINTS: usize = 1000;

impl TrainTuiRenderer {
    /// Create a new TUI renderer with the given interrupter.
    ///
    /// Spawns a dedicated render thread that handles input events and redraws
    /// independently of the training thread.
    pub fn new(interrupter: Interrupter) -> Self {
        let (sender, receiver) = channel();
        let (kill_sender, kill_receiver) = channel();
        std::thread::spawn(move || render_thread(receiver, kill_sender, interrupter));
        Self {
            sender,
            kill_signal: Mutex::new(kill_receiver),
        }
    }

    fn send(&self, msg: RenderMessage) {
        // If the user requested a hard kill from the render thread, panic in the
        // training thread to match Burn's built-in TUI behavior.
        if self.kill_signal.lock().unwrap().try_recv().is_ok() {
            panic!("Killing training from user input.");
        }
        // Ignore send errors — the render thread may have shut down.
        let _ = self.sender.send(msg);
    }
}

impl Drop for TrainTuiRenderer {
    fn drop(&mut self) {
        self.send(RenderMessage::Close);
    }
}

impl MetricsRendererTraining for TrainTuiRenderer {
    fn update_train(&mut self, state: MetricState) {
        self.send(RenderMessage::Update(state));
    }

    fn update_valid(&mut self, _state: MetricState) {}

    fn render_train(&mut self, item: TrainingProgress, progress_indicators: Vec<ProgressType>) {
        self.send(RenderMessage::Render(item, progress_indicators));
    }

    fn render_valid(&mut self, _item: TrainingProgress, _progress_indicators: Vec<ProgressType>) {}

    fn on_train_end(
        &mut self,
        summary: Option<burn_train::LearnerSummary>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.send(RenderMessage::End(summary));
        Ok(())
    }
}

impl MetricsRendererEvaluation for TrainTuiRenderer {
    fn update_test(&mut self, _name: EvaluationName, _state: MetricState) {}

    fn render_test(&mut self, _item: EvaluationProgress, _progress_indicators: Vec<ProgressType>) {}

    fn on_test_end(
        &mut self,
        _summary: Option<burn_train::LearnerSummary>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

impl MetricsRenderer for TrainTuiRenderer {
    fn manual_close(&mut self) {}

    fn register_metric(&mut self, definition: MetricDefinition) {
        self.send(RenderMessage::Register(definition));
    }
}

fn render_thread(
    receiver: std::sync::mpsc::Receiver<RenderMessage>,
    kill_signal: Sender<()>,
    interrupter: Interrupter,
) {
    let mut inner = InnerRenderer::new(kill_signal, interrupter);
    let mut last_draw = Instant::now();

    while inner.running {
        // Drain all pending metric/training messages.
        while let Ok(msg) = receiver.try_recv() {
            match msg {
                RenderMessage::Register(def) => inner.register(def),
                RenderMessage::Update(state) => inner.update(state),
                RenderMessage::Render(progress, indicators) => {
                    inner.progress = Some(progress);
                    inner.indicators = indicators;
                    inner.dirty = true;
                }
                RenderMessage::End(summary) => {
                    inner.on_end(summary);
                    inner.running = false;
                }
                RenderMessage::Close => {
                    inner.running = false;
                }
            }
        }

        inner.handle_events();

        // Redraw at roughly 60 FPS or immediately when state changed.
        if inner.dirty || last_draw.elapsed() >= FRAME_INTERVAL {
            let _ = inner.draw();
            inner.dirty = false;
            last_draw = Instant::now();
        }

        std::thread::sleep(FRAME_INTERVAL);
    }

    // Final draw and cleanup happen when `inner` is dropped.
    let _ = inner.draw();
}

impl InnerRenderer {
    fn new(kill_signal: Sender<()>, interrupter: Interrupter) -> Self {
        let mut stdout = io::stdout();
        enable_raw_mode().ok();
        let _ = crossterm::execute!(stdout, EnterAlternateScreen);

        let terminal = Terminal::new(CrosstermBackend::new(stdout)).expect("create terminal");

        // Reset the terminal on panic so the panic message is visible.
        let previous_panic_hook = Arc::new(take_hook());
        set_hook({
            let previous = previous_panic_hook.clone();
            Box::new(move |info| {
                let _ = disable_raw_mode();
                let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
                previous(info);
            })
        });

        Self {
            terminal,
            metric_definitions: HashMap::new(),
            numeric: NumericState {
                data: HashMap::new(),
                order: Vec::new(),
            },
            text: TextState {
                entries: HashMap::new(),
                order: Vec::new(),
            },
            progress: None,
            indicators: Vec::new(),
            selected_tab: 0,
            plot_kind: PlotKind::Recent,
            interrupter,
            kill_signal,
            quit_pending: false,
            dirty: true,
            running: true,
            previous_panic_hook: Some(previous_panic_hook),
        }
    }

    fn register(&mut self, definition: MetricDefinition) {
        let id = definition.metric_id.clone();
        let name = definition.name.clone();
        self.metric_definitions.insert(id.clone(), definition);
        self.numeric
            .data
            .entry(id.clone())
            .or_insert_with(|| MetricHistory::new(name.clone()));
        if !self.numeric.order.contains(&id) {
            self.numeric.order.push(id.clone());
        }
        self.text.entries.entry(id).or_insert_with(|| TextEntry {
            name,
            formatted: String::new(),
        });
        self.dirty = true;
    }

    fn update(&mut self, state: MetricState) {
        match state {
            MetricState::Numeric(entry, value) => {
                self.update_text(&entry);
                self.update_numeric(entry.metric_id, value);
            }
            MetricState::Generic(entry) => {
                self.update_text(&entry);
            }
        }
        self.dirty = true;
    }

    fn update_text(&mut self, entry: &MetricEntry) {
        let id = &entry.metric_id;
        if let Some(def) = self.metric_definitions.get(id) {
            let name = def.name.clone();
            let formatted = entry.serialized_entry.formatted.clone();
            let e = self.text.entries.entry(id.clone()).or_insert(TextEntry {
                name,
                formatted: String::new(),
            });
            e.formatted = formatted;
            if !self.text.order.contains(id) {
                self.text.order.push(id.clone());
            }
        }
    }

    fn update_numeric(&mut self, id: MetricId, value: NumericEntry) {
        let y = match value {
            NumericEntry::Value(v) => v,
            NumericEntry::Aggregated {
                aggregated_value, ..
            } => aggregated_value,
        };
        if y.is_nan() || y.is_infinite() {
            return;
        }
        if let Some(history) = self.numeric.data.get_mut(&id) {
            let x = history.next_x;
            history.next_x += 1.0;
            history.points.push((x, y));
            if x > history.max_x {
                history.max_x = x;
            }
            if x < history.min_x {
                history.min_x = x;
            }
            if y > history.max_y {
                history.max_y = y;
            }
            if y < history.min_y {
                history.min_y = y;
            }
            if history.points.len() > MAX_FULL_POINTS {
                history.downsample();
            }
        }
    }

    fn on_end(&mut self, _summary: Option<burn_train::LearnerSummary>) {
        self.dirty = true;
    }

    fn reset(&mut self) {
        if let Some(previous) = self.previous_panic_hook.take() {
            let _ = disable_raw_mode();
            let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
            let _ = take_hook();
            if let Some(previous) = Arc::into_inner(previous) {
                set_hook(previous);
            }
        }
    }

    fn handle_events(&mut self) {
        while event::poll(Duration::from_secs(0)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if self.quit_pending {
                    match key.code {
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            self.interrupter
                                .stop(Some("Stopping training from user input."));
                            self.quit_pending = false;
                            self.dirty = true;
                        }
                        KeyCode::Char('k') | KeyCode::Char('K') => {
                            // Signal the training thread to panic on its next
                            // metric send, then panic here to tear down the
                            // renderer immediately.
                            let _ = self.kill_signal.send(());
                            panic!("Killing training from user input.");
                        }
                        KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
                            self.quit_pending = false;
                            self.dirty = true;
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') => {
                            self.quit_pending = true;
                            self.dirty = true;
                        }
                        KeyCode::Left => {
                            if self.selected_tab > 0 {
                                self.selected_tab -= 1;
                                self.dirty = true;
                            }
                        }
                        KeyCode::Right => {
                            let max = self.numeric.order.len().saturating_sub(1);
                            if self.selected_tab < max {
                                self.selected_tab += 1;
                                self.dirty = true;
                            }
                        }
                        KeyCode::Up | KeyCode::Down => {
                            self.plot_kind = match self.plot_kind {
                                PlotKind::Recent => PlotKind::Full,
                                PlotKind::Full => PlotKind::Recent,
                            };
                            self.dirty = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn selected_metric_name(&self) -> Option<String> {
        self.numeric
            .order
            .get(self.selected_tab)
            .and_then(|id| self.numeric.data.get(id).map(|h| h.name.clone()))
    }

    fn selected_metric(&self) -> Option<MetricId> {
        self.numeric.order.get(self.selected_tab).cloned()
    }

    fn draw(&mut self) -> io::Result<()> {
        let controls = controls_widget(self.quit_pending);
        let metrics = metrics_widget(&self.text);
        let progress = progress_widget(self.progress.as_ref());
        let tab_line = plot_tab_line(&self.numeric, self.selected_tab);
        let selected_name = self.selected_metric_name().unwrap_or_default();
        let plot_kind = self.plot_kind;
        let chart = self.selected_metric().and_then(|id| {
            self.numeric.data.get(&id).map(|history| {
                let points = match plot_kind {
                    PlotKind::Recent => history.recent_points(),
                    PlotKind::Full => history.points.clone(),
                };
                let title = match plot_kind {
                    PlotKind::Recent => "Recent History",
                    PlotKind::Full => "Full History",
                };
                let bounds = history.bounds(&points);
                (history.name.clone(), points, title.to_string(), bounds)
            })
        });

        self.terminal.draw(|frame| {
            let size = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(8), Constraint::Length(3)])
                .split(size);
            let top = chunks[0];
            let progress_area = chunks[1];

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(top);
            let left = chunks[0];
            let right = chunks[1];

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(6), Constraint::Min(6)])
                .split(left);
            let controls_area = chunks[0];
            let metrics_area = chunks[1];

            frame.render_widget(controls, controls_area);
            frame.render_widget(metrics, metrics_area);

            let inner = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(5)])
                .split(right);
            let tab_area = inner[0];
            let chart_area = inner[1];

            frame.render_widget(tab_line, tab_area);

            if let Some((name, points, title, (x_bounds, y_bounds))) = chart {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Plots — {} ({})", name, title));
                if points.is_empty() {
                    frame.render_widget(
                        Paragraph::new("No numeric data yet for this metric.").block(block),
                        chart_area,
                    );
                } else if points.len() == 1 {
                    let (_, y) = points[0];
                    frame.render_widget(
                        Paragraph::new(format!(
                            "1 data point ({:.2}). At least 2 are needed to draw a line.",
                            y
                        ))
                        .block(block),
                        chart_area,
                    );
                } else {
                    let dataset = Dataset::default()
                        .marker(symbols::Marker::Braille)
                        .graph_type(GraphType::Line)
                        .style(Style::default().fg(Color::LightRed))
                        .data(&points);
                    let chart_widget =
                        ratatui::widgets::Chart::new(vec![dataset])
                            .block(block)
                            .x_axis(ratatui::widgets::Axis::default().bounds(x_bounds).labels(
                                vec![Line::from("0"), Line::from(format!("{:.0}", x_bounds[1]))],
                            ))
                            .y_axis(ratatui::widgets::Axis::default().bounds(y_bounds).labels(
                                vec![
                                    Line::from(format!("{:.2}", y_bounds[0])),
                                    Line::from(format!("{:.2}", y_bounds[1])),
                                ],
                            ));
                    frame.render_widget(chart_widget, chart_area);
                }
            } else {
                let block = Block::default().borders(Borders::ALL).title(format!(
                    "Plots — {} ({})",
                    selected_name,
                    match plot_kind {
                        PlotKind::Recent => "Recent History",
                        PlotKind::Full => "Full History",
                    }
                ));
                frame.render_widget(
                    Paragraph::new("No data for metric.").block(block),
                    chart_area,
                );
            }

            frame.render_widget(progress, progress_area);
        })?;
        Ok(())
    }
}

impl Drop for InnerRenderer {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.reset();
        }
    }
}

fn controls_widget(quit_pending: bool) -> Paragraph<'static> {
    let lines = if quit_pending {
        vec![
            Line::from(vec![Span::styled(
                "Quit options",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("Stop", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" : s  "),
                Span::styled(
                    "Stop the training. Breaks from the training loop at the next checkpoint.",
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(vec![
                Span::styled("Kill", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" : k  "),
                Span::styled(
                    "Stop the training immediately. Panics the training process.",
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(vec![
                Span::styled("Cancel", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" : c / esc  "),
                Span::styled(
                    "Cancel the action, continue the training.",
                    Style::default().fg(Color::Yellow),
                ),
            ]),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled("Quit", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" : q  "),
                Span::styled("Stop the training.", Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled(
                    "Switch Metrics",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" : ← →  "),
                Span::styled(
                    "Switch between metrics.",
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "Switch Plot Type",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" : ↑ ↓  "),
                Span::styled(
                    "Switch between recent and full history.",
                    Style::default().fg(Color::Yellow),
                ),
            ]),
        ]
    };
    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Controls"))
        .alignment(Alignment::Left)
}

fn metrics_widget(state: &TextState) -> Paragraph<'static> {
    let mut lines = Vec::new();
    for id in &state.order {
        if let Some(entry) = state.entries.get(id) {
            lines.push(Line::from(vec![Span::styled(
                entry.name.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(vec![Span::styled(
                entry.formatted.clone(),
                Style::default().add_modifier(Modifier::ITALIC),
            )]));
            lines.push(Line::from(""));
        }
    }
    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Metrics"))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
}

fn progress_widget(progress: Option<&TrainingProgress>) -> Gauge<'static> {
    let (label, ratio) = if let Some(progress) = progress {
        let total = progress.global_progress.items_total.max(1);
        let current = progress.global_progress.items_processed.min(total);
        let ratio = current as f64 / total as f64;
        (format!("{} / {}", current, total), ratio)
    } else {
        ("Training".to_string(), 0.0)
    };
    Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Progress"))
        .gauge_style(Style::default().fg(Color::Red))
        .ratio(ratio)
        .label(label)
}

fn plot_tab_line(state: &NumericState, selected_tab: usize) -> Paragraph<'static> {
    let tabs: Vec<&str> = state
        .order
        .iter()
        .filter_map(|id| state.data.get(id).map(|h| h.name.as_str()))
        .collect();
    let selected = selected_tab.min(tabs.len().saturating_sub(1));
    let spans: Vec<Span> = tabs
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if i == selected {
                Span::styled(
                    format!(" {} ", name),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw(format!(" {} ", name))
            }
        })
        .collect();
    Paragraph::new(Line::from(spans))
}

impl MetricHistory {
    fn new(name: String) -> Self {
        Self {
            name,
            points: Vec::new(),
            min_x: 0.0,
            max_x: 0.0,
            min_y: f64::MAX,
            max_y: f64::MIN,
            next_x: 0.0,
        }
    }

    fn recent_points(&self) -> Vec<(f64, f64)> {
        self.points
            .iter()
            .rev()
            .take(RECENT_POINTS)
            .copied()
            .rev()
            .collect()
    }

    fn bounds(&self, points: &[(f64, f64)]) -> ([f64; 2], [f64; 2]) {
        if points.is_empty() {
            return ([0.0, 1.0], [0.0, 1.0]);
        }
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for (x, y) in points {
            min_x = min_x.min(*x);
            max_x = max_x.max(*x);
            min_y = min_y.min(*y);
            max_y = max_y.max(*y);
        }
        if min_y == max_y {
            min_y -= 1.0;
            max_y += 1.0;
        }
        if min_x == max_x {
            min_x -= 1.0;
            max_x += 1.0;
        }
        ([min_x, max_x], [min_y, max_y])
    }

    fn downsample(&mut self) {
        let mut new_points = Vec::with_capacity(self.points.len() / 2);
        let mut new_min_x = f64::MAX;
        let mut new_max_x = f64::MIN;
        let mut new_min_y = f64::MAX;
        let mut new_max_y = f64::MIN;
        for (i, (x, y)) in self.points.drain(..).enumerate() {
            if i % 2 == 0 {
                new_min_x = new_min_x.min(x);
                new_max_x = new_max_x.max(x);
                new_min_y = new_min_y.min(y);
                new_max_y = new_max_y.max(y);
                new_points.push((x, y));
            }
        }
        self.points = new_points;
        self.min_x = new_min_x;
        self.max_x = new_max_x;
        self.min_y = new_min_y;
        self.max_y = new_max_y;
    }
}
