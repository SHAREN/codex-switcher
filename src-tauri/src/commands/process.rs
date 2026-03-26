//! Process detection commands

use anyhow::Context;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

#[cfg(windows)]
use std::collections::HashSet;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsCodexProcess {
    name: String,
    process_id: u32,
    parent_process_id: u32,
    #[serde(default)]
    command_line: String,
    #[serde(default)]
    main_window_title: String,
}

/// Information about running Codex processes
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodexProcessInfo {
    /// Number of active Codex app instances
    pub count: usize,
    /// Number of ignored background/stale Codex-related processes
    pub background_count: usize,
    /// Whether switching is allowed (no active Codex app instances)
    pub can_switch: bool,
    /// Process IDs of active Codex app instances
    pub pids: Vec<u32>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexActivityState {
    Unknown,
    Idle,
    Busy,
    AwaitingApproval,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CodexActivityInfo {
    pub state: CodexActivityState,
    pub conversation_id: Option<String>,
    pub last_event_at: Option<String>,
    pub summary: String,
}

const PROCESS_STATE_POLL_ATTEMPTS: usize = 12;
const PROCESS_STATE_POLL_DELAY: Duration = Duration::from_millis(250);
const CODEX_ACTIVITY_BUSY_MAX_AGE_MINUTES: i64 = 30;
const CODEX_ACTIVITY_LOG_TAIL_BYTES: u64 = 256 * 1024;
const CODEX_ACTIVITY_MAX_LOG_FILES: usize = 12;

/// Check for running Codex processes
#[tauri::command]
pub async fn check_codex_processes() -> Result<CodexProcessInfo, String> {
    snapshot_codex_processes().map_err(|e| e.to_string())
}

/// Inspect desktop Codex logs and infer whether a turn is still in progress.
#[tauri::command]
pub async fn get_codex_activity() -> Result<CodexActivityInfo, String> {
    snapshot_codex_activity().map_err(|e| e.to_string())
}

/// Launch the Codex desktop app if it is not already running.
#[tauri::command]
pub async fn start_codex_app() -> Result<CodexProcessInfo, String> {
    let current = snapshot_codex_processes().map_err(|e| e.to_string())?;
    if current.count > 0 {
        return Ok(current);
    }

    launch_codex_app().map_err(|e| e.to_string())?;
    let final_state = wait_for_codex_process_state(true)
        .await
        .map_err(|e| e.to_string())?;

    if final_state.count == 0 {
        return Err("Codex app launch was requested, but no running window was detected.".into());
    }

    Ok(final_state)
}

/// Stop all active Codex desktop app instances.
#[tauri::command]
pub async fn stop_codex_app() -> Result<CodexProcessInfo, String> {
    let current = snapshot_codex_processes().map_err(|e| e.to_string())?;
    if current.count == 0 {
        return Ok(current);
    }

    stop_codex_processes(&current.pids).map_err(|e| e.to_string())?;
    let final_state = wait_for_codex_process_state(false)
        .await
        .map_err(|e| e.to_string())?;

    if final_state.count > 0 {
        return Err(format!(
            "Failed to stop all Codex app instances. {} still running.",
            final_state.count
        ));
    }

    Ok(final_state)
}

fn snapshot_codex_processes() -> anyhow::Result<CodexProcessInfo> {
    let (pids, bg_count) = find_codex_processes()?;
    Ok(build_codex_process_info(pids, bg_count))
}

fn build_codex_process_info(pids: Vec<u32>, background_count: usize) -> CodexProcessInfo {
    let count = pids.len();
    CodexProcessInfo {
        count,
        background_count,
        can_switch: count == 0,
        pids,
    }
}

fn snapshot_codex_activity() -> anyhow::Result<CodexActivityInfo> {
    let process_info = snapshot_codex_processes()?;
    if process_info.count == 0 {
        return Ok(CodexActivityInfo {
            state: CodexActivityState::Idle,
            conversation_id: None,
            last_event_at: None,
            summary: "Codex desktop app is not running.".into(),
        });
    }

    #[cfg(windows)]
    {
        let log_files = find_windows_codex_activity_log_files(&process_info.pids)?;
        if log_files.is_empty() {
            return Ok(CodexActivityInfo {
                state: CodexActivityState::Unknown,
                conversation_id: None,
                last_event_at: None,
                summary: "Codex desktop logs were not found, so live turn activity is unknown."
                    .into(),
            });
        }

        let mut raw_lines = Vec::new();
        for path in log_files {
            let contents = read_log_tail(&path, CODEX_ACTIVITY_LOG_TAIL_BYTES)
                .with_context(|| format!("failed to read {}", path.display()))?;
            raw_lines.extend(contents.lines().map(str::to_owned));
        }
        raw_lines.sort_unstable();

        let events = parse_codex_activity_events(&raw_lines.join("\n"));

        return Ok(analyze_codex_activity(&events));
    }

    #[cfg(not(windows))]
    {
        Ok(CodexActivityInfo {
            state: CodexActivityState::Unknown,
            conversation_id: None,
            last_event_at: None,
            summary: "Codex turn activity inference is currently implemented only for Windows desktop logs."
                .into(),
        })
    }
}

async fn wait_for_codex_process_state(should_be_running: bool) -> anyhow::Result<CodexProcessInfo> {
    let mut last_state = snapshot_codex_processes()?;

    for _ in 0..PROCESS_STATE_POLL_ATTEMPTS {
        let is_running = last_state.count > 0;
        if is_running == should_be_running {
            return Ok(last_state);
        }

        sleep(PROCESS_STATE_POLL_DELAY).await;
        last_state = snapshot_codex_processes()?;
    }

    Ok(last_state)
}

fn stop_codex_processes(pids: &[u32]) -> anyhow::Result<()> {
    for pid in pids {
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .output()
                .with_context(|| format!("failed to stop Codex PID {pid}"))?;
        }

        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .creation_flags(CREATE_NO_WINDOW)
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output()
                .with_context(|| format!("failed to stop Codex PID {pid}"))?;
        }
    }

    Ok(())
}

