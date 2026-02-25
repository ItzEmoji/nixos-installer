use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Step};
use crate::disk::FsType;
use crate::theme::Theme;

/// Helper to create a rounded block with the theme's border style.
fn themed_block<'a>(theme: &Theme, title: &str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent_dim))
        .title(title.to_string())
        .title_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg))
}

/// Themed block with a custom border color.
fn themed_block_colored<'a>(theme: &Theme, title: &str, border_color: Color) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(title.to_string())
        .title_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme.bg))
}

/// Main render function dispatching to step-specific renderers.
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let t = &app.theme;

    frame.render_widget(Block::default().style(Style::default().bg(t.bg)), area);

    let [header_area, progress_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .areas(area);

    render_header(frame, app, header_area);
    render_progress(frame, app, progress_area);
    render_footer(frame, footer_area, app);

    match app.step {
        Step::CloningRepo => render_cloning(frame, app, body_area),
        Step::SelectPreset => render_select_preset(frame, app, body_area),
        Step::HostName => render_text_input(frame, app, body_area, "Host Name", false),
        Step::SelectNixosModules => render_module_checklist(
            frame,
            &app.theme,
            &app.nixos_modules,
            app.nixos_cursor,
            " Select NixOS Modules (Space to toggle) ",
            body_area,
        ),
        Step::SelectSystemPackages => render_module_checklist(
            frame,
            &app.theme,
            &app.system_packages,
            app.system_package_cursor,
            " Select System Packages (Space to toggle) ",
            body_area,
        ),
        Step::CreateUser => render_text_input(frame, app, body_area, "Username", false),
        Step::UserPassword => render_text_input(frame, app, body_area, "Password", true),
        Step::UserPasswordConfirm => {
            render_text_input(frame, app, body_area, "Confirm Password", true)
        }
        Step::AddAnotherUser => render_yes_no(frame, &app.theme, app.another_user_cursor, body_area, "Add another user?"),
        Step::SelectHmModules => {
            let title = if app.hm_user_index < app.users.len() {
                format!(
                    " HM Modules for '{}' (Space to toggle) ",
                    app.users[app.hm_user_index].username
                )
            } else {
                " Select Home Manager Modules (Space to toggle) ".to_string()
            };
            render_module_checklist(frame, &app.theme, &app.hm_modules, app.hm_cursor, &title, body_area);
        }
        Step::SelectUserPackages => {
            let title = if app.hm_user_index < app.users.len() {
                format!(
                    " Packages for '{}' (Space to toggle) ",
                    app.users[app.hm_user_index].username
                )
            } else {
                " Select User Packages (Space to toggle) ".to_string()
            };
            render_module_checklist(frame, &app.theme, &app.user_pkg_modules, app.user_pkg_cursor, &title, body_area);
        }
        Step::SelectDisk => render_select_disk(frame, app, body_area),
        Step::PartitionModeSelect => render_partition_mode(frame, app, body_area),
        Step::SwapSize => render_text_input(frame, app, body_area, "Swap Size (GiB)", false),
        Step::CustomPartitionMount => {
            render_text_input(frame, app, body_area, "Mount Point (e.g. /, /boot, swap)", false)
        }
        Step::CustomPartitionSize => render_text_input(
            frame,
            app,
            body_area,
            "Size in GiB (leave empty for remaining space)",
            false,
        ),
        Step::CustomPartitionFs => render_fs_select(frame, app, body_area),
        Step::CustomPartitionAnother => {
            render_yes_no(frame, &app.theme, app.another_partition_cursor, body_area, "Add another partition?")
        }
        Step::Confirm => render_confirm(frame, app, body_area),
        Step::Installing => render_installing(frame, app, body_area),
        Step::RootPassword => render_text_input(frame, app, body_area, "Root Password", true),
        Step::RootPasswordConfirm => {
            render_text_input(frame, app, body_area, "Confirm Root Password", true)
        }
        Step::Complete => render_complete(frame, app, body_area),
    }

    if let Some(msg) = &app.status_message {
        render_status_popup(frame, &app.theme, area, msg);
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let title = format!(
        " {} - {} [{}/{}] ",
        app.branding_title,
        app.step_title(),
        app.step_number(),
        app.total_steps()
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.accent))
        .title(title)
        .title_style(Style::default().fg(t.accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(t.bg));
    frame.render_widget(block, area);
}

fn render_progress(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let ratio = app.step_number() as f64 / app.total_steps() as f64;
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.accent_dim))
                .style(Style::default().bg(t.bg)),
        )
        .gauge_style(Style::default().fg(t.accent).bg(t.surface))
        .ratio(ratio)
        .label(format!("Step {}/{}", app.step_number(), app.total_steps()));
    frame.render_widget(gauge, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let hints = match app.step {
        Step::CloningRepo => {
            if app.clone_error.is_some() {
                vec![
                    Span::styled(" Up/Down ", Style::default().fg(t.accent).bold()),
                    Span::styled("Scroll log ", Style::default().fg(t.text_dim)),
                    Span::styled(" Enter ", Style::default().fg(t.red).bold()),
                    Span::styled("Quit ", Style::default().fg(t.text_dim)),
                ]
            } else {
                vec![Span::styled(
                    " Cloning repository, please wait... ",
                    Style::default().fg(t.yellow),
                )]
            }
        }
        Step::SelectPreset | Step::SelectDisk => {
            vec![
                Span::styled(" Up/Down ", Style::default().fg(t.accent).bold()),
                Span::styled("Navigate ", Style::default().fg(t.text_dim)),
                Span::styled(" Enter ", Style::default().fg(t.accent).bold()),
                Span::styled("Select ", Style::default().fg(t.text_dim)),
                Span::styled(" q ", Style::default().fg(t.red).bold()),
                Span::styled("Quit", Style::default().fg(t.text_dim)),
            ]
        }
        Step::PartitionModeSelect => {
            vec![
                Span::styled(" Up/Down ", Style::default().fg(t.accent).bold()),
                Span::styled("Navigate ", Style::default().fg(t.text_dim)),
                Span::styled(" Enter ", Style::default().fg(t.accent).bold()),
                Span::styled("Select ", Style::default().fg(t.text_dim)),
                Span::styled(" Esc ", Style::default().fg(t.yellow).bold()),
                Span::styled("Back", Style::default().fg(t.text_dim)),
            ]
        }
        Step::SelectNixosModules | Step::SelectHmModules | Step::SelectSystemPackages | Step::SelectUserPackages => {
            vec![
                Span::styled(" Up/Down ", Style::default().fg(t.accent).bold()),
                Span::styled("Navigate ", Style::default().fg(t.text_dim)),
                Span::styled(" Space ", Style::default().fg(t.accent).bold()),
                Span::styled("Toggle ", Style::default().fg(t.text_dim)),
                Span::styled(" Enter ", Style::default().fg(t.accent).bold()),
                Span::styled("Confirm ", Style::default().fg(t.text_dim)),
                Span::styled(" Esc ", Style::default().fg(t.yellow).bold()),
                Span::styled("Back ", Style::default().fg(t.text_dim)),
                Span::styled(" q ", Style::default().fg(t.red).bold()),
                Span::styled("Quit", Style::default().fg(t.text_dim)),
            ]
        }
        Step::AddAnotherUser | Step::CustomPartitionAnother | Step::Complete => {
            vec![
                Span::styled(" Left/Right ", Style::default().fg(t.accent).bold()),
                Span::styled("Choose ", Style::default().fg(t.text_dim)),
                Span::styled(" Enter ", Style::default().fg(t.accent).bold()),
                Span::styled("Confirm ", Style::default().fg(t.text_dim)),
            ]
        }
        Step::Confirm => {
            vec![
                Span::styled(" Left/Right ", Style::default().fg(t.accent).bold()),
                Span::styled("Choose ", Style::default().fg(t.text_dim)),
                Span::styled(" Space ", Style::default().fg(t.accent).bold()),
                Span::styled("Toggle ", Style::default().fg(t.text_dim)),
                Span::styled(" Enter ", Style::default().fg(t.accent).bold()),
                Span::styled("Confirm ", Style::default().fg(t.text_dim)),
            ]
        }
        Step::Installing => {
            if app.install_error.is_some() {
                vec![
                    Span::styled(" Up/Down ", Style::default().fg(t.accent).bold()),
                    Span::styled("Scroll log ", Style::default().fg(t.text_dim)),
                    Span::styled(" Enter ", Style::default().fg(t.red).bold()),
                    Span::styled("Quit ", Style::default().fg(t.text_dim)),
                ]
            } else if app.install_done {
                vec![
                    Span::styled(" Enter ", Style::default().fg(t.accent).bold()),
                    Span::styled("Continue ", Style::default().fg(t.text_dim)),
                ]
            } else {
                vec![Span::styled(
                    " Please wait... ",
                    Style::default().fg(t.yellow),
                )]
            }
        }
        _ => {
            vec![
                Span::styled(" Type ", Style::default().fg(t.accent).bold()),
                Span::styled("to enter text ", Style::default().fg(t.text_dim)),
                Span::styled(" Enter ", Style::default().fg(t.accent).bold()),
                Span::styled("Confirm ", Style::default().fg(t.text_dim)),
                Span::styled(" Esc ", Style::default().fg(t.yellow).bold()),
                Span::styled("Back", Style::default().fg(t.text_dim)),
            ]
        }
    };
    let line = Line::from(hints);
    let p = Paragraph::new(line).style(Style::default().bg(t.bg));
    frame.render_widget(p, area);
}

