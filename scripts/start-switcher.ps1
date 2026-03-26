$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$logDir = Join-Path $root ".tmp"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

$stdout = Join-Path $logDir "start-switcher.stdout.log"
$stderr = Join-Path $logDir "start-switcher.stderr.log"
$watchdogLog = Join-Path $logDir "start-switcher.watchdog.log"
$pnpm = Join-Path $env:APPDATA "npm\pnpm.cmd"
$healthUrl = if ($env:CODEX_SWITCHER_WATCHDOG_URL) { $env:CODEX_SWITCHER_WATCHDOG_URL } else { "http://127.0.0.1:3210/api/health" }
$checkIntervalSec = if ($env:CODEX_SWITCHER_WATCHDOG_INTERVAL_SEC) { [int]$env:CODEX_SWITCHER_WATCHDOG_INTERVAL_SEC } else { 15 }
$restartDelaySec = if ($env:CODEX_SWITCHER_WATCHDOG_RESTART_DELAY_SEC) { [int]$env:CODEX_SWITCHER_WATCHDOG_RESTART_DELAY_SEC } else { 5 }
$startupGraceSec = if ($env:CODEX_SWITCHER_WATCHDOG_STARTUP_GRACE_SEC) { [int]$env:CODEX_SWITCHER_WATCHDOG_STARTUP_GRACE_SEC } else { 90 }
$maxFailures = if ($env:CODEX_SWITCHER_WATCHDOG_MAX_FAILURES) { [int]$env:CODEX_SWITCHER_WATCHDOG_MAX_FAILURES } else { 3 }

if (-not (Test-Path $pnpm)) {
  $pnpm = Join-Path $env:ProgramFiles "nodejs\pnpm.cmd"
}

if (-not (Test-Path $pnpm)) {
  throw "pnpm.cmd not found at $pnpm"
}

function Write-Log {
  param(
    [string]$Message
  )

  $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
  $line = "[$timestamp] $Message"
  Write-Host $line
  Add-Content -Path $watchdogLog -Value $line
}

function Start-SwitcherProcess {
  if (Test-Path $stdout) {
    Remove-Item $stdout -Force
  }

  if (Test-Path $stderr) {
    Remove-Item $stderr -Force
  }

  $arguments = '/c', "`"$pnpm`" lan"
  $proc = Start-Process -FilePath $env:ComSpec -ArgumentList $arguments -WorkingDirectory $root -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
  Write-Log "Started switcher process tree with PID $($proc.Id)"
  return $proc
}

function Stop-SwitcherProcess {
  param(
    $Process
  )

  if ($null -eq $Process) {
    return
  }

  try {
    $Process.Refresh()
  } catch {
    return
  }

  if ($Process.HasExited) {
    return
  }

  Write-Log "Stopping switcher process tree with PID $($Process.Id)"
  & taskkill /PID $Process.Id /T /F | Out-Null
}

function Test-SwitcherHealth {
  try {
    $response = Invoke-RestMethod -Uri $healthUrl -Method Get -TimeoutSec 5
    return $response.ok -eq $true
  } catch {
    return $false
  }
}

$switcherProcess = $null

try {
  Write-Log "Watchdog started. Health URL: $healthUrl"

  while ($true) {
    $switcherProcess = Start-SwitcherProcess
    $startedAt = Get-Date
    $consecutiveFailures = 0
    $startupHealthy = $false

    while ($true) {
      Start-Sleep -Seconds $checkIntervalSec

      try {
        $switcherProcess.Refresh()
      } catch {
        Write-Log "Failed to refresh switcher process state: $($_.Exception.Message)"
        break
      }

      if ($switcherProcess.HasExited) {
        Write-Log "Switcher exited with code $($switcherProcess.ExitCode). Restarting in $restartDelaySec sec."
        break
      }

      $isHealthy = Test-SwitcherHealth

      if ($isHealthy) {
        if (-not $startupHealthy) {
          Write-Log "Healthcheck passed after startup."
          $startupHealthy = $true
        }

        $consecutiveFailures = 0
        continue
      }

      $secondsSinceStart = ((Get-Date) - $startedAt).TotalSeconds
      if (-not $startupHealthy -and $secondsSinceStart -lt $startupGraceSec) {
        Write-Log "Healthcheck not ready yet ($([int]$secondsSinceStart)s/$startupGraceSec s grace window)."
        continue
      }

      $consecutiveFailures += 1
      Write-Log "Healthcheck failed ($consecutiveFailures/$maxFailures)."

      if ($consecutiveFailures -lt $maxFailures) {
        continue
      }

      Write-Log "Healthcheck failure threshold reached. Restarting switcher."
      Stop-SwitcherProcess -Process $switcherProcess
      break
    }

    Start-Sleep -Seconds $restartDelaySec
  }
} finally {
  Stop-SwitcherProcess -Process $switcherProcess
}