fn launch_codex_app() -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .args(["-a", "Codex"])
            .status()
            .context("failed to launch Codex app")?;

        if !status.success() {
            anyhow::bail!("macOS launcher returned a non-zero exit status");
        }

        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        anyhow::bail!(
            "Launching the Codex desktop app is currently supported only on Windows and macOS"
        );
    }

    #[cfg(windows)]
    {
        let app_id = find_windows_codex_app_id()?;
        let status = Command::new("explorer.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .arg(format!("shell:AppsFolder\\{app_id}"))
            .status()
            .context("failed to launch Codex via explorer.exe")?;

        if !status.success() {
            anyhow::bail!("explorer.exe returned a non-zero exit status while launching Codex");
        }

        return Ok(());
    }

    #[allow(unreachable_code)]
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CodexActivityEventKind {
    TurnStart,
    TurnInterrupt,
    ApprovalRequested,
    ApprovalResolved,
    Terminal(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexActivityEvent {
    timestamp: String,
    conversation_id: String,
    kind: CodexActivityEventKind,
}

#[derive(Debug, Clone, Default)]
struct CodexConversationActivity {
    last_start_at: Option<String>,
    last_terminal_at: Option<String>,
    last_terminal_status: Option<String>,
    last_approval_at: Option<String>,
    last_approval_resolved_at: Option<String>,
}

fn parse_codex_activity_events(contents: &str) -> Vec<CodexActivityEvent> {
    let mut events = Vec::new();
    let mut approval_request_map = HashMap::<String, String>::new();

    for line in contents.lines() {
        let Some(timestamp) = extract_log_timestamp(line) else {
            continue;
        };

        if line.contains("[desktop-notifications] show approval") {
            let Some(conversation_id) = extract_log_value(line, "conversationId=") else {
                continue;
            };

            if let Some(request_id) = extract_log_value(line, "requestId=") {
                approval_request_map.insert(request_id, conversation_id.clone());
            }

            events.push(CodexActivityEvent {
                timestamp,
                conversation_id,
                kind: CodexActivityEventKind::ApprovalRequested,
            });
            continue;
        }

        if line.contains("method=item/commandExecution/requestApproval")
            && line.contains("response=")
        {
            let Some(request_id) = extract_log_value(line, "id=") else {
                continue;
            };
            let Some(conversation_id) = approval_request_map.get(&request_id).cloned() else {
                continue;
            };

            events.push(CodexActivityEvent {
                timestamp,
                conversation_id,
                kind: CodexActivityEventKind::ApprovalResolved,
            });
            continue;
        }

        let Some(conversation_id) = extract_log_value(line, "conversationId=") else {
            continue;
        };

        let kind = if line.contains("method=turn/start") {
            Some(CodexActivityEventKind::TurnStart)
        } else if line.contains("method=turn/interrupt") {
            Some(CodexActivityEventKind::TurnInterrupt)
        } else if line.contains("[desktop-notifications] show turn-complete") {
            Some(CodexActivityEventKind::Terminal("completed".into()))
        } else if let Some(status) = extract_log_value(line, "latestTurnStatus=") {
            if matches!(
                status.as_str(),
                "completed" | "failed" | "interrupted" | "cancelled"
            ) {
                Some(CodexActivityEventKind::Terminal(status))
            } else {
                None
            }
        } else {
            None
        };

        if let Some(kind) = kind {
            events.push(CodexActivityEvent {
                timestamp,
                conversation_id,
                kind,
            });
        }
    }

    events.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
    events
}

fn analyze_codex_activity(events: &[CodexActivityEvent]) -> CodexActivityInfo {
    if events.is_empty() {
        return CodexActivityInfo {
            state: CodexActivityState::Unknown,
            conversation_id: None,
            last_event_at: None,
            summary: "No Codex turn activity markers were found in the recent desktop logs.".into(),
        };
    }

    let mut conversations = HashMap::<String, CodexConversationActivity>::new();

    for event in events {
        let entry = conversations
            .entry(event.conversation_id.clone())
            .or_default();

        match &event.kind {
            CodexActivityEventKind::TurnStart => {
                entry.last_start_at = Some(event.timestamp.clone());
            }
            CodexActivityEventKind::TurnInterrupt => {
                entry.last_terminal_at = Some(event.timestamp.clone());
                entry.last_terminal_status = Some("interrupted".into());
            }
            CodexActivityEventKind::ApprovalRequested => {
                entry.last_approval_at = Some(event.timestamp.clone());
            }
            CodexActivityEventKind::ApprovalResolved => {
                entry.last_approval_resolved_at = Some(event.timestamp.clone());
            }
            CodexActivityEventKind::Terminal(status) => {
                entry.last_terminal_at = Some(event.timestamp.clone());
                entry.last_terminal_status = Some(status.clone());
            }
        }
    }

    let mut approval_candidate: Option<(String, CodexConversationActivity)> = None;
    let mut busy_candidate: Option<(String, CodexConversationActivity)> = None;
    let mut stale_unresolved_candidate: Option<(String, CodexConversationActivity)> = None;

    for (conversation_id, activity) in conversations {
        let Some(last_start_at) = activity.last_start_at.clone() else {
            continue;
        };

        let has_terminal_after_start = activity
            .last_terminal_at
            .as_ref()
            .is_some_and(|terminal| terminal >= &last_start_at);

        if has_terminal_after_start {
            continue;
        }

        let has_pending_approval_after_start =
            activity.last_approval_at.as_ref().is_some_and(|approval| {
                approval >= &last_start_at
                    && activity
                        .last_approval_resolved_at
                        .as_ref()
                        .is_none_or(|resolved| resolved < approval)
            });

        if has_pending_approval_after_start {
            if should_replace_activity_candidate(
                approval_candidate.as_ref().map(|(_, current)| current),
                &activity,
                CandidateKind::Approval,
            ) {
                approval_candidate = Some((conversation_id, activity));
            }
            continue;
        }

        if is_recent_activity_timestamp(
            &last_start_at,
            ChronoDuration::minutes(CODEX_ACTIVITY_BUSY_MAX_AGE_MINUTES),
        ) {
            if should_replace_activity_candidate(
                busy_candidate.as_ref().map(|(_, current)| current),
                &activity,
                CandidateKind::Busy,
            ) {
                busy_candidate = Some((conversation_id, activity));
            }
        } else if should_replace_activity_candidate(
            stale_unresolved_candidate
                .as_ref()
                .map(|(_, current)| current),
            &activity,
            CandidateKind::Stale,
        ) {
            stale_unresolved_candidate = Some((conversation_id, activity));
        }
    }

    if let Some((conversation_id, activity)) = approval_candidate {
        let last_event_at = activity.last_approval_at.or(activity.last_start_at);
        return CodexActivityInfo {
            state: CodexActivityState::AwaitingApproval,
            conversation_id: Some(conversation_id),
            last_event_at: last_event_at.clone(),
            summary: "A Codex session is waiting for approval.".into(),
        };
    }

    if let Some((conversation_id, activity)) = busy_candidate {
        let last_event_at = activity.last_start_at;
        return CodexActivityInfo {
            state: CodexActivityState::Busy,
            conversation_id: Some(conversation_id),
            last_event_at: last_event_at.clone(),
            summary: "A Codex turn appears to still be running.".into(),
        };
    }

    if let Some((conversation_id, activity)) = stale_unresolved_candidate {
        let last_event_at = activity.last_start_at;
        return CodexActivityInfo {
            state: CodexActivityState::Unknown,
            conversation_id: Some(conversation_id),
            last_event_at: last_event_at.clone(),
            summary: "The latest Codex turn has no clear completion marker.".into(),
        };
    }

    let last_event_at = events.last().map(|event| event.timestamp.clone());
    CodexActivityInfo {
        state: CodexActivityState::Idle,
        conversation_id: None,
        last_event_at: last_event_at.clone(),
        summary: "No unfinished Codex turn was detected in the recent desktop logs.".into(),
    }
}

#[derive(Clone, Copy)]
enum CandidateKind {
    Approval,
    Busy,
    Stale,
}

fn should_replace_activity_candidate(
    current: Option<&CodexConversationActivity>,
    next: &CodexConversationActivity,
    kind: CandidateKind,
) -> bool {
    let next_key = activity_candidate_key(next, kind);
    let current_key = current.and_then(|activity| activity_candidate_key(activity, kind));
    match (current_key, next_key) {
        (None, Some(_)) => true,
        (Some(current_key), Some(next_key)) => next_key > current_key,
        _ => false,
    }
}

fn activity_candidate_key(
    activity: &CodexConversationActivity,
    kind: CandidateKind,
) -> Option<&str> {
    match kind {
        CandidateKind::Approval => activity
            .last_approval_at
            .as_deref()
            .or(activity.last_start_at.as_deref()),
        CandidateKind::Busy | CandidateKind::Stale => activity.last_start_at.as_deref(),
    }
}

fn extract_log_timestamp(line: &str) -> Option<String> {
    line.split_once(' ')
        .map(|(timestamp, _)| timestamp.to_string())
}

fn extract_log_value(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let value = &line[start..];
    let token = value.split_whitespace().next()?.trim();
    if token.is_empty() || token == "null" {
        return None;
    }

    Some(token.trim_matches('"').trim_end_matches(',').to_string())
}

fn is_recent_activity_timestamp(timestamp: &str, max_age: ChronoDuration) -> bool {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|parsed| Utc::now().signed_duration_since(parsed.with_timezone(&Utc)) <= max_age)
        .unwrap_or(false)
}

fn read_log_tail(path: &Path, max_bytes: u64) -> anyhow::Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let file_len = file.metadata()?.len();
    let start = file_len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let text = String::from_utf8_lossy(&buffer);

    if start == 0 {
        return Ok(text.into_owned());
    }

    Ok(text.lines().skip(1).collect::<Vec<_>>().join("\n"))
}

#[cfg(windows)]
fn find_windows_codex_activity_log_files(active_root_pids: &[u32]) -> anyhow::Result<Vec<PathBuf>> {
    let package_dirs = find_windows_codex_package_dirs()?;
    let mut log_files = Vec::new();

    for package_dir in package_dirs {
        let logs_dir = package_dir
            .join("LocalCache")
            .join("Local")
            .join("Codex")
            .join("Logs");

        if !logs_dir.is_dir() {
            continue;
        }

        collect_files_recursively(&logs_dir, &mut log_files)?;
    }

    log_files.retain(|path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();

        active_root_pids
            .iter()
            .any(|pid| name.contains(&format!("-{pid}-")))
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
    });

    log_files.sort_by(|left, right| {
        let left_modified = left
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        let right_modified = right
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        right_modified.cmp(&left_modified)
    });
    log_files.truncate(CODEX_ACTIVITY_MAX_LOG_FILES);

    Ok(log_files)
}