// ---- Step-specific renderers ----

fn render_cloning(frame: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme.clone();
    let [progress_area, log_area] =
        Layout::vertical([Constraint::Length(5), Constraint::Fill(1)]).areas(area);

    let ratio = app.clone_percent as f64 / 100.0;

    let gauge_style = if app.clone_error.is_some() {
        Style::default().fg(t.red).bg(t.surface)
    } else if app.clone_done {
        Style::default().fg(t.green).bg(t.surface)
    } else {
        Style::default().fg(t.accent).bg(t.surface)
    };

    let label = if app.clone_error.is_some() {
        "Clone FAILED - see log below".to_string()
    } else if app.clone_done {
        "Clone complete!".to_string()
    } else if app.clone_phase.is_empty() {
        "Starting...".to_string()
    } else {
        app.clone_phase.clone()
    };

    let gauge = Gauge::default()
        .block(themed_block(&t, " Cloning Repository "))
        .gauge_style(gauge_style)
        .ratio(ratio.min(1.0))
        .label(label);
    frame.render_widget(gauge, progress_area);

    // Auto-scroll clone log
    if app.auto_scroll && !app.clone_log.is_empty() {
        let inner_height = log_area.height.saturating_sub(2) as usize;
        if app.clone_log.len() > inner_height {
            app.clone_log_scroll = app.clone_log.len() - inner_height;
        } else {
            app.clone_log_scroll = 0;
        }
    }

    let log_lines: Vec<Line> = app
        .clone_log
        .iter()
        .map(|l| {
            let color = if l.starts_with("ERROR") || l.contains("fatal") || l.contains("error") {
                t.red
            } else if l.contains("complete") || l.contains("Complete") || l.contains("done") {
                t.green
            } else {
                t.text_dim
            };
            Line::from(format!("  {}", l)).style(Style::default().fg(color))
        })
        .collect();

    let log_title = if app.clone_error.is_some() {
        " Log (Up/Down to scroll | Enter to quit) ".to_string()
    } else {
        " Log ".to_string()
    };

    let border_color = if app.clone_error.is_some() {
        t.red
    } else {
        t.accent_dim
    };

    let log = Paragraph::new(Text::from(log_lines))
        .block(themed_block_colored(&t, &log_title, border_color))
        .scroll((app.clone_log_scroll as u16, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(log, log_area);
}

fn render_select_preset(frame: &mut Frame, app: &mut App, area: Rect) {
    let t = &app.theme;
    let items: Vec<ListItem> = app
        .preset_display_items()
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let style = if i == app.preset_cursor {
                Style::default()
                    .fg(t.bg)
                    .bg(t.accent)
                    .add_modifier(Modifier::BOLD)
            } else if name == "Custom" {
                Style::default().fg(t.yellow)
            } else {
                Style::default().fg(t.text)
            };
            let prefix = if name == "Custom" { "+ " } else { "  " };
            ListItem::new(format!("{}{}", prefix, name)).style(style)
        })
        .collect();

    let list = List::new(items).block(themed_block(t, " Select a host preset "));

    let mut state = ListState::default();
    state.select(Some(app.preset_cursor));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_text_input(frame: &mut Frame, app: &App, area: Rect, label: &str, masked: bool) {
    let t = &app.theme;
    let [_spacer_top, input_area, msg_area, _spacer_bottom] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(5),
        Constraint::Length(3),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_pad_left, center, _pad_right] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Percentage(60),
        Constraint::Fill(1),
    ])
    .areas(input_area);

    let input_text = match app.current_input_ref() {
        Some(text) => {
            if masked {
                "*".repeat(text.len())
            } else {
                text.to_string()
            }
        }
        None => String::new(),
    };

    let cursor = if input_text.is_empty() {
        "_".to_string()
    } else {
        format!("{}_", input_text)
    };

    let input = Paragraph::new(cursor)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.accent))
                .title(format!(" {} ", label))
                .title_style(Style::default().fg(t.accent).bold())
                .style(Style::default().bg(t.surface)),
        )
        .style(Style::default().fg(t.text));

    frame.render_widget(input, center);

    // Show password mismatch warning
    let [_ml, msg_center, _mr] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Percentage(60),
        Constraint::Fill(1),
    ])
    .areas(msg_area);

    let show_pw_warn = app.password_mismatch
        && (app.step == Step::UserPassword || app.step == Step::UserPasswordConfirm);
    let show_root_warn = app.root_password_mismatch
        && (app.step == Step::RootPassword || app.step == Step::RootPasswordConfirm);

    if show_pw_warn || show_root_warn {
        let warn = Paragraph::new("Passwords did not match. Please try again.")
            .style(Style::default().fg(t.red))
            .wrap(Wrap { trim: true });
        frame.render_widget(warn, msg_center);
    }
}

