# Idle HUD

Minimal Windows overlay that pops up when you stop typing. Shows writing time, idle time, key count, and average CPS; hides as soon as you type again.

### Why

* Nudge yourself back to work after micro-breaks
* Track light productivity signals without heavy tooling
* Subtle on top of your workflow, not a dashboard

### Key Features

* Appears after **N seconds of inactivity** (default 30s)
* One-minute **idle progress bar** in green; alert sound at 60s
* **Auto-hide on first keystroke**, optional single reconnect sound
* **Fullscreen-aware:** hides and mutes when a fullscreen app is active
* **Always-on-top** with native window decorations and rounded corners
* **No persistence, no admin, no disk I/O**

### What It Shows

* **Writing:** cumulative time while you’re actively typing
* **Idle:** cumulative time not typing
* **Keys:** total A–Z and 0–9 keypresses
* **CPS:** Keys ÷ Writing time (in seconds)

### Typical Use Cases

* Gentle self-accountability during deep work
* Recovering from distraction loops
* Lightweight session rhythm for writers and developers
* Pomodoro-adjacent “soft timer” without strict cycles

### Build & Run

* Requirements: Windows 10/11, Rust stable + Cargo
* Build: `cargo build --release`
* Run: `cargo run --release`

The window starts hidden off-screen and appears when idle crosses the threshold.

### Configure

Adjust constants at the top of `src/main.rs`:

* `IDLE_THRESHOLD_MS` (default 30000)
* `SESSION_GAP_MS` (default 15000)
* `ALERT_MS` (default 60000)

### Notes

* Only A–Z and 0–9 keys are counted; system keys and space are ignored
* Some overlays or anti-cheat systems can block global key hooks
* Sounds use Windows aliases: `DeviceConnect` and `DeviceDisconnect`

### Privacy

No files, no network, no telemetry. State resets daily in memory only.