#[cfg(windows)]
fn find_windows_codex_package_dirs() -> anyhow::Result<Vec<PathBuf>> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    let packages_dir = PathBuf::from(local_app_data).join("Packages");

    let mut package_dirs = Vec::new();
    for entry in fs::read_dir(&packages_dir)
        .with_context(|| format!("failed to read {}", packages_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("OpenAI.Codex_") {
            package_dirs.push(entry.path());
        }
    }

    Ok(package_dirs)
}

fn collect_files_recursively(dir: &Path, output: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files_recursively(&path, output)?;
        } else if file_type.is_file() {
            output.push(path);
        }
    }

    Ok(())
}

/// Find all running codex processes. Returns (active_pids, background_count)
fn find_codex_processes() -> anyhow::Result<(Vec<u32>, usize)> {
    #[cfg(unix)]
    {
        let mut pids = Vec::new();
        let mut bg_count = 0;

        // Use ps with custom format to get the pid and full command line
        let output = Command::new("ps").args(["-eo", "pid,command"]).output();

        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) {
                // Skip header
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // The first part is PID, the rest is the command string
                if let Some((pid_str, command)) = line.split_once(' ') {
                    let command = command.trim();

                    // Get the executable path/name (first word of the command string before args)
                    let executable = command.split_whitespace().next().unwrap_or("");

                    // Check if the executable is exactly "codex" or ends with "/codex"
                    let is_codex = executable == "codex" || executable.ends_with("/codex");

                    // Exclude if it's running from an extension or IDE integration (like Antigravity)
                    // These are expected background processes we shouldn't block on
                    let is_ide_plugin = is_ide_plugin_process(command);

                    // Skip our own app
                    let is_switcher =
                        command.contains("codex-switcher") || command.contains("Codex Switcher");

                    if is_codex && !is_switcher {
                        if let Ok(pid) = pid_str.trim().parse::<u32>() {
                            if pid != std::process::id() && !pids.contains(&pid) {
                                if is_ide_plugin {
                                    bg_count += 1;
                                } else {
                                    pids.push(pid);
                                }
                            }
                        }
                    }
                }
            }
        }

        return Ok((pids, bg_count));
    }

    #[cfg(windows)]
    {
        return find_windows_codex_processes();
    }

    #[allow(unreachable_code)]
    Ok((Vec::new(), 0))
}