/// Render a checklist of NixModule items.
fn render_module_checklist(
    frame: &mut Frame,
    theme: &Theme,
    modules: &[crate::nix::NixModule],
    cursor: usize,
    title: &str,
    area: Rect,
) {
    if modules.is_empty() {
        let msg = Paragraph::new(Text::from(vec![
            Line::from(""),
            Line::from("  No modules found.")
                .style(Style::default().fg(theme.red).add_modifier(Modifier::BOLD)),
            Line::from(""),
            Line::from("  The module directory could not be read.")
                .style(Style::default().fg(theme.text_dim)),
            Line::from("  Make sure the installer is run from the nixos-dots repo root,")
                .style(Style::default().fg(theme.text_dim)),
            Line::from("  or pass the repo path as a CLI argument:")
                .style(Style::default().fg(theme.text_dim)),
            Line::from(""),
            Line::from("    nixos-installer /path/to/nixos-dots")
                .style(Style::default().fg(theme.yellow)),
            Line::from(""),
            Line::from("  Press Enter to continue without selecting modules.")
                .style(Style::default().fg(theme.text_dim)),
        ]))
        .block(themed_block_colored(theme, title, theme.red));
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = modules
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let checkbox = if m.selected { "[x]" } else { "[ ]" };
            let style = if i == cursor {
                Style::default()
                    .fg(theme.bg)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else if m.selected {
                Style::default().fg(theme.green)
            } else {
                Style::default().fg(theme.text)
            };

            let display = format!(" {} {}", checkbox, m.name);

            ListItem::new(display).style(style)
        })
        .collect();

    let list = List::new(items).block(themed_block(theme, title));

    let mut state = ListState::default();
    state.select(Some(cursor));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_select_disk(frame: &mut Frame, app: &mut App, area: Rect) {
    let t = &app.theme;
    if app.disks.is_empty() {
        let msg = Paragraph::new(Text::from(vec![
            Line::from(""),
            Line::from("  No disks found.")
                .style(Style::default().fg(t.red).add_modifier(Modifier::BOLD)),
            Line::from(""),
            Line::from("  Make sure you are running as root and have")
                .style(Style::default().fg(t.text_dim)),
            Line::from("  physical disks attached to the system.")
                .style(Style::default().fg(t.text_dim)),
            Line::from(""),
            Line::from("  Press Esc to quit.")
                .style(Style::default().fg(t.text_dim)),
        ]))
        .block(themed_block_colored(t, " Error ", t.red));
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .disks
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let style = if i == app.disk_cursor {
                Style::default()
                    .fg(t.bg)
                    .bg(t.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.text)
            };
            ListItem::new(format!("  {} - {} [{}]", d.path, d.size_human, d.model)).style(style)
        })
        .collect();

    let list = List::new(items).block(themed_block(t, " Select Installation Disk "));

    let mut state = ListState::default();
    state.select(Some(app.disk_cursor));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_partition_mode(frame: &mut Frame, app: &mut App, area: Rect) {
    let t = &app.theme;
    let options = vec![
        ("Use Full Disk", "Automatic EFI + swap + root partitioning"),
        (
            "Custom Partitions",
            "Manually define mount points, sizes, and filesystems",
        ),
    ];

    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, (name, desc))| {
            let style = if i == app.partition_mode_cursor {
                Style::default()
                    .fg(t.bg)
                    .bg(t.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.text)
            };
            ListItem::new(Text::from(vec![
                Line::from(format!("  {}", name)),
                Line::from(format!("    {}", desc)).style(Style::default().fg(t.text_dim)),
            ]))
            .style(style)
        })
        .collect();

    let list = List::new(items).block(themed_block(t, " Partition Mode "));

    let mut state = ListState::default();
    state.select(Some(app.partition_mode_cursor));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_fs_select(frame: &mut Frame, app: &mut App, area: Rect) {
    let t = &app.theme;
    let fs_types = FsType::all();
    let items: Vec<ListItem> = fs_types
        .iter()
        .enumerate()
        .map(|(i, fs)| {
            let style = if i == app.part_fs_cursor {
                Style::default()
                    .fg(t.bg)
                    .bg(t.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.text)
            };
            ListItem::new(format!("  {}", fs.display_name())).style(style)
        })
        .collect();

    let list = List::new(items).block(
        themed_block(t, &format!(" Filesystem for '{}' ", app.part_mount_input)),
    );

    let mut state = ListState::default();
    state.select(Some(app.part_fs_cursor));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_yes_no(frame: &mut Frame, theme: &Theme, cursor: usize, area: Rect, question: &str) {
    let [_top, center, _bottom] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(7),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_left, mid, _right] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Percentage(50),
        Constraint::Fill(1),
    ])
    .areas(center);

    let yes_style = if cursor == 0 {
        Style::default()
            .fg(theme.bg)
            .bg(theme.green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.green)
    };
    let no_style = if cursor == 1 {
        Style::default()
            .fg(theme.bg)
            .bg(theme.red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.red)
    };

    let text = Text::from(vec![
        Line::from(""),
        Line::from(question).style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        Line::from(""),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("  Yes  ", yes_style),
            Span::raw("    "),
            Span::styled("  No  ", no_style),
        ]),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent_dim))
        .style(Style::default().bg(theme.bg));

    let p = Paragraph::new(text).block(block).centered();
    frame.render_widget(p, mid);
}

