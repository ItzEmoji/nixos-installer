use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::theme::ThemeName;

/// Default path for the system-wide installer configuration.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/nixos-installer/config.toml";

/// Custom theme color overrides defined inline in config.toml.
/// Each field is an RGB hex string like "#89b4fa" or "89b4fa".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomThemeConfig {
    pub accent: Option<String>,
    pub accent_dim: Option<String>,
    pub bg: Option<String>,
    pub surface: Option<String>,
    pub text: Option<String>,
    pub text_dim: Option<String>,
    pub red: Option<String>,
    pub green: Option<String>,
    pub yellow: Option<String>,
}

impl CustomThemeConfig {
    /// Returns true if at least one color is set.
    pub fn has_overrides(&self) -> bool {
        self.accent.is_some()
            || self.accent_dim.is_some()
            || self.bg.is_some()
            || self.surface.is_some()
            || self.text.is_some()
            || self.text_dim.is_some()
            || self.red.is_some()
            || self.green.is_some()
            || self.yellow.is_some()
    }
}

/// Parse an RGB hex color string like "#89b4fa" or "89b4fa" into (r, g, b).
pub fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Installer-level configuration (lives at /etc/nixos-installer/config.toml or a custom path).
/// This is the config the user edits via `--init` and loads via `--config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallerConfig {
    /// The git repository URL to clone (overrides the built-in default).
    pub repo_url: Option<String>,

    /// Color theme name (e.g. "catppuccin-mocha", "nord", "dracula", "tokyo-night", "gruvbox").
    pub theme: Option<ThemeName>,

    /// Custom theme color overrides. Applied on top of the selected base theme.
    /// Allows partial overrides — only set the colors you want to change.
    pub theme_custom: Option<CustomThemeConfig>,

    /// Home Manager base modules that are always included (never shown in selection).
    /// These are referenced as `self.homeManagerModules.<name>` in the generated nix.
    pub hm_base_modules: Vec<String>,

    // ---- Defaults (pre-fill TUI fields) ----

    /// Default hostname to pre-fill in the hostname input.
    pub default_hostname: Option<String>,

    /// Default username to pre-fill when creating the first user.
    pub default_username: Option<String>,

    /// Default swap size in GiB (pre-fills the swap size input for full-disk mode).
    pub default_swap_size: Option<String>,

    // ---- Branding ----

    /// Custom title shown in the TUI header. Defaults to "NixOS Installer".
    pub branding_title: Option<String>,

    // ---- Install hooks ----

    /// Scripts to run before nixos-install (after partitioning and config generation).
    /// Each entry is a path to an executable script.
    pub pre_install_hooks: Vec<String>,

    /// Scripts to run after nixos-install completes (before password setup).
    /// Each entry is a path to an executable script.
    pub post_install_hooks: Vec<String>,
}

impl Default for InstallerConfig {
    fn default() -> Self {
        Self {
            repo_url: None,
            theme: None,
            theme_custom: None,
            hm_base_modules: Vec::new(),
            default_hostname: None,
            default_username: None,
            default_swap_size: None,
            branding_title: None,
            pre_install_hooks: Vec::new(),
            post_install_hooks: Vec::new(),
        }
    }
}

/// Load the installer config from a given path.
/// Returns the default config if the file doesn't exist or can't be parsed.
pub fn load_config(path: &Path) -> InstallerConfig {
    match std::fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<InstallerConfig>(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
                InstallerConfig::default()
            }
        },
        Err(_) => InstallerConfig::default(),
    }
}