#[cfg(windows)]
fn find_windows_codex_processes() -> anyhow::Result<(Vec<u32>, usize)> {
    // tasklist counts every Electron helper (`--type=gpu-process`, crashpad, renderer, etc.),
    // which inflates the badge and incorrectly blocks switching. Use PowerShell so we can inspect
    // the command line and only count live top-level app instances.
    const POWERSHELL_SCRIPT: &str = r#"
$windowTitles = @{}
Get-Process -Name Codex -ErrorAction SilentlyContinue | ForEach-Object {
  $windowTitles[[uint32]$_.Id] = $_.MainWindowTitle
}

Get-CimInstance Win32_Process |
  Where-Object { $_.Name -ieq 'Codex.exe' -or $_.Name -ieq 'codex.exe' } |
  ForEach-Object {
    [PSCustomObject]@{
      Name = $_.Name
      ProcessId = [uint32]$_.ProcessId
      ParentProcessId = [uint32]$_.ParentProcessId
      CommandLine = if ($_.CommandLine) { $_.CommandLine } else { '' }
      MainWindowTitle = if ($windowTitles.ContainsKey([uint32]$_.ProcessId)) {
        [string]$windowTitles[[uint32]$_.ProcessId]
      } else {
        ''
      }
    }
  } |
  ConvertTo-Json -Compress
"#;

    let output = Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            POWERSHELL_SCRIPT,
        ])
        .output()
        .context("failed to query Windows process list")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("PowerShell process query failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let processes = parse_windows_codex_processes(&stdout)?;

    let mut active_pids = Vec::new();
    let mut ignored_count = 0;

    for process in processes
        .iter()
        .filter(|process| is_windows_codex_root_process(process))
    {
        let command = process.command_line.to_ascii_lowercase();
        if is_ide_plugin_process(&command) {
            ignored_count += 1;
            continue;
        }

        let has_window = !process.main_window_title.trim().is_empty();
        let has_renderer =
            windows_has_descendant_matching(process.process_id, &processes, |child| {
                child
                    .command_line
                    .to_ascii_lowercase()
                    .contains("--type=renderer")
            });
        let has_app_server =
            windows_has_descendant_matching(process.process_id, &processes, |child| {
                let command = child.command_line.to_ascii_lowercase();
                command.contains("resources\\codex.exe") && command.contains("app-server")
            });

        if has_window || has_renderer || has_app_server {
            active_pids.push(process.process_id);
        } else {
            // Ignore stale helper trees left behind after the window has already closed.
            ignored_count += 1;
        }
    }

    active_pids.sort_unstable();
    active_pids.dedup();

    Ok((active_pids, ignored_count))
}

