use std::fs::OpenOptions;
use std::io::Write;
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::config::{self, InstallerConfig};
use crate::disk::{self, BlockDevice, CloneState, FsType, PartitionPlan};
use crate::nix::{self, HostPreset, NixModule};
use crate::theme::Theme;

/// Persistent log file path for debugging installation failures.
pub const LOG_FILE: &str = "/tmp/nixos-installer.log";

/// User being created during the wizard.
#[derive(Debug, Clone)]
pub struct UserEntry {
    pub username: String,
    pub password: String,
    pub hm_modules: Vec<NixModule>,
    pub package_modules: Vec<NixModule>,
    pub needs_hm_selection: bool,
}

/// Partition mode choice.
#[derive(Debug, Clone, PartialEq)]
pub enum PartitionMode {
    FullDisk,
    Custom,
}

/// Shared state between the installation background thread and the UI.
#[derive(Debug, Clone)]
pub struct InstallState {
    pub log: Vec<String>,
    pub progress: usize,
    pub total: usize,
    pub error: Option<String>,
    pub done: bool,
}

/// All the wizard steps.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    CloningRepo,
    SelectPreset,
    HostName,
    SelectNixosModules,
    SelectSystemPackages,
    CreateUser,
    AddAnotherUser,
    SelectHmModules,
    SelectUserPackages,
    SelectDisk,
    PartitionModeSelect,
    SwapSize,
    CustomPartitionMount,
    CustomPartitionSize,
    CustomPartitionFs,
    CustomPartitionAnother,
    Confirm,
    Installing,
    RootPassword,
    RootPasswordConfirm,
    UserPassword,
    UserPasswordConfirm,
    Complete,
}

/// Application state.
pub struct App {
    pub step: Step,
    pub should_quit: bool,
    pub base_path: PathBuf,

    // Repository cloning
    pub repo_url: Option<String>,
    pub clone_log: Vec<String>,
    pub clone_phase: String,
    pub clone_percent: u8,
    pub clone_error: Option<String>,
    pub clone_done: bool,
    pub clone_log_scroll: usize,
    pub shared_clone: Option<Arc<Mutex<CloneState>>>,

    // Preset selection
    pub presets: Vec<HostPreset>,
    pub preset_cursor: usize,
    pub is_custom: bool,

    // Host configuration
    pub host_name: String,
    pub host_name_input: String,

    // NixOS module selection (filtered: no home-manager, wsl, home-*)
    pub nixos_modules: Vec<NixModule>,
    pub nixos_cursor: usize,

    // Package set selection (from modules/packages/) — system-level
    pub system_packages: Vec<NixModule>,
    pub system_package_cursor: usize,

    // User management
    pub users: Vec<UserEntry>,
    pub current_username: String,
    pub current_password: String,
    pub current_password_confirm: String,
    pub password_mismatch: bool,

    // HM module selection (iterating through users; filtered: no home, home-wsl, packages-*)
    pub hm_user_index: usize,
    pub hm_modules: Vec<NixModule>,
    pub hm_cursor: usize,

    // Per-user package selection (iterating through users after HM modules)
    pub user_pkg_modules: Vec<NixModule>,
    pub user_pkg_cursor: usize,

    // Disk selection
    pub disks: Vec<BlockDevice>,
    pub disk_cursor: usize,
    pub selected_disk: Option<BlockDevice>,

    // Partitioning
    pub partition_mode: PartitionMode,
    pub partition_mode_cursor: usize,
    pub swap_size_input: String,
    pub partitions: Vec<PartitionPlan>,

    // Custom partition entry
    pub part_mount_input: String,
    pub part_size_input: String,
    pub part_fs_cursor: usize,

    // Confirm
    pub confirm_cursor: usize,
    pub accept_flake_config: bool,

    // Root password
    pub root_password: String,
    pub root_password_confirm: String,
    pub root_password_mismatch: bool,

    // Post-install user password collection
    pub password_user_index: usize,

    // Add another user / partition prompt
    pub another_user_cursor: usize,
    pub another_partition_cursor: usize,

    // Installation
    pub install_log: Vec<String>,
    pub install_progress: usize,
    pub install_total: usize,
    pub install_error: Option<String>,
    pub install_done: bool,
    pub log_scroll: usize,
    pub auto_scroll: bool,
    pub shared_install: Option<Arc<Mutex<InstallState>>>,

    // Complete
    pub reboot_cursor: usize,

    // Status / error display
    pub status_message: Option<String>,

    // Installer configuration (from config.toml)
    pub config: InstallerConfig,

    // Active color theme
    pub theme: Theme,

    // Branding title from config (used in header)
    pub branding_title: String,
}

