use dioxus::prelude::*;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};

/// Render markdown text as Dioxus elements styled with Tailwind classes.
#[component]
pub fn Markdown(text: String) -> Element {
    let nodes = use_memo(move || parse_markdown(&text));
    rsx! {
        div { class: "text-neutral-200 break-words leading-relaxed",
            {render_nodes(&nodes.read())}
        }
    }
}

#[derive(Clone, PartialEq)]
enum MdNode {
    Text(String),
    Strong(Vec<MdNode>),
    Emphasis(Vec<MdNode>),
    Strikethrough(Vec<MdNode>),
    Code(String),
    Link {
        url: String,
        title: String,
        children: Vec<MdNode>,
    },
    Image {
        url: String,
        title: String,
        alt: String,
    },
    Paragraph(Vec<MdNode>),
    Heading(u8, Vec<MdNode>),
    List(bool, Vec<MdNode>),
    Item(Vec<MdNode>),
    TaskListItem(bool, Vec<MdNode>),
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    BlockQuote(Vec<MdNode>),
    ThematicBreak,
    Table(Vec<MdNode>),
    TableHead(Vec<MdNode>),
    TableRow(Vec<MdNode>),
    TableCell(Vec<MdNode>),
    SoftBreak,
    HardBreak,
    Html(String),
}

fn parse_markdown(text: &str) -> Vec<MdNode> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(text, options);

    let mut root: Vec<MdNode> = Vec::new();
    let mut stack: Vec<(Tag, Vec<MdNode>, Option<bool>)> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => {
                stack.push((tag, Vec::new(), None));
            }
            Event::End(_) => {
                let (tag, children, task_checked) = stack.pop().expect("unbalanced markdown tags");
                let node = if let Some(checked) = task_checked {
                    MdNode::TaskListItem(checked, children)
                } else {
                    tag_to_node(tag, children)
                };
                push_node(&mut stack, &mut root, node);
            }
            Event::Text(text) => {
                push_node(&mut stack, &mut root, MdNode::Text(text.into_string()));
            }
            Event::Code(code) => {
                push_node(&mut stack, &mut root, MdNode::Code(code.into_string()));
            }
            Event::Html(html) => {
                push_node(&mut stack, &mut root, MdNode::Html(html.into_string()));
            }
            Event::InlineHtml(html) => {
                push_node(&mut stack, &mut root, MdNode::Html(html.into_string()));
            }
            Event::SoftBreak => {
                push_node(&mut stack, &mut root, MdNode::SoftBreak);
            }
            Event::HardBreak => {
                push_node(&mut stack, &mut root, MdNode::HardBreak);
            }
            Event::Rule => {
                push_node(&mut stack, &mut root, MdNode::ThematicBreak);
            }
            Event::InlineMath(math) => {
                push_node(&mut stack, &mut root, MdNode::Code(math.into_string()));
            }
            Event::DisplayMath(math) => {
                push_node(&mut stack, &mut root, MdNode::Code(math.into_string()));
            }
            Event::FootnoteReference(text) => {
                push_node(&mut stack, &mut root, MdNode::Text(text.into_string()));
            }
            Event::TaskListMarker(checked) => {
                if let Some((_, _, task_checked)) = stack.last_mut() {
                    *task_checked = Some(checked);
                }
            }
        }
    }

    root
}

fn push_node(
    stack: &mut Vec<(Tag, Vec<MdNode>, Option<bool>)>,
    root: &mut Vec<MdNode>,
    node: MdNode,
) {
    if let Some((_, children, _)) = stack.last_mut() {
        children.push(node);
    } else {
        root.push(node);
    }
}