#[cfg(windows)]
fn find_windows_codex_app_id() -> anyhow::Result<String> {
    const POWERSHELL_SCRIPT: &str = r#"
$startApp = Get-StartApps |
  Where-Object { $_.Name -eq 'Codex' -or $_.AppID -like 'OpenAI.Codex_*' } |
  Select-Object -First 1 -ExpandProperty AppID

if (-not $startApp) {
  $packageFamily = Get-AppxPackage OpenAI.Codex |
    Select-Object -First 1 -ExpandProperty PackageFamilyName

  if ($packageFamily) {
    $startApp = "$packageFamily!App"
  }
}

if (-not $startApp) {
  throw 'Codex app is not installed or its AppUserModelID could not be resolved.'
}

$startApp
"#;

    let output = Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            POWERSHELL_SCRIPT,
        ])
        .output()
        .context("failed to resolve Codex AppUserModelID")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("PowerShell app lookup failed: {}", stderr.trim());
    }

    let app_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if app_id.is_empty() {
        anyhow::bail!("PowerShell app lookup returned an empty AppUserModelID");
    }

    Ok(app_id)
}

#[cfg(windows)]
fn parse_windows_codex_processes(stdout: &str) -> anyhow::Result<Vec<WindowsCodexProcess>> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let value: serde_json::Value =
        serde_json::from_str(trimmed).context("failed to parse Windows process JSON")?;

    match value {
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(|value| {
                serde_json::from_value(value)
                    .context("failed to deserialize Windows Codex process entry")
            })
            .collect(),
        value => Ok(vec![serde_json::from_value(value)
            .context("failed to deserialize Windows Codex process entry")?]),
    }
}

