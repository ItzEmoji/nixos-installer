use std::process::Command;
use std::sync::{Arc, Mutex};

/// Shared state for the git clone progress.
#[derive(Debug, Clone)]
pub struct CloneState {
    pub log: Vec<String>,
    pub phase: String,
    pub percent: u8,
    pub error: Option<String>,
    pub done: bool,
}

/// Clone a git repository to `dest` with progress tracking.
/// The progress is reported via the shared `CloneState`.
/// Uses `git clone --progress` and parses stderr for progress info.
pub fn clone_repo(url: &str, dest: &std::path::Path, state: Arc<Mutex<CloneState>>) {
    use std::io::Read;

    let log = |state: &Arc<Mutex<CloneState>>, msg: &str| {
        if let Ok(mut s) = state.lock() {
            s.log.push(msg.to_string());
        }
    };

    log(&state, &format!("Cloning {}...", url));
    if let Ok(mut s) = state.lock() {
        s.phase = "Starting clone...".to_string();
    }

    let mut cmd = Command::new("git");
    cmd.args(["clone", "--progress", url])
        .arg(dest.as_os_str())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());

    match cmd.spawn() {
        Ok(mut child) => {
            // git clone --progress writes progress to stderr
            if let Some(stderr) = child.stderr.take() {
                let reader = std::io::BufReader::new(stderr);
                // git progress lines use \r for in-place updates, so we read
                // byte-by-byte and split on \r or \n.
                let mut line_buf = String::new();
                let mut bytes = reader.bytes();
                while let Some(Ok(byte)) = bytes.next() {
                    if byte == b'\r' || byte == b'\n' {
                        let line = line_buf.trim().to_string();
                        if !line.is_empty() {
                            // Parse progress from lines like:
                            //   Receiving objects:  42% (123/456), 1.2 MiB | 5.0 MiB/s
                            //   Resolving deltas:  80% (12/15)
                            //   Enumerating objects: 100, done.
                            if let Ok(mut s) = state.lock() {
                                s.phase = line.clone();
                                // Try to extract percentage
                                if let Some(pct_pos) = line.find('%') {
                                    // Walk backwards from '%' to find the number
                                    let before = &line[..pct_pos];
                                    let num_str: String = before
                                        .chars()
                                        .rev()
                                        .take_while(|c| c.is_ascii_digit())
                                        .collect::<String>()
                                        .chars()
                                        .rev()
                                        .collect();
                                    if let Ok(pct) = num_str.parse::<u8>() {
                                        s.percent = pct;
                                    }
                                }
                                s.log.push(line);
                            }
                        }
                        line_buf.clear();
                    } else {
                        line_buf.push(byte as char);
                    }
                }
                // Flush remaining buffer
                let line = line_buf.trim().to_string();
                if !line.is_empty() {
                    if let Ok(mut s) = state.lock() {
                        s.log.push(line);
                    }
                }
            }

            match child.wait() {
                Ok(status) if status.success() => {
                    log(&state, "Clone completed successfully.");
                    if let Ok(mut s) = state.lock() {
                        s.percent = 100;
                        s.phase = "Clone complete!".to_string();
                        s.done = true;
                    }
                }
                Ok(status) => {
                    let msg = format!(
                        "git clone failed with exit code {:?}",
                        status.code()
                    );
                    log(&state, &msg);
                    if let Ok(mut s) = state.lock() {
                        s.error = Some(msg);
                    }
                }
                Err(e) => {
                    let msg = format!("Failed to wait for git clone: {}", e);
                    log(&state, &msg);
                    if let Ok(mut s) = state.lock() {
                        s.error = Some(msg);
                    }
                }
            }
        }
        Err(e) => {
            let msg = format!("Failed to run git clone: {}", e);
            log(&state, &msg);
            if let Ok(mut s) = state.lock() {
                s.error = Some(msg);
            }
        }
    }
}

/// Represents a physical block device detected on the system.
#[derive(Debug, Clone)]
pub struct BlockDevice {
    #[allow(dead_code)]
    pub name: String,       // e.g. "sda", "nvme0n1"
    pub path: String,       // e.g. "/dev/sda"
    #[allow(dead_code)]
    pub size_bytes: u64,
    pub size_human: String, // e.g. "500G"
    pub model: String,
}