impl App {
    pub fn new(
        base_path: Option<PathBuf>,
        repo_url: Option<String>,
        installer_config: InstallerConfig,
        theme: Theme,
    ) -> Self {
        // If we already have a local base path, scan immediately.
        // Otherwise, start with CloningRepo step.
        let (step, base_path, presets, nixos_modules, package_modules, status, needs_clone, cfg) =
            if let Some(bp) = base_path {
                let warnings = nix::validate_base_path(&bp);
                let status = if warnings.is_empty() {
                    None
                } else {
                    Some(warnings.join("\n"))
                };
                let cfg = config::load_repo_config(&bp, &installer_config);
                let presets = nix::scan_host_presets(&bp);
                let nixos_modules = nix::scan_nixos_modules(&bp);
                let package_modules = nix::scan_package_modules(&bp);
                (Step::SelectPreset, bp, presets, nixos_modules, package_modules, status, false, cfg)
            } else {
                // Will clone into /tmp/nixos-dotfiles
                let bp = PathBuf::from("/tmp/nixos-dotfiles");
                (
                    Step::CloningRepo,
                    bp,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    None,
                    true,
                    installer_config,
                )
            };

        let branding = cfg.branding_title.clone()
            .unwrap_or_else(|| "NixOS Installer".to_string());

        let mut app = Self {
            step,
            should_quit: false,
            base_path,

            repo_url,
            clone_log: Vec::new(),
            clone_phase: String::new(),
            clone_percent: 0,
            clone_error: None,
            clone_done: false,
            clone_log_scroll: 0,
            shared_clone: None,

            presets,
            preset_cursor: 0,
            is_custom: false,

            host_name: String::new(),
            host_name_input: cfg.default_hostname.clone().unwrap_or_default(),

            nixos_modules,
            nixos_cursor: 0,

            system_packages: package_modules,
            system_package_cursor: 0,

            users: Vec::new(),
            current_username: String::new(),
            current_password: String::new(),
            current_password_confirm: String::new(),
            password_mismatch: false,

            hm_user_index: 0,
            hm_modules: Vec::new(),
            hm_cursor: 0,

            user_pkg_modules: Vec::new(),
            user_pkg_cursor: 0,

            disks: Vec::new(),
            disk_cursor: 0,
            selected_disk: None,

            partition_mode: PartitionMode::FullDisk,
            partition_mode_cursor: 0,
            swap_size_input: cfg.default_swap_size.clone().unwrap_or_else(|| "4".to_string()),
            partitions: Vec::new(),

            part_mount_input: String::new(),
            part_size_input: String::new(),
            part_fs_cursor: 0,

            confirm_cursor: 0,
            accept_flake_config: true,

            root_password: String::new(),
            root_password_confirm: String::new(),
            root_password_mismatch: false,

            password_user_index: 0,

            another_user_cursor: 0,
            another_partition_cursor: 0,

            install_log: Vec::new(),
            install_progress: 0,
            install_total: 8,
            install_error: None,
            install_done: false,
            log_scroll: 0,
            auto_scroll: true,
            shared_install: None,

            reboot_cursor: 0,

            status_message: status,

            config: cfg,

            theme,

            branding_title: branding,
        };

        // If we need to clone, start the background clone thread
        if needs_clone {
            app.start_clone();
        }

        app
    }

    /// Get the display names for the preset list (including "Custom" at the end).
    pub fn preset_display_items(&self) -> Vec<String> {
        let mut items: Vec<String> = self
            .presets
            .iter()
            .map(|p| p.name.clone())
            .collect();
        items.push("Custom".to_string());
        items
    }

    // ---- Navigation helpers ----

    pub fn list_next(len: usize, cursor: &mut usize) {
        if len == 0 {
            return;
        }
        *cursor = (*cursor + 1) % len;
    }

    pub fn list_prev(len: usize, cursor: &mut usize) {
        if len == 0 {
            return;
        }
        *cursor = if *cursor == 0 { len - 1 } else { *cursor - 1 };
    }

    // ---- Clone management ----

    /// Start cloning the dotfiles repository in a background thread.
    fn start_clone(&mut self) {
        let state = Arc::new(Mutex::new(CloneState {
            log: Vec::new(),
            phase: String::new(),
            percent: 0,
            error: None,
            done: false,
        }));
        self.shared_clone = Some(Arc::clone(&state));

        let url = self.repo_url.clone().unwrap_or_default();
        let dest = self.base_path.clone();

        // Clean up any previous clone at the destination
        if dest.exists() {
            let _ = std::fs::remove_dir_all(&dest);
        }

        std::thread::spawn(move || {
            disk::clone_repo(&url, &dest, state);
        });
    }

    /// Copy state from the background clone thread into App fields.
    pub fn sync_clone_state(&mut self) {
        if let Some(shared) = &self.shared_clone {
            match shared.lock() {
                Ok(s) => {
                    self.clone_log = s.log.clone();
                    self.clone_phase = s.phase.clone();
                    self.clone_percent = s.percent;
                    self.clone_error = s.error.clone();
                    self.clone_done = s.done;
                }
                Err(_) => {
                    // Mutex poisoned — the clone thread panicked
                    self.clone_error =
                        Some("Clone thread crashed unexpectedly".to_string());
                    self.clone_done = true;
                }
            }
        }

        // Auto-scroll the clone log
        if self.auto_scroll && !self.clone_log.is_empty() {
            self.clone_log_scroll = self.clone_log.len().saturating_sub(1);
        }
    }