#[cfg(windows)]
fn is_windows_codex_root_process(process: &WindowsCodexProcess) -> bool {
    let name = process.name.to_ascii_lowercase();
    let command = process.command_line.to_ascii_lowercase();

    name == "codex.exe"
        && !command.contains("codex-switcher")
        && !command.contains("--type=")
        && !command.contains("resources\\codex.exe")
}

#[cfg(any(unix, windows))]
fn is_ide_plugin_process(command: &str) -> bool {
    command.contains(".antigravity")
        || command.contains("openai.chatgpt")
        || command.contains(".vscode")
}

#[cfg(windows)]
fn windows_has_descendant_matching<F>(
    root_pid: u32,
    processes: &[WindowsCodexProcess],
    mut predicate: F,
) -> bool
where
    F: FnMut(&WindowsCodexProcess) -> bool,
{
    let mut queue = vec![root_pid];
    let mut visited = HashSet::new();

    while let Some(parent_pid) = queue.pop() {
        for process in processes
            .iter()
            .filter(|process| process.parent_process_id == parent_pid)
        {
            if !visited.insert(process.process_id) {
                continue;
            }

            if predicate(process) {
                return true;
            }

            queue.push(process.process_id);
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    fn activity_event(
        timestamp: String,
        conversation_id: &str,
        kind: CodexActivityEventKind,
    ) -> CodexActivityEvent {
        CodexActivityEvent {
            timestamp,
            conversation_id: conversation_id.into(),
            kind,
        }
    }

    #[test]
    fn build_codex_process_info_marks_switchable_when_no_roots_are_running() {
        let info = build_codex_process_info(Vec::new(), 2);
        assert_eq!(info.count, 0);
        assert!(info.can_switch);
        assert_eq!(info.background_count, 2);
        assert!(info.pids.is_empty());
    }

    #[test]
    fn build_codex_process_info_marks_running_roots_as_not_switchable() {
        let info = build_codex_process_info(vec![1001, 1002], 1);
        assert_eq!(info.count, 2);
        assert!(!info.can_switch);
        assert_eq!(info.background_count, 1);
        assert_eq!(info.pids, vec![1001, 1002]);
    }

    #[test]
    fn analyze_codex_activity_marks_waiting_approval_as_active() {
        let events = vec![
            activity_event(
                "2026-03-22T10:30:25.496Z".into(),
                "conv-1",
                CodexActivityEventKind::TurnStart,
            ),
            activity_event(
                "2026-03-22T10:35:46.727Z".into(),
                "conv-1",
                CodexActivityEventKind::ApprovalRequested,
            ),
        ];

        let info = analyze_codex_activity(&events);

        assert_eq!(info.state, CodexActivityState::AwaitingApproval);
        assert_eq!(info.conversation_id.as_deref(), Some("conv-1"));
        assert_eq!(
            info.last_event_at.as_deref(),
            Some("2026-03-22T10:35:46.727Z")
        );
    }

    #[test]
    fn analyze_codex_activity_marks_recent_unfinished_turn_as_busy() {
        let started_at = (Utc::now() - ChronoDuration::minutes(2)).to_rfc3339();
        let events = vec![activity_event(
            started_at.clone(),
            "conv-2",
            CodexActivityEventKind::TurnStart,
        )];

        let info = analyze_codex_activity(&events);

        assert_eq!(info.state, CodexActivityState::Busy);
        assert_eq!(info.conversation_id.as_deref(), Some("conv-2"));
        assert_eq!(info.last_event_at.as_deref(), Some(started_at.as_str()));
    }

    #[test]
    fn analyze_codex_activity_marks_resolved_approval_as_busy_not_waiting() {
        let started_at = (Utc::now() - ChronoDuration::minutes(3)).to_rfc3339();
        let approval_at = (Utc::now() - ChronoDuration::minutes(2)).to_rfc3339();
        let approval_resolved_at = (Utc::now() - ChronoDuration::minutes(1)).to_rfc3339();
        let events = vec![
            activity_event(
                started_at.clone(),
                "conv-approval",
                CodexActivityEventKind::TurnStart,
            ),
            activity_event(
                approval_at,
                "conv-approval",
                CodexActivityEventKind::ApprovalRequested,
            ),
            activity_event(
                approval_resolved_at.clone(),
                "conv-approval",
                CodexActivityEventKind::ApprovalResolved,
            ),
        ];

        let info = analyze_codex_activity(&events);

        assert_eq!(info.state, CodexActivityState::Busy);
        assert_eq!(info.conversation_id.as_deref(), Some("conv-approval"));
        assert_eq!(info.last_event_at.as_deref(), Some(started_at.as_str()));
    }

    #[test]
    fn analyze_codex_activity_marks_completed_turn_as_idle() {
        let events = vec![
            activity_event(
                "2026-03-22T10:30:25.496Z".into(),
                "conv-3",
                CodexActivityEventKind::TurnStart,
            ),
            activity_event(
                "2026-03-22T10:31:10.000Z".into(),
                "conv-3",
                CodexActivityEventKind::Terminal("completed".into()),
            ),
        ];

        let info = analyze_codex_activity(&events);

        assert_eq!(info.state, CodexActivityState::Idle);
        assert!(info.conversation_id.is_none());
        assert_eq!(
            info.last_event_at.as_deref(),
            Some("2026-03-22T10:31:10.000Z")
        );
    }

    #[test]
    fn parse_codex_activity_events_links_approval_response_and_turn_complete() {
        let contents = r#"
2026-03-22T10:35:46.727Z info [electron-message-handler] [desktop-notifications] show approval conversationId=conv-4 kind=commandExecution requestId=0
2026-03-22T10:54:40.059Z info [electron-message-handler] Sending server response id=0 method=item/commandExecution/requestApproval response={"decision":{"accept":true}}
2026-03-22T10:59:10.717Z info [electron-message-handler] [desktop-notifications] show turn-complete conversationId=conv-4 turnId=turn-1
"#;

        let events = parse_codex_activity_events(contents);

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].kind, CodexActivityEventKind::ApprovalRequested);
        assert_eq!(events[1].kind, CodexActivityEventKind::ApprovalResolved);
        assert_eq!(
            events[2].kind,
            CodexActivityEventKind::Terminal("completed".into())
        );
        assert!(events.iter().all(|event| event.conversation_id == "conv-4"));
    }

    #[cfg(windows)]
    #[test]
    fn parse_windows_codex_processes_accepts_single_json_object() {
        let stdout = r#"{"Name":"Codex.exe","ProcessId":42,"ParentProcessId":0,"CommandLine":"Codex.exe","MainWindowTitle":"Codex"}"#;
        let processes = parse_windows_codex_processes(stdout).expect("parse should succeed");
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].process_id, 42);
    }
}