/// Represents a single partition the user wants to create.
#[derive(Debug, Clone)]
pub struct PartitionPlan {
    pub label: String,       // user-facing label, e.g. "EFI", "root", "swap"
    pub mount_point: String, // e.g. "/boot", "/", "swap"
    pub size_mb: Option<u64>, // None = fill remaining space
    pub fs_type: FsType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FsType {
    Fat32,
    Ext4,
    Btrfs,
    Swap,
}

impl FsType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FsType::Fat32 => "vfat",
            FsType::Ext4 => "ext4",
            FsType::Btrfs => "btrfs",
            FsType::Swap => "swap",
        }
    }

    pub const ALL: &[FsType] = &[FsType::Fat32, FsType::Ext4, FsType::Btrfs, FsType::Swap];

    pub fn all() -> &'static [FsType] {
        Self::ALL
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            FsType::Fat32 => "FAT32 (EFI)",
            FsType::Ext4 => "ext4",
            FsType::Btrfs => "Btrfs",
            FsType::Swap => "swap",
        }
    }
}

/// List all block devices (disks, not partitions) using lsblk.
/// Returns Ok with a list of devices, or Err with an error message if lsblk fails.
pub fn list_block_devices() -> Result<Vec<BlockDevice>, String> {
    let output = Command::new("lsblk")
        .args([
            "-d",         // disks only (no partitions)
            "-n",         // no header
            "-b",         // bytes
            "-o", "NAME,SIZE,MODEL",
            "--json",
        ])
        .output()
        .map_err(|e| format!("Failed to run lsblk: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "lsblk failed (exit {:?}): {}",
            output.status.code(),
            stderr.trim()
        ));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse lsblk output: {}", e))?;

    let devices = match parsed.get("blockdevices").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Ok(Vec::new()),
    };

    Ok(devices
        .iter()
        .filter_map(|dev| {
            let name = dev.get("name")?.as_str()?.to_string();
            let size_bytes = dev
                .get("size")
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0);
            let model = dev
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .trim()
                .to_string();

            // Skip tiny devices (< 1 GB), loop devices, ram disks
            if size_bytes < 1_000_000_000 {
                return None;
            }
            if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("zram") {
                return None;
            }

            Some(BlockDevice {
                path: format!("/dev/{}", name),
                size_human: format_bytes(size_bytes),
                name,
                size_bytes,
                model,
            })
        })
        .collect())
}

/// Format bytes into a human-readable string.
fn format_bytes(bytes: u64) -> String {
    const GIB: u64 = 1_073_741_824;
    const TIB: u64 = GIB * 1024;
    if bytes >= TIB {
        format!("{:.1} TiB", bytes as f64 / TIB as f64)
    } else {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    }
}

/// Wipe the disk, create a GPT partition table, and create partitions.
pub fn partition_disk(disk: &str, partitions: &[PartitionPlan]) -> Result<(), String> {
    // 1. Wipe existing partition table
    run_cmd("wipefs", &["-a", "-f", disk])?;

    // 2. Create GPT label
    run_cmd("parted", &["-s", disk, "mklabel", "gpt"])?;

    // 3. Create partitions sequentially
    let mut start_mb: u64 = 1; // start at 1 MiB (alignment)

    for (i, part) in partitions.iter().enumerate() {
        let end = match part.size_mb {
            Some(size) => {
                let end_mb = start_mb + size;
                format!("{}MiB", end_mb)
            }
            None => "100%".to_string(),
        };

        let fs_flag = match part.fs_type {
            FsType::Fat32 => "fat32",
            FsType::Ext4 => "ext4",
            FsType::Btrfs => "btrfs",
            FsType::Swap => "linux-swap",
        };

        run_cmd(
            "parted",
            &[
                "-s",
                disk,
                "mkpart",
                &part.label,
                fs_flag,
                &format!("{}MiB", start_mb),
                &end,
            ],
        )?;

        // Set ESP flag on EFI partition
        if part.fs_type == FsType::Fat32 && part.mount_point == "/boot" {
            let part_num = format!("{}", i + 1);
            run_cmd("parted", &["-s", disk, "set", &part_num, "esp", "on"])?;
        }

        if let Some(size) = part.size_mb {
            start_mb += size;
        }
    }

    Ok(())
}