    /// Called when clone is done: scan modules and advance to SelectPreset.
    pub fn finish_clone(&mut self) {
        // Validate and scan the freshly cloned repo
        let warnings = nix::validate_base_path(&self.base_path);
        if !warnings.is_empty() {
            self.status_message = Some(warnings.join("\n"));
        }

        self.config = config::load_repo_config(&self.base_path, &self.config);
        self.presets = nix::scan_host_presets(&self.base_path);
        self.nixos_modules = nix::scan_nixos_modules(&self.base_path);
        self.system_packages = nix::scan_package_modules(&self.base_path);

        // Apply repo-level config defaults that weren't set at startup
        if self.host_name_input.is_empty() {
            if let Some(ref h) = self.config.default_hostname {
                self.host_name_input = h.clone();
            }
        }
        if let Some(ref s) = self.config.default_swap_size {
            self.swap_size_input = s.clone();
        }
        if let Some(ref t) = self.config.branding_title {
            self.branding_title = t.clone();
        }

        self.step = Step::SelectPreset;
    }

    // ---- Go-back navigation ----

    /// Go back to the previous logical step when the user presses Esc.
    /// Returns `true` if we went back, `false` if there is no previous step.
    pub fn go_back(&mut self) -> bool {
        match self.step {
            // First step — can't go back
            Step::CloningRepo | Step::SelectPreset => false,

            Step::HostName => {
                self.step = Step::SelectPreset;
                true
            }
            Step::SelectNixosModules => {
                self.step = Step::HostName;
                true
            }
            Step::SelectSystemPackages => {
                self.step = Step::SelectNixosModules;
                true
            }
            Step::CreateUser => {
                if self.is_custom {
                    self.step = Step::SelectSystemPackages;
                } else {
                    self.step = Step::SelectPreset;
                }
                true
            }

            // After a user is committed, going back is complex (would need to
            // undo the push). Let Esc quit instead.
            Step::AddAnotherUser => false,
            Step::SelectHmModules => false,
            Step::SelectUserPackages => false,

            Step::SelectDisk => {
                // Go back to the step before disk selection.
                // If any user needed HM selection we'd go back there, but
                // re-entering HM selection is messy, so go to AddAnotherUser.
                // Simpler: just don't go back from here (q to quit).
                false
            }
            Step::PartitionModeSelect => {
                self.step = Step::SelectDisk;
                true
            }
            Step::SwapSize => {
                self.step = Step::PartitionModeSelect;
                true
            }
            Step::CustomPartitionMount => {
                if self.partitions.is_empty() {
                    // First partition — go back to mode select
                    self.step = Step::PartitionModeSelect;
                } else {
                    // Subsequent partition — undo the "yes, add another" choice
                    self.step = Step::CustomPartitionAnother;
                }
                true
            }
            Step::CustomPartitionSize => {
                self.step = Step::CustomPartitionMount;
                true
            }
            Step::CustomPartitionFs => {
                self.step = Step::CustomPartitionSize;
                true
            }
            Step::CustomPartitionAnother => false,

            Step::Confirm => {
                self.step = Step::PartitionModeSelect;
                true
            }

            // Can't go back from active installation or post-install steps
            Step::Installing | Step::RootPassword | Step::RootPasswordConfirm
            | Step::UserPassword | Step::UserPasswordConfirm | Step::Complete => {
                false
            }
        }
    }

    // ---- Step transitions ----

    pub fn confirm_preset_selection(&mut self) {
        let items = self.preset_display_items();
        if self.preset_cursor >= items.len() {
            return;
        }

        if self.preset_cursor == items.len() - 1 {
            // "Custom" selected
            self.is_custom = true;
            self.step = Step::HostName;
        } else {
            // Existing preset
            self.is_custom = false;
            self.host_name = self.presets[self.preset_cursor].name.clone();
            self.prefill_username_if_empty();
            self.step = Step::CreateUser;
        }
    }

    pub fn confirm_host_name(&mut self) {
        let name = self.host_name_input.trim().to_string();
        if name.is_empty() {
            self.status_message = Some("Host name cannot be empty".to_string());
            return;
        }
        self.host_name = name;
        self.step = Step::SelectNixosModules;
        self.status_message = None;
    }

    pub fn confirm_nixos_modules(&mut self) {
        self.step = Step::SelectSystemPackages;
    }

    pub fn confirm_system_packages(&mut self) {
        self.prefill_username_if_empty();
        self.step = Step::CreateUser;
    }

    /// Pre-fill the username input from config if the user list is empty
    /// and a default_username is configured.
    fn prefill_username_if_empty(&mut self) {
        if self.users.is_empty() && self.current_username.is_empty() {
            if let Some(ref name) = self.config.default_username {
                self.current_username = name.clone();
            }
        }
    }