fn render_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let [summary_area, button_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]).areas(area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    lines.push(
        Line::from(format!("  Host: {}", app.host_name))
            .style(Style::default().fg(t.accent).bold()),
    );
    lines.push(
        Line::from(format!(
            "  Mode: {}",
            if app.is_custom { "Custom" } else { "Preset" }
        ))
        .style(Style::default().fg(t.text)),
    );

    if let Some(disk) = &app.selected_disk {
        lines.push(Line::from(""));
        lines.push(
            Line::from(format!("  Disk: {} ({})", disk.path, disk.size_human))
                .style(Style::default().fg(t.accent)),
        );
    }

    lines.push(Line::from(""));
    lines.push(Line::from("  Partitions:").style(Style::default().fg(t.yellow).bold()));
    for p in &app.partitions {
        let size = match p.size_mb {
            Some(mb) => format!("{:.1} GiB", mb as f64 / 1024.0),
            None => "remaining".to_string(),
        };
        lines.push(
            Line::from(format!(
                "    {} -> {} ({}) [{}]",
                p.label,
                p.mount_point,
                size,
                p.fs_type.as_str()
            ))
            .style(Style::default().fg(t.text)),
        );
    }

    lines.push(Line::from(""));
    lines.push(Line::from("  Users:").style(Style::default().fg(t.yellow).bold()));
    for u in &app.users {
        let mod_count = u.hm_modules.iter().filter(|m| m.selected).count();
        let pkg_count = u.package_modules.iter().filter(|m| m.selected).count();
        lines.push(
            Line::from(format!("    {} ({} HM modules, {} packages)", u.username, mod_count, pkg_count))
                .style(Style::default().fg(t.text)),
        );
    }

    if app.is_custom {
        let nixos_count = app.nixos_modules.iter().filter(|m| m.selected).count();
        let sys_pkg_count = app.system_packages.iter().filter(|m| m.selected).count();
        lines.push(Line::from(""));
        lines.push(
            Line::from(format!(
                "  NixOS Modules: {} selected, System Packages: {} selected",
                nixos_count, sys_pkg_count
            ))
            .style(Style::default().fg(t.text)),
        );
    }

    lines.push(Line::from(""));
    let flake_checkbox = if app.accept_flake_config {
        "[x]"
    } else {
        "[ ]"
    };
    let flake_style = if app.accept_flake_config {
        Style::default().fg(t.green)
    } else {
        Style::default().fg(t.text_dim)
    };
    lines.push(
        Line::from(format!(
            "  {} accept-flake-config  (Space to toggle)",
            flake_checkbox
        ))
        .style(flake_style),
    );

    lines.push(Line::from(""));
    lines.push(
        Line::from("  WARNING: This will ERASE all data on the selected disk!")
            .style(Style::default().fg(t.red).add_modifier(Modifier::BOLD)),
    );

    let summary = Paragraph::new(Text::from(lines))
        .block(themed_block(t, " Installation Summary "))
        .wrap(Wrap { trim: false });
    frame.render_widget(summary, summary_area);

    let cursor = app.confirm_cursor;
    let yes_style = if cursor == 0 {
        Style::default()
            .fg(t.bg)
            .bg(t.green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.green)
    };
    let no_style = if cursor == 1 {
        Style::default()
            .fg(t.bg)
            .bg(t.red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.red)
    };

    let buttons = Paragraph::new(Line::from(vec![
        Span::raw("    "),
        Span::styled("  Install  ", yes_style),
        Span::raw("    "),
        Span::styled("  Go Back  ", no_style),
    ]))
    .centered()
    .style(Style::default().bg(t.bg));

    frame.render_widget(buttons, button_area);
}

