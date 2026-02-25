mod app;
mod config;
mod disk;
mod nix;
mod theme;
mod ui;

use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

use app::{App, Step};
use config::InstallerConfig;
use disk::FsType;
use theme::ThemeName;

/// Default dotfiles repository URL.
const DEFAULT_REPO_URL: &str = "https://github.com/itzemoji/nixos-dotfiles.git";

/// Walk upwards from `start` looking for a directory that contains both
/// `flake.nix` and a `modules/` subdirectory (the nixos-dots repo root).
fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("flake.nix").exists() && current.join("modules").is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Parse CLI arguments.
/// Supports:
///   --repo <URL>        Override the dotfiles repository URL
///   --config <PATH>     Load installer config from a custom path
///   --theme <NAME>      Override the color theme
///   --init              Generate a default config.toml at /etc/nixos-installer/
///   --help              Show usage information
///   <PATH>              Use an existing local repo instead of cloning
struct CliArgs {
    /// The repo URL to clone (None if a local base_path is given).
    repo_url: Option<String>,
    /// An existing local base path (None if cloning).
    base_path: Option<PathBuf>,
    /// Custom config file path.
    config_path: Option<PathBuf>,
    /// Theme override from CLI.
    theme_override: Option<ThemeName>,
    /// Run --init mode: generate config and exit.
    init: bool,
    /// Show help.
    help: bool,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut repo_url: Option<String> = None;
    let mut base_path: Option<PathBuf> = None;
    let mut config_path: Option<PathBuf> = None;
    let mut theme_override: Option<ThemeName> = None;
    let mut init = false;
    let mut help = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--repo" => {
                i += 1;
                if i < args.len() {
                    repo_url = Some(args[i].clone());
                }
            }
            "--config" => {
                i += 1;
                if i < args.len() {
                    config_path = Some(PathBuf::from(&args[i]));
                }
            }
            "--theme" => {
                i += 1;
                if i < args.len() {
                    match ThemeName::from_str_loose(&args[i]) {
                        Some(t) => theme_override = Some(t),
                        None => {
                            eprintln!(
                                "Unknown theme '{}'. Available: {}",
                                args[i],
                                ThemeName::all_names().join(", ")
                            );
                            std::process::exit(1);
                        }
                    }
                }
            }
            "--init" => init = true,
            "--help" | "-h" => help = true,
            other => {
                // Positional argument: local base path
                base_path = Some(PathBuf::from(other));
            }
        }
        i += 1;
    }

    // If no --repo was given, check env var
    if repo_url.is_none() && base_path.is_none() {
        if let Ok(url) = env::var("NIXOS_DOTFILES_REPO") {
            if !url.is_empty() {
                repo_url = Some(url);
            }
        }
    }

    CliArgs {
        repo_url,
        base_path,
        config_path,
        theme_override,
        init,
        help,
    }
}

fn print_help() {
    println!("nixos-installer - A TUI-based NixOS installer");
    println!();
    println!("USAGE:");
    println!("    nixos-installer [OPTIONS] [PATH]");
    println!();
    println!("ARGS:");
    println!("    <PATH>              Use an existing local repo instead of cloning");
    println!();
    println!("OPTIONS:");
    println!("    --repo <URL>        Override the dotfiles repository URL");
    println!("    --config <PATH>     Load config from a custom path (default: /etc/nixos-installer/config.toml)");
    println!("    --theme <NAME>      Override the color theme");
    println!("    --init              Generate a default config.toml at /etc/nixos-installer/");
    println!("    --help, -h          Show this help message");
    println!();
    println!("AVAILABLE THEMES:");
    for name in ThemeName::all_names() {
        println!("    {}", name);
    }
    println!();
    println!("ENVIRONMENT:");
    println!("    NIXOS_DOTFILES_REPO    Fallback repository URL if --repo is not given");
}

