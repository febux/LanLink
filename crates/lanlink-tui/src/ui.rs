//! Ratatui rendering. No business logic: everything displayed is read from
//! [`App`].

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Sparkline, Table, Wrap,
};
use ratatui::Frame;

use crate::app::{App, Focus, Severity, Tab};

/// Top-level draw: header (tabs), body (screen), footer (status bar).
pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.width < 24 || area.height < 10 {
        let msg = Paragraph::new("terminal too small (need >= 24x10)")
            .style(Style::new().fg(Color::Yellow))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, area);
        return;
    }

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(f, app, chunks[0]);
    match app.tab {
        Tab::Dashboard => draw_dashboard(f, app, chunks[1]),
        Tab::Peers => draw_peers(f, app, chunks[1]),
        Tab::Networks => draw_networks(f, app, chunks[1]),
    }
    draw_footer(f, app, chunks[2]);

    if app.focus == Focus::Help {
        draw_help(f, area);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    let mut title_spans = vec![Span::styled("LanLink", Style::new().fg(Color::Cyan).bold())];
    if let Some(snapshot) = &app.snapshot {
        title_spans.push(Span::raw(" "));
        title_spans.push(Span::styled(
            format!("network {}", snapshot.status.network_id.0),
            Style::new().dim(),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(title_spans)), rows[0]);

    let mut tab_spans: Vec<Span> = Vec::new();
    for (i, t) in Tab::ALL.iter().enumerate() {
        if i > 0 {
            tab_spans.push(Span::raw(" "));
        }
        let label = format!(" {} [{}] ", t.title(), i + 1);
        let style = if *t == app.tab {
            Style::new().fg(Color::Black).bg(Color::Cyan).bold()
        } else {
            Style::new().fg(Color::Gray)
        };
        tab_spans.push(Span::styled(label, style));
    }
    f.render_widget(Paragraph::new(Line::from(tab_spans)), rows[1]);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let daemon_state = if app.daemon_offline() {
        Span::styled("daemon: offline", Style::new().fg(Color::Red).bold())
    } else if app.snapshot.is_some() {
        Span::styled("daemon: online", Style::new().fg(Color::Green))
    } else {
        Span::styled("daemon: connecting", Style::new().fg(Color::Yellow))
    };

    let center = match (&app.toast, app.busy) {
        (_, true) => Span::styled("working...", Style::new().fg(Color::Yellow)),
        (Some(toast), false) => {
            let style = match toast.severity {
                Severity::Info => Style::new().fg(Color::Cyan),
                Severity::Error => Style::new().fg(Color::Red).bold(),
            };
            Span::styled(toast.message.clone(), style)
        }
        (None, false) => Span::styled("? help  q quit", Style::new().dim()),
    };

    let right = Span::styled(
        format!(" {} │ {} ", app.socket_desc, app.coordinator),
        Style::new().dim(),
    );

    let line = Line::from(vec![
        daemon_state,
        Span::raw(" │ "),
        center,
        Span::raw(" "),
        right,
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    // Left: daemon info.
    let (rows, title) = match &app.snapshot {
        Some(snapshot) => {
            let s = &snapshot.status;
            let relay = s
                .relay_addr
                .map(|a| a.to_string())
                .unwrap_or_else(|| "none".into());
            let badges = format!(
                "{}{}",
                if s.gateway_enabled { "gateway " } else { "" },
                if s.firewall_enabled { "firewall" } else { "" },
            );
            let badges = if badges.is_empty() {
                "-".to_string()
            } else {
                badges
            };
            (
                vec![
                    ("Network", s.network_id.0.to_string()),
                    ("Virtual IP", s.assigned_ip.to_string()),
                    ("Interface", s.tap_name.clone()),
                    ("UDP bind", s.bind.to_string()),
                    ("Uptime", fmt_duration(s.uptime_seconds)),
                    ("Relay", relay),
                    ("Features", badges),
                    ("Peers", snapshot.peers.len().to_string()),
                    ("TX total", fmt_bytes(s.tx_bytes)),
                    ("RX total", fmt_bytes(s.rx_bytes)),
                ],
                format!(" lanlinkd v{} ", s.version),
            )
        }
        None => (
            vec![(
                "Status",
                app.snapshot_error
                    .clone()
                    .unwrap_or_else(|| "waiting for first snapshot".into()),
            )],
            " lanlinkd ".to_string(),
        ),
    };

    let info_rows: Vec<Row> = rows
        .into_iter()
        .map(|(k, v)| Row::new(vec![Cell::from(k), Cell::from(v)]))
        .collect();
    let info = Table::new(info_rows, [Constraint::Length(14), Constraint::Min(10)])
        .block(Block::new().borders(Borders::ALL).title(title))
        .column_spacing(2);
    f.render_widget(info, cols[0]);

    // Right: rate sparklines + a peer gauge.
    let right_rows = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Min(3),
    ])
    .split(cols[1]);

    let (tx_now, rx_now) = (
        app.tx_rate.last().copied().unwrap_or(0),
        app.rx_rate.last().copied().unwrap_or(0),
    );
    let tx = Sparkline::default()
        .data(&app.tx_rate)
        .style(Style::new().fg(Color::Green))
        .block(
            Block::new()
                .borders(Borders::ALL)
                .title(format!(" TX {}/s ", fmt_bytes(tx_now))),
        );
    f.render_widget(tx, right_rows[0]);

    let rx = Sparkline::default()
        .data(&app.rx_rate)
        .style(Style::new().fg(Color::Cyan))
        .block(
            Block::new()
                .borders(Borders::ALL)
                .title(format!(" RX {}/s ", fmt_bytes(rx_now))),
        );
    f.render_widget(rx, right_rows[1]);

    if let Some(snapshot) = &app.snapshot {
        let total = snapshot.peers.len().max(1);
        let direct = snapshot
            .peers
            .iter()
            .filter(|p| p.path == lanlink_proto::status::PathKind::Direct)
            .count();
        let ratio = direct as f64 / total as f64;
        let gauge = Gauge::default()
            .ratio(ratio.clamp(0.0, 1.0))
            .gauge_style(Style::new().fg(Color::Green))
            .label(format!("direct paths {direct}/{}", snapshot.peers.len()))
            .block(Block::new().borders(Borders::ALL).title(" Connectivity "));
        f.render_widget(gauge, right_rows[2]);
    } else {
        let placeholder = Paragraph::new("no connectivity data")
            .block(Block::new().borders(Borders::ALL).title(" Connectivity "))
            .style(Style::new().dim());
        f.render_widget(placeholder, right_rows[2]);
    }
}

fn draw_peers(f: &mut Frame, app: &App, area: Rect) {
    let header_style = Style::new().fg(Color::Yellow).bold();
    let sort_note = app.peer_sort.title();

    let header = Row::new(vec![
        Cell::from("Node"),
        Cell::from("Virtual IP"),
        Cell::from("Endpoint"),
        Cell::from("Path"),
        Cell::from("Latency"),
        Cell::from("Last seen"),
        Cell::from("TX"),
        Cell::from("RX"),
    ])
    .style(header_style);

    let rows: Vec<Row> = app
        .sorted_peers()
        .into_iter()
        .map(|p| {
            let node = p.node_id.0[..4]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let endpoint = p
                .endpoint
                .as_ref()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "-".into());
            let latency = match p.latency_ms {
                Some(ms) => format!("{ms} ms"),
                None => "-".into(),
            };
            let seen = match p.last_seen_secs_ago {
                Some(0) => "now".into(),
                Some(s) => format!("{s} s"),
                None => "-".into(),
            };
            let gateway = if p.is_gateway { "+" } else { "" };
            Row::new(vec![
                Cell::from(format!("{gateway}{node}")),
                Cell::from(p.virtual_ip.to_string()),
                Cell::from(endpoint),
                Cell::from(p.path.to_string()).style(path_style(p.path)),
                Cell::from(latency).style(latency_style(p.latency_ms)),
                Cell::from(seen),
                Cell::from(fmt_bytes(p.tx_bytes)),
                Cell::from(fmt_bytes(p.rx_bytes)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(13),
            Constraint::Length(22),
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
        ],
    )
    .header(header)
    .block(
        Block::new()
            .borders(Borders::ALL)
            .title(format!(" Peers (s: sort by {sort_note}) ")),
    )
    .column_spacing(1);

    f.render_widget(table, area);
}

fn draw_networks(f: &mut Frame, app: &App, area: Rect) {
    let cols =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).split(area);

    // Left: public directory list.
    let items: Vec<ListItem> = if app.directory.is_empty() {
        vec![
            ListItem::new(Line::from(app.directory_error.clone().unwrap_or_else(
                || "no public networks (or coordinator unreachable)".into(),
            )))
            .style(Style::new().dim()),
        ]
    } else {
        app.directory
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let marker = if i == app.dir_selected && app.focus == Focus::DirList {
                    ">"
                } else {
                    " "
                };
                let public = if n.is_public { " [public]" } else { "" };
                ListItem::new(Line::from(format!(
                    "{marker} {} — {} ({} members{})",
                    n.network_id.0, n.name, n.member_count, public,
                )))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::new()
                .borders(Borders::ALL)
                .title(" Public directory "),
        )
        .highlight_style(Style::new().bg(Color::DarkGray));
    f.render_widget(list, cols[0]);

    // Right: selected details + last join result.
    let detail = match app.directory.get(app.dir_selected) {
        Some(n) => format!(
            "network  {}\nname     {}\nsubnet   {}\nmembers  {}\npublic   {}\n\nenter: join this network\nc: create   j: join by id",
            n.network_id.0,
            n.name,
            n.subnet,
            n.member_count,
            n.is_public,
        ),
        None => "c: create a network\nj: join by id".to_string(),
    };

    let mut right_text = detail;
    if let Some(join) = &app.join_result {
        right_text.push_str("\n\n─ last join ─\n");
        right_text.push_str(&format!(
            "assigned IP {}\npeers       {}\n",
            join.assigned_ip,
            join.peers.len()
        ));
        for p in &join.peers {
            right_text.push_str(&format!(
                "  {} {}\n",
                p.virtual_ip,
                if p.is_gateway {
                    format!(" (gateway {})", p.shared_subnet)
                } else {
                    String::new()
                }
            ));
        }
    }

    let detail_pane = Paragraph::new(right_text)
        .block(Block::new().borders(Borders::ALL).title(" Details "))
        .wrap(Wrap { trim: false });
    f.render_widget(detail_pane, cols[1]);

    // Form popups.
    if app.focus == Focus::CreateForm {
        draw_form(f, area, &app.create_form, "Create network");
    } else if app.focus == Focus::JoinForm {
        draw_form(f, area, &app.join_form, "Join network");
    }
}

fn draw_form(f: &mut Frame, area: Rect, form: &crate::app::Form, _title: &str) {
    let popup = centered_rect(area, 60, form.labels.len() as u16 * 3 + 4);
    f.render_widget(Clear, popup);

    let rows: Vec<Line> = form
        .labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let focused = i == form.cursor;
            let is_toggle = form.toggle == Some(i);
            let is_password = form.password == Some(i);
            let value = if is_toggle {
                if form.values.get(i).map(|v| v == "1").unwrap_or(false) {
                    "[x]".into()
                } else {
                    "[ ]".into()
                }
            } else if focused {
                let text = form
                    .values
                    .get(i)
                    .map(|v| {
                        if is_password {
                            "*".repeat(v.len())
                        } else {
                            v.clone()
                        }
                    })
                    .unwrap_or_default();
                format!("{text}▏")
            } else {
                form.values
                    .get(i)
                    .map(|v| {
                        if is_password {
                            "*".repeat(v.len())
                        } else {
                            v.clone()
                        }
                    })
                    .unwrap_or_default()
            };
            let style = if focused {
                Style::new().fg(Color::Yellow).bold()
            } else {
                Style::new()
            };
            Line::from(vec![
                Span::styled(format!("{label:<22} "), Style::new().dim()),
                Span::styled(value, style),
            ])
        })
        .collect();

    let para = Paragraph::new(rows)
        .block(
            Block::new()
                .borders(Borders::ALL)
                .title(format!(" {} ", form.title))
                .style(Style::new().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let popup = centered_rect(area, 56, 20);
    f.render_widget(Clear, popup);

    let lines: Vec<Line> = [
        "",
        "  1 / 2 / 3, arrows  switch tab",
        "  q, Ctrl-C          quit",
        "  ?                  toggle this help",
        "",
        "  Dashboard          live daemon overview",
        "  Peers       s      cycle sort (latency / ip / traffic)",
        "  Networks    arrows browse the public directory",
        "             enter   prefill join form with selection",
        "             c       create a network",
        "             j       join by network id",
        "",
        "  forms       tab    next field",
        "             space   toggle a boolean field",
        "             enter   submit (on the last field)",
        "             esc     cancel",
    ]
    .iter()
    .map(|l| Line::from(*l))
    .collect();

    let help = Paragraph::new(lines)
        .block(
            Block::new()
                .borders(Borders::ALL)
                .title(" Help ")
                .style(Style::new().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(help, popup);
}

fn centered_rect(area: Rect, percent_x: u16, height: u16) -> Rect {
    let popup_width = area.width.saturating_sub(2) * percent_x / 100;
    let popup_width = popup_width.max(30).min(area.width.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let height = height.min(area.height.saturating_sub(2));
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, popup_width, height)
}

fn path_style(path: lanlink_proto::status::PathKind) -> Style {
    use lanlink_proto::status::PathKind;
    match path {
        PathKind::Direct => Style::new().fg(Color::Green),
        PathKind::Relay => Style::new().fg(Color::Yellow),
        PathKind::TcpDirect => Style::new().fg(Color::LightGreen),
        PathKind::TcpRelay => Style::new().fg(Color::LightYellow),
        PathKind::Punching => Style::new().fg(Color::Cyan),
        PathKind::Waiting => Style::new().dim(),
    }
}

fn latency_style(ms: Option<u64>) -> Style {
    match ms {
        Some(ms) if ms < 50 => Style::new().fg(Color::Green),
        Some(ms) if ms < 150 => Style::new().fg(Color::Yellow),
        Some(_) => Style::new().fg(Color::Red),
        None => Style::new().dim(),
    }
}

pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn fmt_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}