    pub fn confirm_username(&mut self) {
        let name = self.current_username.trim().to_string();
        if name.is_empty() {
            self.status_message = Some("Username cannot be empty".to_string());
            return;
        }
        // Validate: lowercase alphanumeric, underscores, hyphens
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
            || !name
                .chars()
                .next()
                .map_or(false, |c| c.is_ascii_lowercase() || c == '_')
        {
            self.status_message = Some(
                "Username must start with a lowercase letter or underscore".to_string(),
            );
            return;
        }
        // Check for duplicate
        if self.users.iter().any(|u| u.username == name) {
            self.status_message = Some("User already exists".to_string());
            return;
        }
        self.status_message = None;

        // Check if user config already exists
        let needs_hm = !nix::user_config_exists(
            &self.base_path,
            &self.host_name,
            &name,
        );

        self.users.push(UserEntry {
            username: name,
            password: String::new(),
            hm_modules: Vec::new(),
            package_modules: Vec::new(),
            needs_hm_selection: needs_hm,
        });

        self.current_username.clear();
        self.step = Step::AddAnotherUser;
    }

    pub fn confirm_user_password(&mut self) {
        if self.current_password.is_empty() {
            self.status_message = Some("Password cannot be empty".to_string());
            return;
        }
        self.status_message = None;
        self.step = Step::UserPasswordConfirm;
    }

    pub fn confirm_user_password_confirm(&mut self) {
        if self.current_password != self.current_password_confirm {
            self.password_mismatch = true;
            self.current_password.clear();
            self.current_password_confirm.clear();
            self.step = Step::UserPassword;
            return;
        }
        self.password_mismatch = false;

        // Set the password for this user via nixos-enter
        let username = self.users[self.password_user_index].username.clone();
        self.log_install(&format!("Setting password for user '{}'...", username));
        if let Err(e) = disk::set_user_password_in_target(&username, &self.current_password) {
            self.status_message = Some(format!(
                "Failed to set password for '{}': {}. Press any key to retry.",
                username, e
            ));
            self.current_password.clear();
            self.current_password_confirm.clear();
            // Stay on this user — retry
            self.step = Step::UserPassword;
            return;
        }

        // Store the password (in case it's needed later) and advance
        self.users[self.password_user_index].password = self.current_password.clone();
        self.current_password.clear();
        self.current_password_confirm.clear();

        // Move to next user
        self.password_user_index += 1;
        self.advance_to_next_user_password();
    }

    /// After root password is set, begin collecting passwords for each user.
    fn begin_user_password_collection(&mut self) {
        self.password_user_index = 0;
        self.advance_to_next_user_password();
    }

    /// Advance to the next user that needs a password, or go to Complete.
    fn advance_to_next_user_password(&mut self) {
        if self.password_user_index < self.users.len() {
            self.current_password.clear();
            self.current_password_confirm.clear();
            self.password_mismatch = false;
            self.step = Step::UserPassword;
        } else {
            self.step = Step::Complete;
        }
    }

    pub fn confirm_another_user(&mut self) {
        if self.another_user_cursor == 0 {
            // Yes - create another user
            self.step = Step::CreateUser;
        } else {
            // No - move to HM module selection for users that need it
            self.begin_hm_selection();
        }
        self.another_user_cursor = 0;
    }

    fn begin_hm_selection(&mut self) {
        self.hm_user_index = 0;
        self.advance_to_next_hm_user();
    }

    fn advance_to_next_hm_user(&mut self) {
        // Find the next user that needs HM selection
        while self.hm_user_index < self.users.len() {
            if self.users[self.hm_user_index].needs_hm_selection {
                // Scan HM modules and package modules on demand
                self.users[self.hm_user_index].hm_modules =
                    nix::scan_hm_modules(&self.base_path);
                self.users[self.hm_user_index].package_modules =
                    nix::scan_package_modules(&self.base_path);
                // Load their HM modules for selection
                self.hm_modules = self.users[self.hm_user_index].hm_modules.clone();
                self.hm_cursor = 0;
                self.step = Step::SelectHmModules;
                return;
            }
            self.hm_user_index += 1;
        }

        // No more users need HM selection, move to disk selection
        self.go_to_disk_selection();
    }

    fn go_to_disk_selection(&mut self) {
        match disk::list_block_devices() {
            Ok(disks) => self.disks = disks,
            Err(e) => {
                self.disks = Vec::new();
                self.status_message = Some(format!("Failed to list disks: {}", e));
            }
        }
        self.disk_cursor = 0;
        self.step = Step::SelectDisk;
    }

    pub fn confirm_hm_modules(&mut self) {
        // Save HM selections back to the user
        self.users[self.hm_user_index].hm_modules = self.hm_modules.clone();
        // Now transition to per-user package selection
        self.user_pkg_modules = self.users[self.hm_user_index].package_modules.clone();
        self.user_pkg_cursor = 0;
        self.step = Step::SelectUserPackages;
    }

