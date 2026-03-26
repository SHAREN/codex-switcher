<p align="center">
  <img src="src-tauri/icons/logo.svg" alt="Codex Switcher" width="128" height="128">
</p>

<h1 align="center">Codex Switcher</h1>

<p align="center">
  A Desktop Application for Managing Multiple OpenAI <a href="https://github.com/openai/codex">Codex CLI</a> Accounts<br>
  Easily switch between accounts, monitor usage limits, and stay in control of your quota
</p>

## Features

- **Multi-Account Management** – Add and manage multiple Codex accounts in one place
- **Quick Switching** – Switch between accounts with a single click
- **Usage Monitoring** – View real-time usage for both 5-hour and weekly limits
- **Dual Login Mode** – OAuth authentication or import existing `auth.json` files

## Installation

### Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
- [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/)

### Build from Source

```bash
# Clone the repository
git clone https://github.com/Lampese/codex-switcher.git
cd codex-switcher

# Install dependencies
pnpm install

# Run in development mode
pnpm tauri dev

# Run the LAN web dashboard
pnpm lan

# Build for production
pnpm tauri build
```

The built application will be in `src-tauri/target/release/bundle/`.

The LAN web dashboard listens on `0.0.0.0:3210` by default, so it can be opened from other devices on your local network using `http://<your-pc-ip>:3210`.

`pnpm lan` starts:
- a Node LAN proxy on `0.0.0.0:3210`
- the Rust backend on `127.0.0.1:3211`

This avoids Windows Firewall blocking the Rust binary directly while keeping the same web UI and API.

### Windows watchdog launcher

If you need the LAN dashboard to stay online for Traefik or other remote access, start it through `scripts/start-switcher.cmd` instead of running `pnpm lan` manually.

The watchdog launcher:
- starts `pnpm lan`
- checks `http://127.0.0.1:3210/api/health`
- restarts the process tree if it exits or fails health checks repeatedly
- writes logs to `.tmp/start-switcher.watchdog.log`, `.tmp/start-switcher.stdout.log`, and `.tmp/start-switcher.stderr.log`

Optional environment variables:
- `CODEX_SWITCHER_WATCHDOG_URL`
- `CODEX_SWITCHER_WATCHDOG_INTERVAL_SEC`
- `CODEX_SWITCHER_WATCHDOG_RESTART_DELAY_SEC`
- `CODEX_SWITCHER_WATCHDOG_STARTUP_GRACE_SEC`
- `CODEX_SWITCHER_WATCHDOG_MAX_FAILURES`

Note: ChatGPT OAuth login still requires opening the generated link on the host PC itself because OpenAI redirects back to `localhost`.

## Disclaimer

This tool is designed **exclusively for individuals who personally own multiple OpenAI/ChatGPT accounts**. It is intended to help users manage their own accounts more conveniently.

**This tool is NOT intended for:**
- Sharing accounts between multiple users
- Circumventing OpenAI's terms of service
- Any form of account pooling or credential sharing

By using this software, you agree that you are the rightful owner of all accounts you add to the application. The authors are not responsible for any misuse or violations of OpenAI's terms of service.