fn main() -> io::Result<()> {
    let cli = parse_args();

    if cli.help {
        print_help();
        return Ok(());
    }

    // --init: generate config and exit
    if cli.init {
        let path = cli
            .config_path
            .as_deref()
            .unwrap_or_else(|| Path::new(config::DEFAULT_CONFIG_PATH));
        match config::init_config(path) {
            Ok(()) => {
                println!("Created config at: {}", path.display());
                println!("Edit this file to set your repository URL, theme, and other options.");
                println!();
                println!("To use it:");
                println!("    nixos-installer                    (auto-loads from /etc/nixos-installer/config.toml)");
                println!("    nixos-installer --config {}    (explicit path)", path.display());
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // Load installer config: --config path > default system path
    let config_file = cli
        .config_path
        .as_deref()
        .unwrap_or_else(|| Path::new(config::DEFAULT_CONFIG_PATH));
    let mut installer_config = config::load_config(config_file);

    // CLI overrides
    if let Some(theme) = cli.theme_override {
        installer_config.theme = Some(theme);
    }

    // Resolve the theme (base theme + optional custom overrides)
    let mut theme = installer_config
        .theme
        .as_ref()
        .unwrap_or(&ThemeName::CatppuccinMocha)
        .to_theme();

    // Apply custom color overrides from config if present
    if let Some(ref custom) = installer_config.theme_custom {
        if custom.has_overrides() {
            theme = theme.with_custom_overrides(custom);
        }
    }

    // Determine the repo URL: CLI --repo > config repo_url > env > default
    let cli_repo_url = cli.repo_url.or_else(|| installer_config.repo_url.clone());

    // Determine how we get the base path:
    // 1) Explicit local path from CLI  -> use directly (no clone)
    // 2) Auto-detect local repo        -> use directly (no clone)
    // 3) Otherwise                      -> clone from repo_url (or default)
    let (base_path, repo_url) = if let Some(path) = cli.base_path {
        (Some(path), None)
    } else {
        // Try auto-detect
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        if let Some(root) = find_repo_root(&cwd) {
            (Some(root), None)
        } else if let Some(root) = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .and_then(|p| find_repo_root(&p))
        {
            (Some(root), None)
        } else {
            // No local repo found - we'll need to clone
            let url = cli_repo_url.unwrap_or_else(|| DEFAULT_REPO_URL.to_string());
            (None, Some(url))
        }
    };

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, base_path, repo_url, installer_config, theme);
    ratatui::restore();

    // Print log file location after TUI exits so the user can review
    if std::path::Path::new(app::LOG_FILE).exists() {
        eprintln!("Installation log saved to: {}", app::LOG_FILE);
    }

    result
}

fn run(
    terminal: &mut DefaultTerminal,
    base_path: Option<PathBuf>,
    repo_url: Option<String>,
    installer_config: InstallerConfig,
    theme: theme::Theme,
) -> io::Result<()> {
    let mut app = App::new(base_path, repo_url, installer_config, theme);

    loop {
        // Sync shared clone state each frame when cloning
        if app.step == Step::CloningRepo {
            app.sync_clone_state();
        }

        // Sync shared install state each frame when installing
        if app.step == Step::Installing {
            app.sync_install_state();

            // Auto-scroll: keep log scrolled to bottom
            if app.auto_scroll && !app.install_log.is_empty() {
                // Will be adjusted by render_installing based on visible area,
                // but set a high value so the Paragraph scroll shows the end.
                app.log_scroll = app.install_log.len().saturating_sub(1);
            }
        }

        terminal.draw(|frame| ui::render(frame, &mut app))?;

        if app.should_quit {
            break;
        }

        // Auto-advance when clone finishes (only if no error)
        if app.step == Step::CloningRepo && app.clone_done && app.clone_error.is_none() {
            app.finish_clone();
            continue;
        }

        // Auto-advance when installation finishes
        if app.step == Step::Installing && app.install_done {
            app.step = Step::RootPassword;
            continue;
        }

        // Poll with timeout so the UI redraws during installation
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Ctrl+C always quits (terminal state is restored by ratatui::restore in main)
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                app.should_quit = true;
                continue;
            }

            // Clear status message on any key press
            if app.status_message.is_some() {
                app.status_message = None;
                continue;
            }

            // Esc: try to go back, or quit if at a root step
            if key.code == KeyCode::Esc {
                match app.step {
                    // Steps that don't support Esc at all
                    Step::Installing | Step::Complete | Step::CloningRepo => {}
                    _ => {
                        if !app.go_back() {
                            app.should_quit = true;
                        }
                        continue;
                    }
                }
            }

            // q to quit on list/selection steps
            match app.step {
                Step::SelectPreset
                | Step::SelectDisk
                | Step::SelectNixosModules
                | Step::SelectHmModules
                | Step::SelectSystemPackages
                | Step::SelectUserPackages => {
                    if key.code == KeyCode::Char('q') {
                        app.should_quit = true;
                        continue;
                    }
                }
                _ => {}
            }

            match app.step {
                // ---- Cloning repository ----
                Step::CloningRepo => {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.auto_scroll = false;
                            if app.clone_log_scroll > 0 {
                                app.clone_log_scroll -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let max = app.clone_log.len().saturating_sub(1);
                            if app.clone_log_scroll < max {
                                app.clone_log_scroll += 1;
                            }
                            if app.clone_log_scroll >= max {
                                app.auto_scroll = true;
                            }
                        }
                        KeyCode::Enter => {
                            if app.clone_error.is_some() {
                                app.should_quit = true;
                            }
                        }
                        _ => {}
                    }
                }

                // ---- Preset selection ----
                Step::SelectPreset => {
                    let len = app.preset_display_items().len();
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            let mut c = app.preset_cursor;
                            App::list_prev(len, &mut c);
                            app.preset_cursor = c;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let mut c = app.preset_cursor;
                            App::list_next(len, &mut c);
                            app.preset_cursor = c;
                        }
                        KeyCode::Enter => app.confirm_preset_selection(),
                        _ => {}
                    }
                }

                // ---- Host name input ----
                Step::HostName => match key.code {
                    KeyCode::Enter => app.confirm_host_name(),
                    KeyCode::Backspace => {
                        app.host_name_input.pop();
                    }
                    KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.host_name_input.pop();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.host_name_input.push(c)
                    }
                    _ => {}
                },

                // ---- NixOS module multi-select ----
                Step::SelectNixosModules => {
                    let len = app.nixos_modules.len();
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            let mut c = app.nixos_cursor;
                            App::list_prev(len, &mut c);
                            app.nixos_cursor = c;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let mut c = app.nixos_cursor;
                            App::list_next(len, &mut c);
                            app.nixos_cursor = c;
                        }
                        KeyCode::Char(' ') => {
                            if let Some(m) = app.nixos_modules.get_mut(app.nixos_cursor) {
                                m.selected = !m.selected;
                            }
                        }
                        KeyCode::Enter => app.confirm_nixos_modules(),
                        _ => {}
                    }
                }

                // ---- System package multi-select ----
                Step::SelectSystemPackages => {
                    let len = app.system_packages.len();
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            let mut c = app.system_package_cursor;
                            App::list_prev(len, &mut c);
                            app.system_package_cursor = c;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let mut c = app.system_package_cursor;
                            App::list_next(len, &mut c);
                            app.system_package_cursor = c;
                        }
                        KeyCode::Char(' ') => {
                            if let Some(m) = app.system_packages.get_mut(app.system_package_cursor) {
                                m.selected = !m.selected;
                            }
                        }
                        KeyCode::Enter => app.confirm_system_packages(),
                        _ => {}
                    }
                }

                // ---- Create user ----
                Step::CreateUser => match key.code {
                    KeyCode::Enter => app.confirm_username(),
                    KeyCode::Backspace => {
                        app.current_username.pop();
                    }
                    KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.current_username.pop();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.current_username.push(c)
                    }
                    _ => {}
                },

                // ---- Add another user? ----
                Step::AddAnotherUser => match key.code {
                    KeyCode::Left | KeyCode::Char('h') => app.another_user_cursor = 0,
                    KeyCode::Right | KeyCode::Char('l') => app.another_user_cursor = 1,
                    KeyCode::Enter => app.confirm_another_user(),
                    _ => {}
                },

                // ---- HM module selection ----
                Step::SelectHmModules => {
                    let len = app.hm_modules.len();
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            let mut c = app.hm_cursor;
                            App::list_prev(len, &mut c);
                            app.hm_cursor = c;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let mut c = app.hm_cursor;
                            App::list_next(len, &mut c);
                            app.hm_cursor = c;
                        }
                        KeyCode::Char(' ') => {
                            if let Some(m) = app.hm_modules.get_mut(app.hm_cursor) {
                                m.selected = !m.selected;
                            }
                        }
                        KeyCode::Enter => app.confirm_hm_modules(),
                        _ => {}
                    }
                }

                // ---- Per-user package selection ----
                Step::SelectUserPackages => {
                    let len = app.user_pkg_modules.len();
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            let mut c = app.user_pkg_cursor;
                            App::list_prev(len, &mut c);
                            app.user_pkg_cursor = c;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let mut c = app.user_pkg_cursor;
                            App::list_next(len, &mut c);
                            app.user_pkg_cursor = c;
                        }
                        KeyCode::Char(' ') => {
                            if let Some(m) = app.user_pkg_modules.get_mut(app.user_pkg_cursor) {
                                m.selected = !m.selected;
                            }
                        }
                        KeyCode::Enter => app.confirm_user_packages(),
                        _ => {}
                    }
                }

                // ---- Disk selection ----
                Step::SelectDisk => {
                    let len = app.disks.len();
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            let mut c = app.disk_cursor;
                            App::list_prev(len, &mut c);
                            app.disk_cursor = c;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let mut c = app.disk_cursor;
                            App::list_next(len, &mut c);
                            app.disk_cursor = c;
                        }
                        KeyCode::Enter => app.confirm_disk(),
                        _ => {}
                    }
                }

                // ---- Partition mode ----
                Step::PartitionModeSelect => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        let mut c = app.partition_mode_cursor;
                        App::list_prev(2, &mut c);
                        app.partition_mode_cursor = c;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let mut c = app.partition_mode_cursor;
                        App::list_next(2, &mut c);
                        app.partition_mode_cursor = c;
                    }
                    KeyCode::Enter => app.confirm_partition_mode(),
                    _ => {}
                },

                // ---- Swap size ----
                Step::SwapSize => match key.code {
                    KeyCode::Enter => app.confirm_swap_size(),
                    KeyCode::Backspace => {
                        app.swap_size_input.pop();
                    }
                    KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.swap_size_input.pop();
                    }
                    KeyCode::Char(c)
                        if c.is_ascii_digit()
                            && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        app.swap_size_input.push(c)
                    }
                    _ => {}
                },

                // ---- Custom partition: mount point ----
                Step::CustomPartitionMount => match key.code {
                    KeyCode::Enter => app.confirm_custom_mount(),
                    KeyCode::Backspace => {
                        app.part_mount_input.pop();
                    }
                    KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.part_mount_input.pop();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.part_mount_input.push(c)
                    }
                    _ => {}
                },

                // ---- Custom partition: size ----
                Step::CustomPartitionSize => match key.code {
                    KeyCode::Enter => app.confirm_custom_size(),
                    KeyCode::Backspace => {
                        app.part_size_input.pop();
                    }
                    KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.part_size_input.pop();
                    }
                    KeyCode::Char(c)
                        if (c.is_ascii_digit() || c == '.')
                            && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        app.part_size_input.push(c)
                    }
                    _ => {}
                },

                // ---- Custom partition: filesystem type ----
                Step::CustomPartitionFs => {
                    let len = FsType::all().len();
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            let mut c = app.part_fs_cursor;
                            App::list_prev(len, &mut c);
                            app.part_fs_cursor = c;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let mut c = app.part_fs_cursor;
                            App::list_next(len, &mut c);
                            app.part_fs_cursor = c;
                        }
                        KeyCode::Enter => app.confirm_custom_fs(),
                        _ => {}
                    }
                }

                // ---- Add another partition? ----
                Step::CustomPartitionAnother => match key.code {
                    KeyCode::Left | KeyCode::Char('h') => app.another_partition_cursor = 0,
                    KeyCode::Right | KeyCode::Char('l') => app.another_partition_cursor = 1,
                    KeyCode::Enter => app.confirm_custom_another(),
                    _ => {}
                },

                // ---- Confirm ----
                Step::Confirm => match key.code {
                    KeyCode::Left | KeyCode::Char('h') => app.confirm_cursor = 0,
                    KeyCode::Right | KeyCode::Char('l') => app.confirm_cursor = 1,
                    KeyCode::Char(' ') => {
                        app.accept_flake_config = !app.accept_flake_config;
                    }
                    KeyCode::Enter => app.confirm_install(),
                    _ => {}
                },

                // ---- Installing (wait) ----
                Step::Installing => {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.auto_scroll = false;
                            if app.log_scroll > 0 {
                                app.log_scroll -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let max = app.install_log.len().saturating_sub(1);
                            if app.log_scroll < max {
                                app.log_scroll += 1;
                            }
                            // Re-enable auto-scroll if user scrolled to bottom
                            if app.log_scroll >= max {
                                app.auto_scroll = true;
                            }
                        }
                        KeyCode::Enter => {
                            if app.install_done {
                                app.step = Step::RootPassword;
                            } else if app.install_error.is_some() {
                                app.should_quit = true;
                            }
                        }
                        _ => {}
                    }
                }

                // ---- Root password ----
                Step::RootPassword => match key.code {
                    KeyCode::Enter => app.confirm_root_password(),
                    KeyCode::Backspace => {
                        app.root_password.pop();
                    }
                    KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.root_password.pop();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.root_password.push(c)
                    }
                    _ => {}
                },

                // ---- Root password confirm ----
                Step::RootPasswordConfirm => match key.code {
                    KeyCode::Enter => app.confirm_root_password_confirm(),
                    KeyCode::Backspace => {
                        app.root_password_confirm.pop();
                    }
                    KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.root_password_confirm.pop();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.root_password_confirm.push(c)
                    }
                    _ => {}
                },

                // ---- User password (post-install) ----
                Step::UserPassword => match key.code {
                    KeyCode::Enter => app.confirm_user_password(),
                    KeyCode::Backspace => {
                        app.current_password.pop();
                    }
                    KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.current_password.pop();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.current_password.push(c)
                    }
                    _ => {}
                },

                // ---- User password confirm (post-install) ----
                Step::UserPasswordConfirm => match key.code {
                    KeyCode::Enter => app.confirm_user_password_confirm(),
                    KeyCode::Backspace => {
                        app.current_password_confirm.pop();
                    }
                    KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.current_password_confirm.pop();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.current_password_confirm.push(c)
                    }
                    _ => {}
                },

                // ---- Complete ----
                Step::Complete => match key.code {
                    KeyCode::Left | KeyCode::Char('h') => app.reboot_cursor = 0,
                    KeyCode::Right | KeyCode::Char('l') => app.reboot_cursor = 1,
                    KeyCode::Enter => app.confirm_reboot(),
                    _ => {}
                },
            }
        }
    }

    Ok(())
}