    pub fn confirm_user_packages(&mut self) {
        // Save per-user package selections back to the user
        self.users[self.hm_user_index].package_modules = self.user_pkg_modules.clone();
        self.hm_user_index += 1;
        self.advance_to_next_hm_user();
    }

    pub fn confirm_disk(&mut self) {
        if self.disks.is_empty() {
            self.status_message = Some("No disks available".to_string());
            return;
        }
        self.selected_disk = Some(self.disks[self.disk_cursor].clone());
        self.status_message = None;
        self.step = Step::PartitionModeSelect;
    }

    pub fn confirm_partition_mode(&mut self) {
        if self.partition_mode_cursor == 0 {
            self.partition_mode = PartitionMode::FullDisk;
            self.step = Step::SwapSize;
        } else {
            self.partition_mode = PartitionMode::Custom;
            self.partitions.clear();
            self.step = Step::CustomPartitionMount;
        }
    }

    pub fn confirm_swap_size(&mut self) {
        let input = self.swap_size_input.trim();
        let swap_gb: u64 = if input.is_empty() {
            0
        } else {
            match input.parse::<u64>() {
                Ok(v) => v,
                Err(_) => {
                    self.status_message =
                        Some("Invalid swap size. Enter a whole number in GiB (e.g. 4) or leave empty for no swap.".to_string());
                    return;
                }
            }
        };

        // Build full-disk partition plan: EFI (512M) + swap + root (rest)
        self.partitions.clear();

        self.partitions.push(PartitionPlan {
            label: "EFI".to_string(),
            mount_point: "/boot".to_string(),
            size_mb: Some(512),
            fs_type: FsType::Fat32,
        });

        if swap_gb > 0 {
            self.partitions.push(PartitionPlan {
                label: "swap".to_string(),
                mount_point: "swap".to_string(),
                size_mb: Some(swap_gb * 1024),
                fs_type: FsType::Swap,
            });
        }

        self.partitions.push(PartitionPlan {
            label: "root".to_string(),
            mount_point: "/".to_string(),
            size_mb: None, // use remaining space
            fs_type: FsType::Ext4,
        });

        self.step = Step::Confirm;
    }

    pub fn confirm_custom_mount(&mut self) {
        let mount = self.part_mount_input.trim().to_string();
        if mount.is_empty() {
            self.status_message = Some("Mount point cannot be empty".to_string());
            return;
        }
        if mount != "swap" && !mount.starts_with('/') {
            self.status_message =
                Some("Mount point must start with '/' or be 'swap'".to_string());
            return;
        }
        self.status_message = None;
        self.step = Step::CustomPartitionSize;
    }

    pub fn confirm_custom_size(&mut self) {
        self.status_message = None;
        self.step = Step::CustomPartitionFs;
    }

    pub fn confirm_custom_fs(&mut self) {
        let fs_types = FsType::all();
        let fs = fs_types[self.part_fs_cursor].clone();

        let mount = self.part_mount_input.trim().to_string();
        let size_mb: Option<u64> = if self.part_size_input.trim().is_empty() {
            None
        } else {
            match self.part_size_input.trim().parse::<u64>() {
                Ok(v) if v > 0 => Some(v * 1024),
                Ok(_) => {
                    self.status_message =
                        Some("Size must be greater than 0.".to_string());
                    return;
                }
                Err(_) => {
                    self.status_message = Some(
                        "Invalid size. Enter a whole number in GiB or leave empty for remaining space."
                            .to_string(),
                    );
                    return;
                }
            }
        };

        let label = if mount == "/" {
            "root".to_string()
        } else if mount == "/boot" {
            "EFI".to_string()
        } else if mount == "swap" {
            "swap".to_string()
        } else {
            mount.trim_start_matches('/').replace('/', "-")
        };

        self.partitions.push(PartitionPlan {
            label,
            mount_point: mount,
            size_mb,
            fs_type: fs,
        });

        self.part_mount_input.clear();
        self.part_size_input.clear();
        self.part_fs_cursor = 0;

        self.step = Step::CustomPartitionAnother;
    }

    pub fn confirm_custom_another(&mut self) {
        if self.another_partition_cursor == 0 {
            self.step = Step::CustomPartitionMount;
        } else {
            self.step = Step::Confirm;
        }
        self.another_partition_cursor = 0;
    }

    pub fn confirm_install(&mut self) {
        if self.confirm_cursor == 0 {
            // Validate that there is a root partition
            if !self.partitions.iter().any(|p| p.mount_point == "/") {
                self.status_message = Some(
                    "No root (/) partition defined. Please go back and add one.".to_string(),
                );
                return;
            }
            self.step = Step::Installing;
            self.start_installation();
        } else {
            self.step = Step::PartitionModeSelect;
        }
    }

