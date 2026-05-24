# Tour 7: The Front Desk — TUI Shell & Rich Rendering

> *"This is the face of the building. Every visitor spends their time here. It must be fast, beautiful, and responsive."*

Welcome to the **Front Desk** — the ground floor east wing, where humans interact with the machine. This is the **TUI (Terminal User Interface)** — a real-time, keyboard-driven interface built with `ratatui` and `crossterm`.

In this tour, we'll explore:
1. The **event loop** — how keystrokes become actions
2. The **rendering engine** — how markdown becomes pixels
3. The **input system** — history, editor, clipboard

---

## 🖥️ The Main Hall: `ShellUI`

File: `octopus-cli/src/ui/shell/mod.rs` (~1,148 lines)

The `ShellUI` struct is the front desk's control panel:

```rust
pub struct ShellUI {
    soul: Option<KimiSoul>,              // The brain (moved here when idle)
    soul_arc: Option<Arc<Mutex<Option<KimiSoul>>>>, // The brain (moved here when running)
    input: String,                        // What the user is typing
    cursor_position: usize,               // Where the cursor is
    messages: Vec<(String, String)>,      // Chat history (role, content)
    mode: ShellMode,                      // Agent vs Shell
    state: AppState,                      // Idle or Running
    history: Vec<String>,                 // Persistent input history
    history_index: Option<usize>,         // Where we are in history
    // ... and more
}
```

### The Event Loop

```rust
async fn run_loop(&mut self, terminal: &mut Terminal<...>) -> io::Result<bool> {
    let mut reader = crossterm::event::EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(50));

    while !self.exit {
        terminal.draw(|f| self.draw(f))?;  // Render frame
        self.check_task_completion().await; // Is the soul done?

        tokio::select! {
            _ = tick.tick() => {},  // 50ms heartbeat
            Some(Ok(event)) = reader.next() => {
                if let Event::Key(key) = event {
                    self.handle_key_event(key).await;
                }
            }
            Ok(value) = wire_rx.recv() => {
                // Approval request from RootWireHub
                if let Ok(req) = serde_json::from_value::<ApprovalRequestEvent>(value) {
                    self.pending_approval = Some(req);
                }
            }
        }
    }
    Ok(true)
}
```

This is the **classic game loop pattern**, adapted for a TUI:
1. **Render** — draw the current state
2. **Update** — check if background tasks completed
3. **Input** — handle keyboard/mouse events
4. **Network** — listen for wire events (approvals)

🐍 **Python's way:** `prompt_toolkit`'s `Application` with custom key bindings and async event handlers.

🦀 **Rust's way:** Manual event loop with `tokio::select!`. We own every frame of the pipeline.

✨ **Where Rust shines:** **No framework lock-in.** `ratatui` is a rendering library, not a framework. We control the loop, the timing, and the event handling. In Python, `prompt_toolkit` provides a lot of magic that can be hard to debug or customize.

---

## ⌨️ The Typewriter: Input Handling

### Key Bindings

| Key | Action |
|-----|--------|
| `Enter` | Submit input |
| `Alt+Enter` | Insert newline (multiline) |
| `Ctrl+O` | Open external editor |
| `Ctrl+X` | Toggle agent/shell mode |
| `Ctrl+C` | Cancel running task / show tip |
| `Ctrl+D` | Exit (if input empty) |
| `Ctrl+V` | Paste from clipboard |
| `Ctrl+Y` | Copy last assistant message |
| `Up/Down` | History navigation |
| `Tab` | Slash command completion |

### History Navigation

```rust
fn history_navigate_up(&mut self) {
    if self.history.is_empty() { return; }
    if self.history_index.is_none() {
        self.history_draft = self.input.clone();  // Save draft!
        self.history_index = Some(self.history.len() - 1);
    } else if self.history_index.unwrap() > 0 {
        self.history_index = Some(self.history_index.unwrap() - 1);
    }
    self.input = self.history[self.history_index.unwrap()].clone();
    self.cursor_position = self.input.len();
}
```

Notice the **draft preservation**. When you press Up, your current unfinished input is saved. Press Down past the end of history, and your draft is restored. This is the **"I was typing something, let me check history, now I want my draft back"** pattern.

### External Editor

```rust
async fn run_external_editor(&mut self, terminal: &mut Terminal<...>) -> io::Result<()> {
    // 1. Leave alternate screen
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    // 2. Launch editor
    let status = tokio::process::Command::new(&editor)
        .arg(&temp_path)
        .status()
        .await?;

    // 3. Re-enter alternate screen
    crossterm::terminal::enable_raw_mode()?;
    // ... rebuild terminal ...

    // 4. Read back
    self.input = std::fs::read_to_string(&temp_path)?;
}
```

The TUI **gracefully suspends** itself to launch `$EDITOR`. This is like a window manager minimizing itself so you can use another app.

---

## 🎨 The Art Gallery: Rich Rendering

File: `octopus-cli/src/ui/shell/render.rs` (~300 lines)

The Art Gallery converts raw markdown into beautifully formatted terminal output.

### The Pipeline

