use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Represents an existing host preset found in ./modules/hosts/.
#[derive(Debug, Clone)]
pub struct HostPreset {
    pub name: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    #[allow(dead_code)]
    pub has_hardware_config: bool,
}

/// Represents a discovered NixOS or Home Manager module.
#[derive(Debug, Clone)]
pub struct NixModule {
    pub name: String,
    pub selected: bool,
}

// ---------------------------------------------------------------------------
// fd-based module discovery
// ---------------------------------------------------------------------------

/// Use `fd` to find all `.nix` files under a directory.
/// Tries bare `fd` first; if unavailable, falls back to `nix-shell -p fd --run`.
/// Returns a list of (module_name, file_path) pairs.
/// The module name is the filename stem (without `.nix`).
/// Duplicates are eliminated (first occurrence wins).
fn discover_nix_files_with_fd(dir: &Path) -> Vec<(String, PathBuf)> {
    if !dir.is_dir() {
        return Vec::new();
    }

    // Try bare `fd` first (fast path if already on PATH).
    let output = Command::new("fd")
        .args(["--type", "f", "--extension", "nix", "--no-ignore", "--absolute-path"])
        .arg(".")
        .arg(dir)
        .output();

    // If bare fd failed (not found / non-zero), fall back to `find` which is
    // always available. The previous nix-shell fallback could take over a
    // minute to download fd, causing the UI to freeze.
    let output = match &output {
        Ok(o) if o.status.success() => output,
        _ => Command::new("find")
            .arg(dir)
            .args(["-type", "f", "-name", "*.nix"])
            .output(),
    };

    let stdout = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let path = PathBuf::from(line);
        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        // Skip default.nix — these represent directory-modules;
        // use the parent directory name instead.
        if name == "default" {
            if let Some(parent_name) = path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) {
                // Only register if we haven't seen this name already
                let pname = parent_name.to_string();
                if seen.insert(pname.clone()) {
                    results.push((pname, path));
                }
            }
            continue;
        }
        if seen.insert(name.clone()) {
            results.push((name, path));
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Filtering patterns
// ---------------------------------------------------------------------------

/// Check whether a NixOS module name should be filtered out from user selection.
fn should_skip_nixos_module(name: &str) -> bool {
    name.starts_with("home-") || name == "wsl"
}

/// Check whether a Home Manager module name should be filtered out from user selection.
fn should_skip_hm_module(name: &str) -> bool {
    name == "home" || name == "home-wsl" || name.starts_with("packages-")
}

// ---------------------------------------------------------------------------
// Base path validation
// ---------------------------------------------------------------------------

/// Validate that the base path contains the expected module directories.
/// Returns a list of warning messages for any missing directories.
pub fn validate_base_path(base_path: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let modules_dir = base_path.join("modules");

    if !modules_dir.is_dir() {
        warnings.push(format!(
            "modules/ directory not found at '{}'. Module scanning will not work.",
            base_path.display()
        ));
        return warnings;
    }

    for subdir in &["nixosModules", "homeManagerModules", "packages", "hosts"] {
        let dir = modules_dir.join(subdir);
        if !dir.is_dir() {
            warnings.push(format!(
                "modules/{} directory not found at '{}'",
                subdir,
                dir.display()
            ));
        }
    }

    warnings
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Scan ./modules/hosts/<name>/* for existing host presets.
/// Each subdirectory under hosts/ is a host preset.
pub fn scan_host_presets(base_path: &Path) -> Vec<HostPreset> {
    let hosts_dir = base_path.join("modules").join("hosts");
    let mut presets = Vec::new();

    match fs::read_dir(&hosts_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // Skip WSL hosts (they are not selectable presets)
                    if name.to_lowercase().contains("wsl") {
                        continue;
                    }
                    let path = entry.path();
                    let has_hw = path.join("_hardware-configuration.nix").exists();
                    presets.push(HostPreset {
                        name,
                        path,
                        has_hardware_config: has_hw,
                    });
                }
            }
        }
        Err(_e) => {
            // Silently skip — the TUI shows a "no modules found" message
        }
    }

    presets.sort_by(|a, b| a.name.cmp(&b.name));
    presets
}

/// Scan ./modules/nixosModules/ for available NixOS modules using `fd`.
/// Each `.nix` file becomes a module (name = file stem).
/// Directories with `default.nix` become modules (name = directory name).
pub fn scan_nixos_modules(base_path: &Path) -> Vec<NixModule> {
    let dir = base_path.join("modules").join("nixosModules");
    scan_modules_in_dir(&dir, should_skip_nixos_module)
}

/// Scan ./modules/homeManagerModules/ for available Home Manager modules using `fd`.
/// Each `.nix` file becomes a module (name = file stem).
/// Directories with `default.nix` become modules (name = directory name).
pub fn scan_hm_modules(base_path: &Path) -> Vec<NixModule> {
    let dir = base_path.join("modules").join("homeManagerModules");
    scan_modules_in_dir(&dir, should_skip_hm_module)
}

/// Scan ./modules/packages/ for available package sets using `fd`.
/// Only `.nix` files are considered (name = file stem).
/// The flake registers these as `packages-<name>`, so we prepend the prefix.
pub fn scan_package_modules(base_path: &Path) -> Vec<NixModule> {
    let dir = base_path.join("modules").join("packages");
    let collected = discover_nix_files_with_fd(&dir);

    let mut modules: Vec<NixModule> = collected
        .into_iter()
        .filter(|(name, _)| !name.to_lowercase().contains("wsl"))
        .map(|(name, _)| NixModule {
            name: format!("packages-{}", name),
            selected: false,
        })
        .collect();

    modules.sort_by(|a, b| a.name.cmp(&b.name));
    modules
}

/// Scan a module directory using `fd` and apply a skip filter.
/// The module name is the filename stem or directory name (for `default.nix`).
/// Duplicate names are skipped (first found wins).
fn scan_modules_in_dir(
    dir: &Path,
    skip_fn: fn(&str) -> bool,
) -> Vec<NixModule> {
    let collected = discover_nix_files_with_fd(dir);

    let mut modules: Vec<NixModule> = collected
        .into_iter()
        .filter(|(name, _)| !skip_fn(name))
        .map(|(name, _)| NixModule {
            name,
            selected: false,
        })
        .collect();

    modules.sort_by(|a, b| a.name.cmp(&b.name));
    modules
}

// ---------------------------------------------------------------------------
// Existence check
// ---------------------------------------------------------------------------

/// Check if a user-<username>.nix already exists for this host.
pub fn user_config_exists(base_path: &Path, host_name: &str, username: &str) -> bool {
    let file = base_path
        .join("modules")
        .join("hosts")
        .join(host_name)
        .join(format!("user-{}.nix", username));
    file.exists()
}

// ---------------------------------------------------------------------------
// Configuration generation (mirrors install.sh generate_host_config)
// ---------------------------------------------------------------------------

/// Helper: produce a module line, commented out if not selected.
fn mod_line(kind: &str, name: &str, selected: bool) -> String {
    if selected {
        format!("      self.{}.{}", kind, name)
    } else {
        format!("      # self.{}.{}", kind, name)
    }
}

/// Generate the configuration.nix for a new custom host.
/// ALL discovered modules are included; unselected ones are commented out.
/// Uses hyphens for user module names: `<host>-user-<user>`.
/// Loads `self.nixosModules.home-manager` once when there are users.
/// System packages are included as `self.nixosModules.packages-*`.
/// Adds `{ networking.hostName = "<host>"; }` as the last modules entry.
pub fn generate_configuration_nix(
    host_name: &str,
    nixos_modules: &[NixModule],
    system_packages: &[NixModule],
    users: &[String],
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("      ./_hardware-configuration.nix".to_string());

    // NixOS modules (all discovered, comment out unselected)
    if !nixos_modules.is_empty() {
        lines.push(String::new());
    }
    for m in nixos_modules {
        lines.push(mod_line("nixosModules", &m.name, m.selected));
    }

    // System packages (all discovered, comment out unselected)
    if !system_packages.is_empty() {
        lines.push(String::new());
    }
    for m in system_packages {
        lines.push(mod_line("nixosModules", &m.name, m.selected));
    }

    // User management: home-manager integration + per-user modules
    if !users.is_empty() {
        lines.push(String::new());
        lines.push("      self.nixosModules.home-manager".to_string());
        for user in users {
            lines.push(format!(
                "      self.nixosModules.{}-user-{}",
                host_name, user
            ));
        }
    }

    // networking.hostName inline block
    lines.push("      {".to_string());
    lines.push(format!("        networking.hostName = \"{}\";", host_name));
    lines.push("      }".to_string());

    let module_lines = lines.join("\n");

    format!(
        "{{ inputs, self, ... }}:\n\
         {{\n\
         \x20 flake.nixosConfigurations.{host_name} = inputs.nixpkgs.lib.nixosSystem {{\n\
         \x20   specialArgs = {{ inherit inputs self; }};\n\
         \x20   modules = [\n\
         {module_lines}\n\
         \x20   ];\n\
         \x20 }};\n\
         }}\n",
        host_name = host_name,
        module_lines = module_lines,
    )
}

/// Helper: format a homeManagerModules attribute reference.
fn hm_attr(name: &str) -> String {
    format!("self.homeManagerModules.{}", name)
}

/// Generate a user-<username>.nix that defines both the system user AND
/// the Home Manager imports. Everything lives in a single nixosModule
/// named `<host>-user-<user>`.
///
/// `hm_base_modules` comes from config.toml and lists modules that are
/// always included (e.g. `["home"]`).
///
/// Passwords are NOT embedded in the Nix configuration. They are set
/// post-install via `nixos-enter --root /mnt -- chpasswd`.
pub fn generate_user_nix(
    host_name: &str,
    username: &str,
    hm_modules: &[NixModule],
    package_modules: &[NixModule],
    hm_base_modules: &[String],
) -> String {
    let mut import_lines: Vec<String> = Vec::new();

    // The `home` module is always required (sets home.stateVersion etc.)
    import_lines.push(format!("        {}", hm_attr("home")));

    // Additional base modules from config.toml (always active)
    for base in hm_base_modules {
        if base != "home" {
            import_lines.push(format!("        {}", hm_attr(base)));
        }
    }

    // HM modules (all discovered; comment out unselected)
    for m in hm_modules {
        if m.selected {
            import_lines.push(format!("        {}", hm_attr(&m.name)));
        } else {
            import_lines.push(format!("        # {}", hm_attr(&m.name)));
        }
    }

    // Package sets (all discovered; comment out unselected)
    for m in package_modules {
        if m.selected {
            import_lines.push(format!("        {}", hm_attr(&m.name)));
        } else {
            import_lines.push(format!("        # {}", hm_attr(&m.name)));
        }
    }

    let imports = import_lines.join("\n");

    // Build the HM imports block only if there are any modules to import
    let hm_block = if !imports.is_empty() {
        format!(
            "\n      home-manager.users.{username}.imports = [\n\
             {imports}\n\
             \x20     ];",
            username = username,
            imports = imports,
        )
    } else {
        String::new()
    };

    let module_name = format!("{}-user-{}", host_name, username);

    format!(
        "{{ ... }}:\n\
         {{\n\
         \x20 flake.nixosModules.{module_name} =\n\
         \x20   {{\n\
         \x20     pkgs,\n\
         \x20     self,\n\
         \x20     inputs,\n\
         \x20     ...\n\
         \x20   }}:\n\
         \x20   {{\n\
         \x20     users.users.{username} = {{\n\
         \x20       isNormalUser = true;\n\
         \x20       extraGroups = [ \"wheel\" ];\n\
         \x20     }};{hm_block}\n\
         \x20   }};\n\
         }}\n",
        module_name = module_name,
        username = username,
        hm_block = hm_block,
    )
}

// ---------------------------------------------------------------------------
// File writing
// ---------------------------------------------------------------------------

/// Ensure the host directory exists and return its path.
fn ensure_host_dir(base_path: &Path, host_name: &str) -> Result<PathBuf, String> {
    let host_dir = base_path.join("modules").join("hosts").join(host_name);
    fs::create_dir_all(&host_dir)
        .map_err(|e| format!("Failed to create host directory: {}", e))?;
    Ok(host_dir)
}

/// Write configuration files to the host directory.
pub fn write_host_config(
    base_path: &Path,
    host_name: &str,
    config_content: &str,
) -> Result<(), String> {
    let host_dir = ensure_host_dir(base_path, host_name)?;
    let config_path = host_dir.join("configuration.nix");
    fs::write(&config_path, config_content)
        .map_err(|e| format!("Failed to write configuration.nix: {}", e))?;
    Ok(())
}

/// Write the user-<username>.nix system user definition to the host directory.
pub fn write_user_config(
    base_path: &Path,
    host_name: &str,
    username: &str,
    content: &str,
) -> Result<(), String> {
    let host_dir = ensure_host_dir(base_path, host_name)?;
    let user_path = host_dir.join(format!("user-{}.nix", username));
    fs::write(&user_path, content)
        .map_err(|e| format!("Failed to write user config: {}", e))?;
    Ok(())
}

/// Write the hardware configuration to the host directory.
pub fn write_hardware_config(
    base_path: &Path,
    host_name: &str,
    content: &str,
) -> Result<(), String> {
    let host_dir = ensure_host_dir(base_path, host_name)?;
    let hw_path = host_dir.join("_hardware-configuration.nix");
    fs::write(&hw_path, content)
        .map_err(|e| format!("Failed to write hardware config: {}", e))?;
    Ok(())
}

/// Hash a password using mkpasswd or openssl (mirrors install.sh step_set_password).
/// Passes the password via stdin to avoid exposing it in /proc/<pid>/cmdline.
/// NOTE: This is kept for potential future use but is no longer called during
/// the wizard flow. Passwords are set post-install via nixos-enter + chpasswd.
#[allow(dead_code)]
pub fn hash_password(password: &str) -> Result<String, String> {
    use std::io::Write;

    // Try mkpasswd first (read password from stdin with --stdin)
    if let Ok(mut child) = std::process::Command::new("mkpasswd")
        .args(["-m", "sha-512", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(password.as_bytes());
        }
        if let Ok(output) = child.wait_with_output() {
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
        }
    }

    // Fallback: openssl (read password from stdin)
    if let Ok(mut child) = std::process::Command::new("openssl")
        .args(["passwd", "-6", "-stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(password.as_bytes());
        }
        if let Ok(output) = child.wait_with_output() {
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
        }
    }

    Err("Neither mkpasswd nor openssl available for password hashing".to_string())
}