    pub fn confirm_root_password(&mut self) {
        if self.root_password.is_empty() {
            self.status_message = Some("Root password cannot be empty".to_string());
            return;
        }
        self.status_message = None;
        self.step = Step::RootPasswordConfirm;
    }

    pub fn confirm_root_password_confirm(&mut self) {
        if self.root_password != self.root_password_confirm {
            self.root_password_mismatch = true;
            self.root_password.clear();
            self.root_password_confirm.clear();
            self.step = Step::RootPassword;
            return;
        }
        self.root_password_mismatch = false;

        self.log_install("Setting root password...");
        if let Err(e) = disk::set_root_password(&self.root_password) {
            self.status_message = Some(format!("Failed to set root password: {}. Press any key to retry.", e));
            self.root_password.clear();
            self.root_password_confirm.clear();
            self.step = Step::RootPassword;
            return;
        }

        // Now collect and set passwords for each user
        self.begin_user_password_collection();
    }

    pub fn confirm_reboot(&mut self) {
        if self.reboot_cursor == 0 {
            let _ = disk::reboot();
        }
        self.should_quit = true;
    }

    // ---- Installation logic ----

    fn log_install(&mut self, msg: &str) {
        self.install_log.push(msg.to_string());
        // Also append to persistent log file
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(LOG_FILE)
        {
            let _ = writeln!(f, "{}", msg);
        }
    }