fn tag_to_node(tag: Tag, children: Vec<MdNode>) -> MdNode {
    match tag {
        Tag::Paragraph => MdNode::Paragraph(children),
        Tag::Heading {
            level,
            id: _,
            classes: _,
            attrs: _,
        } => MdNode::Heading(level as u8, children),
        Tag::List(start_num) => MdNode::List(start_num.is_some(), children),
        Tag::Item => MdNode::Item(children),
        Tag::Emphasis => MdNode::Emphasis(children),
        Tag::Strong => MdNode::Strong(children),
        Tag::Strikethrough => MdNode::Strikethrough(children),
        Tag::Link {
            dest_url, title, ..
        } => MdNode::Link {
            url: dest_url.into_string(),
            title: title.into_string(),
            children,
        },
        Tag::Image {
            dest_url, title, ..
        } => MdNode::Image {
            url: dest_url.into_string(),
            title: title.into_string(),
            alt: nodes_to_text(&children),
        },
        Tag::CodeBlock(lang) => {
            let lang = match lang {
                CodeBlockKind::Fenced(lang) => {
                    let s = lang.into_string();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                }
                CodeBlockKind::Indented => None,
            };
            let code = nodes_to_text(&children);
            MdNode::CodeBlock { lang, code }
        }
        Tag::BlockQuote(_) => MdNode::BlockQuote(children),
        Tag::Table(_) => MdNode::Table(children),
        Tag::TableHead => MdNode::TableHead(children),
        Tag::TableRow => MdNode::TableRow(children),
        Tag::TableCell => MdNode::TableCell(children),
        _ => MdNode::Html(format!("<{:?}>", tag)),
    }
}

fn nodes_to_text(nodes: &[MdNode]) -> String {
    let mut text = String::new();
    for node in nodes {
        match node {
            MdNode::Text(s) | MdNode::Code(s) | MdNode::Html(s) => text.push_str(s),
            MdNode::SoftBreak => text.push(' '),
            MdNode::HardBreak => text.push('\n'),
            MdNode::Strong(children)
            | MdNode::Emphasis(children)
            | MdNode::Strikethrough(children)
            | MdNode::Paragraph(children)
            | MdNode::Heading(_, children)
            | MdNode::List(_, children)
            | MdNode::Item(children)
            | MdNode::TaskListItem(_, children)
            | MdNode::BlockQuote(children)
            | MdNode::Table(children)
            | MdNode::TableHead(children)
            | MdNode::TableRow(children)
            | MdNode::TableCell(children)
            | MdNode::Link { children, .. } => text.push_str(&nodes_to_text(children)),
            MdNode::Image { alt, .. } => text.push_str(alt),
            MdNode::CodeBlock { code, .. } => text.push_str(code),
            MdNode::ThematicBreak => {}
        }
    }
    text
}

fn render_nodes(nodes: &[MdNode]) -> Element {
    rsx! {
        for node in nodes.iter() {
            {render_node(node)}
        }
    }
}