fn render_installing(frame: &mut Frame, app: &mut App, area: Rect) {
    let t = app.theme.clone();
    let [progress_area, log_area] =
        Layout::vertical([Constraint::Length(5), Constraint::Fill(1)]).areas(area);

    let ratio = if app.install_total > 0 {
        app.install_progress as f64 / app.install_total as f64
    } else {
        0.0
    };

    let gauge_style = if app.install_error.is_some() {
        Style::default().fg(t.red).bg(t.surface)
    } else if app.install_done {
        Style::default().fg(t.green).bg(t.surface)
    } else {
        Style::default().fg(t.accent).bg(t.surface)
    };

    let label = if app.install_error.is_some() {
        format!(
            "FAILED at step {}/{} - see log below",
            app.install_progress, app.install_total
        )
    } else if app.install_done {
        "Complete!".to_string()
    } else {
        format!("{}/{}", app.install_progress, app.install_total)
    };

    let gauge = Gauge::default()
        .block(themed_block(&t, " Progress "))
        .gauge_style(gauge_style)
        .ratio(ratio.min(1.0))
        .label(label);
    frame.render_widget(gauge, progress_area);

    // Auto-scroll: if enabled, set scroll so the last log line is visible.
    // The log block has 2 lines of border (top + bottom), leaving inner height.
    if app.auto_scroll && !app.install_log.is_empty() {
        let inner_height = log_area.height.saturating_sub(2) as usize;
        if app.install_log.len() > inner_height {
            app.log_scroll = app.install_log.len() - inner_height;
        } else {
            app.log_scroll = 0;
        }
    }

    let log_lines: Vec<Line> = app
        .install_log
        .iter()
        .map(|l| {
            let color = if l.starts_with("ERROR") || l.starts_with("Warning") {
                t.red
            } else if l.contains("complete") || l.contains("Complete") {
                t.green
            } else {
                t.text_dim
            };
            Line::from(format!("  {}", l)).style(Style::default().fg(color))
        })
        .collect();

    // Scroll support: use app.log_scroll to offset the view
    let log_title = if app.install_error.is_some() {
        format!(
            " Log (Up/Down to scroll) | Full log: {} ",
            crate::app::LOG_FILE
        )
    } else {
        " Log ".to_string()
    };

    let border_color = if app.install_error.is_some() {
        t.red
    } else {
        t.accent_dim
    };

    let log = Paragraph::new(Text::from(log_lines))
        .block(themed_block_colored(&t, &log_title, border_color))
        .scroll((app.log_scroll as u16, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(log, log_area);
}

fn render_complete(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let [_top, center, _bottom] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(11),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_left, mid, _right] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Percentage(60),
        Constraint::Fill(1),
    ])
    .areas(center);

    let cursor = app.reboot_cursor;
    let yes_style = if cursor == 0 {
        Style::default()
            .fg(t.bg)
            .bg(t.green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.green)
    };
    let no_style = if cursor == 1 {
        Style::default()
            .fg(t.bg)
            .bg(t.red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.red)
    };

    let text = Text::from(vec![
        Line::from(""),
        Line::from("  NixOS installation completed successfully!")
            .style(Style::default().fg(t.green).add_modifier(Modifier::BOLD)),
        Line::from(""),
        Line::from(format!("  Host: {}", app.host_name)).style(Style::default().fg(t.accent)),
        Line::from(format!(
            "  Users: {}",
            app.users
                .iter()
                .map(|u| u.username.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .style(Style::default().fg(t.text)),
        Line::from(""),
        Line::from("  Would you like to reboot now?")
            .style(Style::default().fg(t.text).bold()),
        Line::from(""),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("  Reboot  ", yes_style),
            Span::raw("    "),
            Span::styled("  Exit  ", no_style),
        ]),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.green))
        .title(" Complete ")
        .title_style(Style::default().fg(t.green).bold())
        .style(Style::default().bg(t.bg));

    let p = Paragraph::new(text).block(block);
    frame.render_widget(p, mid);
}

fn render_status_popup(frame: &mut Frame, theme: &Theme, area: Rect, msg: &str) {
    let popup = popup_area(area, 50, 20);
    frame.render_widget(Clear, popup);

    let p = Paragraph::new(msg)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.yellow))
                .title(" Notice ")
                .title_style(Style::default().fg(theme.yellow).bold())
                .style(Style::default().bg(theme.surface)),
        )
        .style(Style::default().fg(theme.yellow))
        .wrap(Wrap { trim: true })
        .centered();
    frame.render_widget(p, popup);
}

fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let [_, vert_center, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Percentage(percent_y),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Percentage(percent_x),
        Constraint::Fill(1),
    ])
    .areas(vert_center);

    center
}
