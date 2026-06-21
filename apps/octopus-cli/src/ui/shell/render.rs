use std::sync::OnceLock;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

fn syntax_set() -> &'static SyntaxSet {
    static SS: OnceLock<SyntaxSet> = OnceLock::new();
    SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static TS: OnceLock<ThemeSet> = OnceLock::new();
    TS.get_or_init(ThemeSet::load_defaults)
}

fn syntect_to_ratatui_style(style: syntect::highlighting::Style) -> Style {
    let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    let mut ratatui_style = Style::default().fg(fg);
    if style.font_style.contains(FontStyle::BOLD) {
        ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
    }
    ratatui_style
}

fn highlight_code_block(code: &str, lang: Option<&str>) -> Vec<Line<'static>> {
    let ss = syntax_set();
    let ts = theme_set();
    let theme = &ts.themes["base16-ocean.dark"];

    let syntax = lang
        .and_then(|l| ss.find_syntax_by_token(l))
        .or_else(|| ss.find_syntax_by_first_line(code))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for line in LinesWithEndings::from(code) {
        let line_stripped = line.strip_suffix('\n').unwrap_or(line);
        if line_stripped.is_empty() {
            lines.push(Line::from(""));
            continue;
        }
        match highlighter.highlight_line(line_stripped, ss) {
            Ok(ranges) => {
                let spans: Vec<Span<'static>> = ranges
                    .into_iter()
                    .map(|(style, text)| {
                        Span::styled(text.to_string(), syntect_to_ratatui_style(style))
                    })
                    .collect();
                lines.push(Line::from(spans));
            }
            Err(_) => {
                lines.push(Line::from(line_stripped.to_string()));
            }
        }
    }

    lines
}

fn render_diff_block(code: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
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
    lines
}

/// Convert markdown text to ratatui `Line`s with rich formatting.
pub fn render_markdown(input: &str) -> Vec<Line<'static>> {
    let parser = Parser::new(input);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut in_code_block: Option<Option<String>> = None;
    let mut code_buffer = String::new();
    let mut in_diff = false;

    // Track inline formatting state
    let mut bold_depth = 0usize;
    let mut italic_depth = 0usize;
    let mut link_url: Option<String> = None;

    macro_rules! flush_spans {
        () => {
            if !current_spans.is_empty() {
                lines.push(Line::from(std::mem::take(&mut current_spans)));
            }
        };
    }

    macro_rules! current_style {
        () => {{
            let mut style = Style::default();
            if bold_depth > 0 {
                style = style.add_modifier(Modifier::BOLD);
            }
            if italic_depth > 0 {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if link_url.is_some() {
                style = style.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
            }
            style
        }};
    }

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::CodeBlock(lang) => {
                    flush_spans!();
                    let lang_str = match lang {
                        pulldown_cmark::CodeBlockKind::Fenced(l) => {
                            let s = l.to_string();
                            if s.is_empty() { None } else { Some(s) }
                        }
                        pulldown_cmark::CodeBlockKind::Indented => None,
                    };
                    in_diff =
                        lang_str.as_deref() == Some("diff") || lang_str.as_deref() == Some("patch");
                    in_code_block = Some(lang_str);
                    code_buffer.clear();
                }
                Tag::Emphasis => italic_depth += 1,
                Tag::Strong => bold_depth += 1,
                Tag::Link { dest_url, .. } => {
                    link_url = Some(dest_url.to_string());
                }
                Tag::Heading { level, .. } => {
                    flush_spans!();
                    let prefix = match level {
                        pulldown_cmark::HeadingLevel::H1 => "# ",
                        pulldown_cmark::HeadingLevel::H2 => "## ",
                        pulldown_cmark::HeadingLevel::H3 => "### ",
                        pulldown_cmark::HeadingLevel::H4 => "#### ",
                        pulldown_cmark::HeadingLevel::H5 => "##### ",
                        pulldown_cmark::HeadingLevel::H6 => "###### ",
                    };
                    current_spans.push(Span::styled(
                        prefix.to_string(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ));
                    bold_depth += 1;
                }
                Tag::BlockQuote(_) => {
                    flush_spans!();
                    current_spans.push(Span::styled(
                        "│ ".to_string(),
                        Style::default().fg(Color::Gray),
                    ));
                }
                Tag::List(start) => {
                    flush_spans!();
                    if let Some(n) = start {
                        current_spans.push(Span::styled(
                            format!("{}.", n),
                            Style::default().fg(Color::Yellow),
                        ));
                    } else {
                        current_spans.push(Span::styled(
                            "• ".to_string(),
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                }
                Tag::Item => {
                    // Item markers are handled by List start
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::CodeBlock => {
                    if let Some(lang) = in_code_block.take() {
                        flush_spans!();
                        if in_diff {
                            lines.extend(render_diff_block(&code_buffer));
                        } else {
                            lines.extend(highlight_code_block(&code_buffer, lang.as_deref()));
                        }
                        in_diff = false;
                        code_buffer.clear();
                    }
                }
                TagEnd::Emphasis => italic_depth = italic_depth.saturating_sub(1),
                TagEnd::Strong => bold_depth = bold_depth.saturating_sub(1),
                TagEnd::Link => {
                    link_url = None;
                }
                TagEnd::Heading(..) => {
                    bold_depth = bold_depth.saturating_sub(1);
                    flush_spans!();
                }
                TagEnd::BlockQuote(_) => {
                    flush_spans!();
                }
                TagEnd::List(_) => {
                    flush_spans!();
                }
                TagEnd::Item => {
                    flush_spans!();
                }
                TagEnd::Paragraph => {
                    flush_spans!();
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block.is_some() {
                    code_buffer.push_str(&text);
                } else {
                    // Split text on newlines and flush each line
                    for (i, part) in text.split('\n').enumerate() {
                        if i > 0 {
                            flush_spans!();
                        }
                        if !part.is_empty() {
                            current_spans.push(Span::styled(part.to_string(), current_style!()));
                        }
                    }
                }
            }
            Event::Code(code) => {
                current_spans.push(Span::styled(
                    code.to_string(),
                    Style::default().fg(Color::Yellow).bg(Color::Black),
                ));
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                current_spans.push(Span::styled(
                    html.to_string(),
                    Style::default().fg(Color::Magenta),
                ));
            }
            Event::HardBreak | Event::SoftBreak => {
                flush_spans!();
            }
            Event::Rule => {
                flush_spans!();
                lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            _ => {}
        }
    }

    flush_spans!();
    lines
}