fn render_node(node: &MdNode) -> Element {
    match node {
        MdNode::Text(text) => rsx! { "{text}" },
        MdNode::Strong(children) => rsx! {
            strong { class: "font-semibold text-neutral-100", {render_nodes(children)} }
        },
        MdNode::Emphasis(children) => rsx! {
            em { class: "italic text-neutral-200", {render_nodes(children)} }
        },
        MdNode::Strikethrough(children) => rsx! {
            s { class: "line-through text-neutral-400", {render_nodes(children)} }
        },
        MdNode::Code(code) => rsx! {
            code { class: "bg-neutral-800 px-[0.35em] py-[0.15em] rounded text-[0.875em] text-neutral-200",
                "{code}"
            }
        },
        MdNode::Link { url, children, .. } => rsx! {
            a {
                class: "text-blue-400 underline hover:text-blue-300",
                href: "{url}",
                target: "_blank",
                {render_nodes(children)}
            }
        },
        MdNode::Image { url, alt, .. } => rsx! {
            img { class: "max-w-full rounded my-2", src: "{url}", alt: "{alt}" }
        },
        MdNode::Paragraph(children) => rsx! {
            p { class: "my-[0.5em]", {render_nodes(children)} }
        },
        MdNode::Heading(level, children) => {
            let class = match level {
                1 => "text-xl font-semibold text-neutral-100 mt-4 mb-2",
                2 => "text-lg font-semibold text-neutral-100 mt-3 mb-1.5",
                3 => "text-base font-semibold text-neutral-100 mt-3 mb-1",
                _ => "text-sm font-semibold text-neutral-100 mt-2 mb-1",
            };
            match level {
                1 => rsx! { h1 { class: "{class}", {render_nodes(children)} } },
                2 => rsx! { h2 { class: "{class}", {render_nodes(children)} } },
                3 => rsx! { h3 { class: "{class}", {render_nodes(children)} } },
                4 => rsx! { h4 { class: "{class}", {render_nodes(children)} } },
                5 => rsx! { h5 { class: "{class}", {render_nodes(children)} } },
                _ => rsx! { h6 { class: "{class}", {render_nodes(children)} } },
            }
        }
        MdNode::List(ordered, children) => {
            if *ordered {
                rsx! { ol { class: "list-decimal pl-5 my-[0.5em]", {render_nodes(children)} } }
            } else {
                rsx! { ul { class: "list-disc pl-5 my-[0.5em]", {render_nodes(children)} } }
            }
        }
        MdNode::Item(children) => rsx! {
            li { class: "my-[0.25em]", {render_nodes(children)} }
        },
        MdNode::TaskListItem(checked, children) => rsx! {
            li { class: "my-[0.25em] flex items-start gap-2",
                input {
                    r#type: "checkbox",
                    checked: *checked,
                    disabled: true,
                    class: "mt-1"
                }
                {render_nodes(children)}
            }
        },
        MdNode::CodeBlock { lang, code } => rsx! {
            div { class: "relative my-[0.5em]",
                if let Some(lang) = lang {
                    div { class: "absolute top-2 right-2 text-xs text-neutral-500 font-mono",
                        "{lang}"
                    }
                }
                pre { class: "bg-neutral-950 border border-neutral-800 rounded-lg p-3 overflow-x-auto",
                    code { class: "text-xs text-neutral-300 font-mono", "{code}" }
                }
            }
        },
        MdNode::BlockQuote(children) => rsx! {
            blockquote { class: "border-l-4 border-neutral-600 pl-3 my-[0.5em] text-neutral-400",
                {render_nodes(children)}
            }
        },
        MdNode::ThematicBreak => rsx! { hr { class: "border-neutral-700 my-4" } },
        MdNode::Table(children) => {
            let mut head: Option<&Vec<MdNode>> = None;
            let mut body_rows: Vec<&MdNode> = Vec::new();
            for child in children.iter() {
                match child {
                    MdNode::TableHead(h) => head = Some(h),
                    _ => body_rows.push(child),
                }
            }
            rsx! {
                table { class: "w-full border-collapse my-[0.5em] text-sm",
                    if let Some(head) = head {
                        thead { class: "bg-neutral-800",
                            {render_nodes(head)}
                        }
                    }
                    tbody { class: "divide-y divide-neutral-800",
                        for row in body_rows { {render_node(row)} }
                    }
                }
            }
        }
        MdNode::TableHead(children) => rsx! {
            tr { class: "border-b border-neutral-700",
                {render_nodes(children)}
            }
        },
        MdNode::TableRow(children) => rsx! {
            tr { class: "border-b border-neutral-800 last:border-b-0",
                {render_nodes(children)}
            }
        },
        MdNode::TableCell(children) => rsx! {
            td { class: "border border-neutral-700 px-3 py-2 text-left text-neutral-200",
                {render_nodes(children)}
            }
        },
        MdNode::SoftBreak => rsx! { " " },
        MdNode::HardBreak => rsx! { br {} },
        MdNode::Html(html) => rsx! { "{html}" },
    }
}