    fn start_installation(&mut self) {
        // Calculate total steps: base 9 + pre-hooks + post-hooks
        let pre_hook_count = self.config.pre_install_hooks.len();
        let post_hook_count = self.config.post_install_hooks.len();
        let total = 9 + pre_hook_count + post_hook_count;

        let state = Arc::new(Mutex::new(InstallState {
            log: Vec::new(),
            progress: 0,
            total,
            error: None,
            done: false,
        }));
        self.shared_install = Some(Arc::clone(&state));

        // Clone everything the background thread needs.
        let disk_path = match &self.selected_disk {
            Some(d) => d.path.clone(),
            None => {
                if let Ok(mut s) = state.lock() {
                    s.error = Some("No disk selected".to_string());
                    s.log.push("ERROR: No disk selected".to_string());
                }
                return;
            }
        };
        let partitions = self.partitions.clone();
        let base_path = self.base_path.clone();
        let host_name = self.host_name.clone();
        let is_custom = self.is_custom;
        let nixos_modules = self.nixos_modules.clone();
        let system_packages = self.system_packages.clone();
        let users = self.users.clone();
        let accept_flake_config = self.accept_flake_config;
        let installer_config = self.config.clone();
        let pre_hooks = self.config.pre_install_hooks.clone();
        let post_hooks = self.config.post_install_hooks.clone();

        std::thread::spawn(move || {
            // Helper: log a message to shared state and the log file.
            let log = |state: &Arc<Mutex<InstallState>>, msg: &str| {
                if let Ok(mut s) = state.lock() {
                    s.log.push(msg.to_string());
                }
                if let Ok(mut f) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(LOG_FILE)
                {
                    let _ = writeln!(f, "{}", msg);
                }
            };

            let log_error = |state: &Arc<Mutex<InstallState>>, msg: &str| {
                for line in msg.lines() {
                    if let Ok(mut s) = state.lock() {
                        s.log.push(format!("ERROR: {}", line));
                    }
                }
                if let Ok(mut f) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(LOG_FILE)
                {
                    let _ = writeln!(f, "ERROR: {}", msg);
                }
            };

            let set_progress = |state: &Arc<Mutex<InstallState>>, p: usize| {
                if let Ok(mut s) = state.lock() {
                    s.progress = p;
                }
            };

            let fail = |state: &Arc<Mutex<InstallState>>, msg: String| {
                if let Ok(mut s) = state.lock() {
                    s.error = Some(msg);
                }
            };

            // Truncate/create the log file
            if let Ok(mut f) = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(LOG_FILE)
            {
                let _ = writeln!(f, "=== NixOS Installer Log ===\n");
            }

            // Step 1: Partition
            log(&state, &format!("Partitioning {}...", disk_path));
            set_progress(&state, 1);
            if let Err(e) = disk::partition_disk(&disk_path, &partitions) {
                let msg = format!("Partitioning failed: {}", e);
                log_error(&state, &msg);
                fail(&state, msg);
                return;
            }

            // Step 2: Format and mount
            log(&state, "Formatting and mounting partitions...");
            set_progress(&state, 2);
            if let Err(e) = disk::format_and_mount(&disk_path, &partitions) {
                let msg = format!("Format/mount failed: {}", e);
                log_error(&state, &msg);
                fail(&state, msg);
                return;
            }

            // Step 3: Generate hardware config
            log(&state, "Generating hardware configuration...");
            set_progress(&state, 3);
            let hw_config = match disk::generate_hardware_config() {
                Ok(c) => c,
                Err(e) => {
                    let msg = format!("Hardware config generation failed: {}", e);
                    log_error(&state, &msg);
                    fail(&state, msg);
                    return;
                }
            };

            // Step 4: Write hardware config
            log(&state, "Writing hardware configuration...");
            set_progress(&state, 4);
            if let Err(e) = nix::write_hardware_config(&base_path, &host_name, &hw_config) {
                let msg = format!("Failed to write hardware config: {}", e);
                log_error(&state, &msg);
                fail(&state, msg);
                return;
            }

            // Step 5: Write host configuration (if custom)
            set_progress(&state, 5);
            if is_custom {
                log(&state, "Writing host configuration...");
                let usernames: Vec<String> = users.iter().map(|u| u.username.clone()).collect();
                let config = nix::generate_configuration_nix(
                    &host_name,
                    &nixos_modules,
                    &system_packages,
                    &usernames,
                );
                if let Err(e) = nix::write_host_config(&base_path, &host_name, &config) {
                    let msg = format!("Failed to write configuration: {}", e);
                    log_error(&state, &msg);
                    fail(&state, msg);
                    return;
                }
            }

            // Step 6: Write user definition files (user + HM imports combined)
            for user in &users {
                log(&state, &format!("Writing user-{}.nix...", user.username));
                let user_nix = nix::generate_user_nix(
                    &host_name,
                    &user.username,
                    &user.hm_modules,
                    &user.package_modules,
                    &installer_config.hm_base_modules,
                );
                if let Err(e) = nix::write_user_config(
                    &base_path,
                    &host_name,
                    &user.username,
                    &user_nix,
                ) {
                    let msg = format!("Failed to write user config: {}", e);
                    log_error(&state, &msg);
                    fail(&state, msg);
                    return;
                }
            }

            // Step 7: Stage generated files so the flake can see them
            log(&state, "Staging generated files (git add)...");
            set_progress(&state, 6);
            if let Err(e) = disk::git_add_all(&base_path) {
                let msg = format!("git add failed: {}", e);
                log_error(&state, &msg);
                fail(&state, msg);
                return;
            }

            // Pre-install hooks
            let mut step_counter = 7;
            for hook in &pre_hooks {
                log(&state, &format!("Running pre-install hook: {}...", hook));
                set_progress(&state, step_counter);
                match disk::run_hook(hook, &host_name, &base_path, &disk_path) {
                    Ok(output) => {
                        for line in output.lines() {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                log(&state, &format!("  [hook] {}", trimmed));
                            }
                        }
                    }
                    Err(e) => {
                        let msg = format!("Pre-install hook failed: {}", e);
                        log_error(&state, &msg);
                        fail(&state, msg);
                        return;
                    }
                }
                step_counter += 1;
            }

            // Step N: Run nixos-install (stream output in real time)
            log(&state, "Running nixos-install (this may take a while)...");
            set_progress(&state, step_counter);
            step_counter += 1;
            let flake_arg = format!("{}#{}", base_path.to_string_lossy(), host_name);
            let mut cmd = std::process::Command::new("nixos-install");
            cmd.args(["--flake", &flake_arg, "--no-root-passwd"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped());
            if accept_flake_config {
                cmd.env("NIX_CONFIG", "accept-flake-config = true");
            }

            match cmd.spawn() {
                Ok(mut child) => {
                    // Stream stderr line-by-line (nixos-install/nix build outputs to stderr)
                    if let Some(stderr) = child.stderr.take() {
                        let reader = std::io::BufReader::new(stderr);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                let trimmed = line.trim().to_string();
                                if !trimmed.is_empty() {
                                    if let Ok(mut s) = state.lock() {
                                        s.log.push(trimmed.clone());
                                    }
                                    if let Ok(mut f) = OpenOptions::new()
                                        .create(true)
                                        .append(true)
                                        .open(LOG_FILE)
                                    {
                                        let _ = writeln!(f, "{}", trimmed);
                                    }
                                }
                            }
                        }
                    }

                    match child.wait() {
                        Ok(status) if status.success() => {}
                        Ok(status) => {
                            let msg = format!(
                                "nixos-install failed with exit code {:?}",
                                status.code()
                            );
                            log_error(&state, &msg);
                            fail(&state, msg);
                            return;
                        }
                        Err(e) => {
                            let msg = format!("Failed to wait for nixos-install: {}", e);
                            log_error(&state, &msg);
                            fail(&state, msg);
                            return;
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("Failed to run nixos-install: {}", e);
                    log_error(&state, &msg);
                    fail(&state, msg);
                    return;
                }
            }

            set_progress(&state, step_counter);
            step_counter += 1;
            log(&state, "Copying repository to /mnt/etc/nixos/...");
            if let Err(e) = disk::copy_repo_to_target(&base_path) {
                let msg = format!("Failed to copy repo to target: {}", e);
                log_error(&state, &msg);
                fail(&state, msg);
                return;
            }

            // Post-install hooks
            for hook in &post_hooks {
                log(&state, &format!("Running post-install hook: {}...", hook));
                set_progress(&state, step_counter);
                match disk::run_hook(hook, &host_name, &base_path, &disk_path) {
                    Ok(output) => {
                        for line in output.lines() {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                log(&state, &format!("  [hook] {}", trimmed));
                            }
                        }
                    }
                    Err(e) => {
                        let msg = format!("Post-install hook failed: {}", e);
                        log_error(&state, &msg);
                        fail(&state, msg);
                        return;
                    }
                }
                step_counter += 1;
            }

            set_progress(&state, step_counter);
            log(&state, "Installation complete!");
            if let Ok(mut s) = state.lock() {
                s.done = true;
            }
        });
    }

    /// Copy state from the background installation thread into App fields.
    /// Called each frame from the event loop during Step::Installing.
    pub fn sync_install_state(&mut self) {
        if let Some(shared) = &self.shared_install {
            match shared.lock() {
                Ok(s) => {
                    self.install_log = s.log.clone();
                    self.install_progress = s.progress;
                    self.install_total = s.total;
                    self.install_error = s.error.clone();
                    self.install_done = s.done;
                }
                Err(_) => {
                    // Mutex poisoned — the install thread panicked
                    self.install_error =
                        Some("Installation thread crashed unexpectedly".to_string());
                }
            }
        }
    }

    /// Get the current step number (1-indexed) for the progress bar.
    pub fn step_number(&self) -> usize {
        match self.step {
            Step::CloningRepo => 1,
            Step::SelectPreset => 2,
            Step::HostName | Step::SelectNixosModules | Step::SelectSystemPackages => 3,
            Step::CreateUser
            | Step::AddAnotherUser => 4,
            Step::SelectHmModules | Step::SelectUserPackages => 5,
            Step::SelectDisk => 6,
            Step::PartitionModeSelect
            | Step::SwapSize
            | Step::CustomPartitionMount
            | Step::CustomPartitionSize
            | Step::CustomPartitionFs
            | Step::CustomPartitionAnother => 7,
            Step::Confirm => 8,
            Step::Installing => 9,
            Step::RootPassword | Step::RootPasswordConfirm => 10,
            Step::UserPassword | Step::UserPasswordConfirm => 11,
            Step::Complete => 12,
        }
    }

    pub fn total_steps(&self) -> usize {
        12
    }

    /// Step title for the header.
    pub fn step_title(&self) -> String {
        match self.step {
            Step::CloningRepo => "Cloning Repository".to_string(),
            Step::SelectPreset => "Select Host Preset".to_string(),
            Step::HostName => "Enter Host Name".to_string(),
            Step::SelectNixosModules => "Select NixOS Modules".to_string(),
            Step::SelectSystemPackages => "Select System Packages".to_string(),
            Step::CreateUser => {
                let n = self.users.len() + 1;
                format!("Create User #{}", n)
            }
            Step::UserPassword => {
                if self.password_user_index < self.users.len() {
                    format!("Set Password for '{}'", self.users[self.password_user_index].username)
                } else {
                    "Set User Password".to_string()
                }
            }
            Step::UserPasswordConfirm => {
                if self.password_user_index < self.users.len() {
                    format!("Confirm Password for '{}'", self.users[self.password_user_index].username)
                } else {
                    "Confirm User Password".to_string()
                }
            }
            Step::AddAnotherUser => "Add Another User?".to_string(),
            Step::SelectHmModules => "Select Home Manager Modules".to_string(),
            Step::SelectUserPackages => "Select User Packages".to_string(),
            Step::SelectDisk => "Select Installation Disk".to_string(),
            Step::PartitionModeSelect => "Partition Mode".to_string(),
            Step::SwapSize => "Swap Size".to_string(),
            Step::CustomPartitionMount => "Partition Mount Point".to_string(),
            Step::CustomPartitionSize => "Partition Size".to_string(),
            Step::CustomPartitionFs => "Partition Filesystem".to_string(),
            Step::CustomPartitionAnother => "Add Another Partition?".to_string(),
            Step::Confirm => "Confirm Installation".to_string(),
            Step::Installing => "Installing NixOS".to_string(),
            Step::RootPassword => "Set Root Password".to_string(),
            Step::RootPasswordConfirm => "Confirm Root Password".to_string(),
            Step::Complete => "Installation Complete".to_string(),
        }
    }

    /// Get an immutable reference to the current text input.
    pub fn current_input_ref(&self) -> Option<&str> {
        match self.step {
            Step::HostName => Some(&self.host_name_input),
            Step::CreateUser => Some(&self.current_username),
            Step::UserPassword => Some(&self.current_password),
            Step::UserPasswordConfirm => Some(&self.current_password_confirm),
            Step::SwapSize => Some(&self.swap_size_input),
            Step::CustomPartitionMount => Some(&self.part_mount_input),
            Step::CustomPartitionSize => Some(&self.part_size_input),
            Step::RootPassword => Some(&self.root_password),
            Step::RootPasswordConfirm => Some(&self.root_password_confirm),
            _ => None,
        }
    }
}