```
Markdown string
    → pulldown-cmark (parse events)
        → render.rs (convert to ratatui Spans)
            → ratatui (render to terminal cells)
```

### Code Blocks with Syntax Highlighting

```rust
fn highlight_code_block(code: &str, lang: Option<&str>) -> Vec<Line<'static>> {
    let ss = syntax_set();  // Syntect syntax definitions
    let ts = theme_set();   // Syntect color themes
    let syntax = lang
        .and_then(|l| ss.find_syntax_by_token(l))
        .or_else(|| ss.find_syntax_by_first_line(code))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut highlighter = HighlightLines::new(syntax, &ts.themes["base16-ocean.dark"]);

    for line in LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(line, ss)?;
        let spans: Vec<Span> = ranges.into_iter()
            .map(|(style, text)| Span::styled(text.to_string(), syntect_to_ratatui_style(style)))
            .collect();
        lines.push(Line::from(spans));
    }
}
```

🐍 **Python's way:** `rich.syntax.Syntax` + `rich.markdown.Markdown` — high-level widgets that do everything.

🦀 **Rust's way:** `syntect` for highlighting, `pulldown-cmark` for parsing, manual conversion to `ratatui` primitives. More work, but **total control** over the output.

✨ **Where Rust shines:** **No hidden allocations.** `syntect`'s `HighlightLines` works on borrowed strings. `ratatui`'s `Span` uses `Cow<'static, str>` — borrowed when possible, owned only when necessary. The entire rendering pipeline minimizes heap allocation.

### Diff Blocks

```rust
fn render_diff_block(code: &str) -> Vec<Line<'static>> {
    for line in code.lines() {
        let style = if line.starts_with('+') {
            Style::default().fg(Color::Green)
        } else if line.starts_with('-') {
            Style::default().fg(Color::Red)
        } else if line.starts_with("@@") {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(Span::styled(line.to_string(), style)));
    }
}
```

Diff blocks are **specially colored**:
- `+` lines → green (additions)
- `-` lines → red (deletions)
- `@@` lines → cyan (hunk headers)
- Context → gray

This makes code reviews in the terminal **immediately scannable**.

---

## 🎭 The Stage: Drawing the Frame

```rust
fn draw(&mut self, frame: &mut Frame) {
    // 1. Build chat content
    let mut text_lines = Vec::new();
    for (role, content) in &self.messages {
        if role == "assistant" {
            // Rich markdown rendering
            let rendered = render::render_markdown(content);
            for line in rendered {
                // Prepend 🤖 prefix...
                text_lines.push(line);
            }
        } else {
            // Simple role-colored text
            for line in content.lines() {
                text_lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(line.to_string(), style),
                ]));
            }
        }
    }

    // 2. Layout: chat | input | completions | tips | spacer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([...])
        .split(frame.area());

    // 3. Render widgets
    frame.render_widget(Paragraph::new(Text::from(text_lines)), chat_area);
    frame.render_widget(input_block, input_area);
    // ...
}
```

The layout uses **ratatui's constraint system**:
- Chat area: variable height (capped at half terminal)
- Input area: fixed height (3-12 rows depending on content)
- Completions: variable (0-10 rows)
- Tips: variable (0-N rows)
- Spacer: absorbs remaining space

🐍 **Python's way:** `prompt_toolkit`'s `Layout` with `HSplit`, `VSplit`, and `Window` objects.

🦀 **Rust's way:** Manual `Layout::split()` with `Constraint` arrays. More explicit, but gives **pixel-perfect control**.

---

## 📋 The Clipboard: Copy & Paste

File: `octopus-cli/src/utils/clipboard.rs` (~30 lines)

The front desk has a clipboard for copying assistant responses:

```rust
pub fn copy_text(text: &str) -> Result<(), String> {
    with_clipboard(|cb| cb.set_text(text))
}

pub fn paste_text() -> Result<String, String> {
    with_clipboard(|cb| cb.get_text())
}
```

Using the `arboard` crate, this works on:
- **Linux:** X11 (primary + clipboard) and Wayland
- **macOS:** `NSPasteboard`
- **Windows:** `Clipboard` API

🐍 **Python's way:** `pyperclip` or `clipboard` module.

🦀 **Rust's way:** `arboard` — a single crate handles all platforms with no Python dependencies.

---

## 🎁 Souvenir Shop: What to Remember

1. **The TUI is a game loop.** Render → update → input → network, 20 times per second.
2. **Markdown rendering is three-stage.** `pulldown-cmark` → `render.rs` → `ratatui`. Each stage is swappable.
3. **History preserves drafts.** Up/Down navigation doesn't lose your unfinished thought.
4. **The external editor suspends the TUI.** Not a popup — a full terminal handoff.
5. **~1,148 lines for a full TUI.** Python's `shell/__init__.py` was ~1,540 lines, plus `prompt.py` (2,259 lines) and `keyboard.py` (300 lines). Rust achieves parity with less code because `ratatui` is thinner than `prompt_toolkit`.

---

## 🚶 Next Stop

The Front Desk handles the present. But what about the past? Let's visit the **Archives** — where conversations are preserved.

→ [Tour 8: The Archives](./08-archives.md)