/// Load a repo-level config.toml from the repository root.
/// This merges only the repo-level fields into an existing config.
pub fn load_repo_config(base_path: &Path, existing: &InstallerConfig) -> InstallerConfig {
    let config_path = base_path.join("config.toml");
    match std::fs::read_to_string(&config_path) {
        Ok(content) => match toml::from_str::<InstallerConfig>(&content) {
            Ok(repo_cfg) => {
                let mut merged = existing.clone();
                // Repo-level hm_base_modules overrides if non-empty
                if !repo_cfg.hm_base_modules.is_empty() {
                    merged.hm_base_modules = repo_cfg.hm_base_modules;
                }
                // Repo-level repo_url / theme can also override if set
                if repo_cfg.repo_url.is_some() {
                    merged.repo_url = repo_cfg.repo_url;
                }
                if repo_cfg.theme.is_some() {
                    merged.theme = repo_cfg.theme;
                }
                // Repo-level theme_custom overrides if present
                if let Some(tc) = repo_cfg.theme_custom {
                    if tc.has_overrides() {
                        merged.theme_custom = Some(tc);
                    }
                }
                // Repo-level defaults override if set
                if repo_cfg.default_hostname.is_some() {
                    merged.default_hostname = repo_cfg.default_hostname;
                }
                if repo_cfg.default_username.is_some() {
                    merged.default_username = repo_cfg.default_username;
                }
                if repo_cfg.default_swap_size.is_some() {
                    merged.default_swap_size = repo_cfg.default_swap_size;
                }
                if repo_cfg.branding_title.is_some() {
                    merged.branding_title = repo_cfg.branding_title;
                }
                // Repo-level hooks override if non-empty
                if !repo_cfg.pre_install_hooks.is_empty() {
                    merged.pre_install_hooks = repo_cfg.pre_install_hooks;
                }
                if !repo_cfg.post_install_hooks.is_empty() {
                    merged.post_install_hooks = repo_cfg.post_install_hooks;
                }
                merged
            }
            Err(e) => {
                eprintln!("Warning: Failed to parse repo config.toml: {}", e);
                existing.clone()
            }
        },
        Err(_) => existing.clone(),
    }
}

/// Generate the default config.toml content for `--init`.
pub fn generate_default_config() -> String {
    let available = ThemeName::all_names().join(", ");
    format!(
        r##"# NixOS Installer Configuration
# Generated by nixos-installer --init

# Git repository URL for the NixOS dotfiles/flake to install from.
# If not set, the built-in default is used.
# repo_url = "https://github.com/ItzEmoji/nixos-dotfiles.git"

# Color theme for the installer TUI.
# Available themes: {available}
# theme = "catppuccin-mocha"

# Home Manager base modules that are always included for every user
# (never shown in the selection screen).
# hm_base_modules = ["home"]

# ---- Branding ----

# Custom title displayed in the installer header.
# Defaults to "NixOS Installer" if not set.
# branding_title = "MyOrg NixOS Installer"

# ---- Defaults ----
# Pre-fill TUI fields with these values. The user can still change them.

# Default hostname for the system.
# default_hostname = "nixos-desktop"

# Default username for the first user.
# default_username = "admin"

# Default swap size in GiB (for full-disk partitioning mode).
# default_swap_size = "4"

# ---- Install Hooks ----
# Scripts to run at specific points during installation.
# Each entry is a path to an executable script.
# The scripts receive environment variables:
#   INSTALLER_HOST_NAME    - the configured hostname
#   INSTALLER_BASE_PATH    - path to the cloned/local repo
#   INSTALLER_DISK         - selected disk path (e.g. /dev/sda)
#   INSTALLER_MOUNT_ROOT   - mount root (/mnt)

# Scripts to run before nixos-install (after partitioning + config generation).
# pre_install_hooks = ["/etc/nixos-installer/hooks/pre-install.sh"]

# Scripts to run after nixos-install completes (before password setup).
# post_install_hooks = ["/etc/nixos-installer/hooks/post-install.sh"]

# ---- Custom Theme Colors ----
# Override individual colors of the selected base theme.
# Colors are RGB hex values (with or without '#' prefix).
# Only set the colors you want to change — the rest come from the base theme.

# [theme_custom]
# accent = "#89b4fa"
# accent_dim = "#585b70"
# bg = "#1e1e2e"
# surface = "#313244"
# text = "#cdd6f4"
# text_dim = "#9399b2"
# red = "#f38ba8"
# green = "#a6e3a1"
# yellow = "#f9e2af"
"##,
        available = available
    )
}

/// Write the default config to /etc/nixos-installer/config.toml (or a custom path).
/// Creates the directory if it doesn't exist.
pub fn init_config(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    let content = generate_default_config();
    std::fs::write(path, &content)
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

    Ok(())
}