/// Format the partitions and mount them.
pub fn format_and_mount(disk: &str, partitions: &[PartitionPlan]) -> Result<(), String> {
    // Resolve partition device paths
    let part_prefix = if disk.contains("nvme") || disk.contains("mmcblk") {
        format!("{}p", disk)
    } else {
        disk.to_string()
    };

    for (i, part) in partitions.iter().enumerate() {
        let dev = format!("{}{}", part_prefix, i + 1);

        // Format
        match part.fs_type {
            FsType::Fat32 => run_cmd("mkfs.fat", &["-F", "32", &dev])?,
            FsType::Ext4 => run_cmd("mkfs.ext4", &["-F", &dev])?,
            FsType::Btrfs => run_cmd("mkfs.btrfs", &["-f", &dev])?,
            FsType::Swap => {
                run_cmd("mkswap", &[&dev])?;
                run_cmd("swapon", &[&dev])?;
                continue; // no mount point
            }
        };

        // Mount
        if part.mount_point == "/" {
            run_cmd("mount", &[&dev, "/mnt"])?;
        }
    }

    // Second pass: mount non-root partitions (they need /mnt to exist first)
    for (i, part) in partitions.iter().enumerate() {
        let dev = format!("{}{}", part_prefix, i + 1);

        if part.fs_type == FsType::Swap || part.mount_point == "/" {
            continue;
        }

        let target = format!("/mnt{}", part.mount_point);
        run_cmd("mkdir", &["-p", &target])?;
        run_cmd("mount", &[&dev, &target])?;
    }

    Ok(())
}

/// Generate NixOS hardware configuration.
pub fn generate_hardware_config() -> Result<String, String> {
    let output = Command::new("nixos-generate-config")
        .args(["--root", "/mnt", "--show-hardware-config"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run nixos-generate-config: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "nixos-generate-config failed (exit {:?}):\n{}",
            output.status.code(),
            stderr.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Set the root password in the target system.
pub fn set_root_password(password: &str) -> Result<(), String> {
    let mut child = Command::new("nixos-enter")
        .args(["--root", "/mnt", "--", "chpasswd"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run nixos-enter: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(format!("root:{}\n", password).as_bytes())
            .map_err(|e| format!("Failed to write password: {}", e))?;
    }

    let status = child
        .wait()
        .map_err(|e| format!("Failed to wait for chpasswd: {}", e))?;

    if !status.success() {
        return Err("chpasswd failed in target".to_string());
    }
    Ok(())
}

/// Set a user password using chpasswd inside the target system.
pub fn set_user_password_in_target(username: &str, password: &str) -> Result<(), String> {
    let input = format!("{}:{}", username, password);
    let mut child = Command::new("nixos-enter")
        .args(["--root", "/mnt", "--", "chpasswd"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run nixos-enter: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(format!("{}\n", input).as_bytes())
            .map_err(|e| format!("Failed to write: {}", e))?;
    }

    let status = child
        .wait()
        .map_err(|e| format!("chpasswd failed: {}", e))?;

    if !status.success() {
        return Err("chpasswd failed in target".to_string());
    }
    Ok(())
}

/// Copy the repository into the target system's /etc/nixos/ so the user can
/// modify the config and push to GitHub after reboot.
pub fn copy_repo_to_target(base_path: &std::path::Path) -> Result<(), String> {
    run_cmd("mkdir", &["-p", "/mnt/etc/nixos"])?;
    // Copy contents (not the directory itself) preserving .git, permissions, etc.
    let src = format!("{}/.", base_path.to_string_lossy());
    run_cmd("cp", &["-a", &src, "/mnt/etc/nixos/"])
}

/// Stage all new/modified files in the repo so the flake can see them.
pub fn git_add_all(base_path: &std::path::Path) -> Result<(), String> {
    let output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(base_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run 'git add': {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git add failed: {}", stderr.trim()));
    }

    Ok(())
}

/// Reboot the system.
pub fn reboot() -> Result<(), String> {
    run_cmd("reboot", &[])
}

/// Run an install hook script with installer context as environment variables.
/// Returns Ok(output) with the script's combined stdout+stderr, or Err on failure.
pub fn run_hook(
    script_path: &str,
    host_name: &str,
    base_path: &std::path::Path,
    disk_path: &str,
) -> Result<String, String> {
    let output = Command::new(script_path)
        .env("INSTALLER_HOST_NAME", host_name)
        .env("INSTALLER_BASE_PATH", base_path.to_string_lossy().as_ref())
        .env("INSTALLER_DISK", disk_path)
        .env("INSTALLER_MOUNT_ROOT", "/mnt")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run hook '{}': {}", script_path, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}{}", stdout, stderr);

    if !output.status.success() {
        return Err(format!(
            "Hook '{}' failed with exit code {:?}\n{}",
            script_path,
            output.status.code(),
            combined.trim()
        ));
    }

    Ok(combined)
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run '{}': {}", cmd, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut msg = format!(
            "Command '{}' failed with exit code {:?}",
            cmd,
            output.status.code()
        );
        if !stderr.is_empty() {
            msg.push_str(&format!("\n--- stderr ---\n{}", stderr.trim()));
        }
        if !stdout.is_empty() {
            msg.push_str(&format!("\n--- stdout ---\n{}", stdout.trim()));
        }
        return Err(msg);
    }

    Ok(())
}
