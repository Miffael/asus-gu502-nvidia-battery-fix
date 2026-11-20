# asus-gu502-nvidia-battery-fix (`nv-service-tray`)

A tiny native Windows tray app (Rust, ~330 KB, ~11 MB RAM) that lets the NVIDIA dGPU of an
Optimus laptop **actually sleep** on battery. It stops NVIDIA's background services in the quiet
[G-Helper](https://github.com/seerge/g-helper) modes (Silent / Balanced) and gives you two tray toggles to
start them on demand.

Built and measured on an **ASUS ROG Zephyrus M GU502GV** (i7-9750H, RTX 2060 + Intel UHD 630, no MUX),
but it works on any Windows laptop with NVIDIA Optimus and G-Helper.

> Not affiliated with ASUS or NVIDIA. Use at your own risk; everything it does is reversible (see *Uninstall*).

## The problem

On Optimus laptops the Intel iGPU drives the internal screen and the NVIDIA dGPU should power down
(runtime D3) whenever no game or GPU app needs it. On the GU502GV it barely ever did:

- G-Helper **Eco / Optimized mode is not available** — the BIOS doesn't expose the switch
  (`Eco flag : -65536` in the G-Helper log), so the dGPU can't be cut off completely. Runtime D3 is the only way to save power.
- Even with nothing running on the RTX, it kept waking up. The culprit turned out to be
  **`NVDisplay.ContainerLocalSystem`** — the NVIDIA Display Container service. One of its plugins
  (most likely the power-policy plugin `_nvtopps`) polls the GPU every 20–60 s and pulls it back to D0.
- **`NvContainerLocalSystem`** (NVIDIA App backend) doesn't wake the GPU, but keeps ~180 MB of background
  processes alive (overlay, ShadowPlay, telemetry, account, AI/MCP containers) that you may not want.

Neither service is needed for games to run. Rendering, DLSS, Frame Generation, Reflex, PhysX, CUDA, NVENC,
NvAPI and Optimus GPU routing are all handled by the driver itself (`nvlddmkm` + its user-mode DLLs).
The dGPU still wakes up automatically when a game starts or an external monitor is plugged in (on the GU502GV
HDMI is wired to the dGPU).

### What you lose while the services are stopped

| Service | Without it |
|---|---|
| `NVDisplay.ContainerLocalSystem` | NVIDIA Control Panel won't open; driver profiles aren't updated between driver releases. Settings you already made stay in effect. |
| `NvContainerLocalSystem` | NVIDIA App overlay (Alt+Z), Instant Replay / recording, performance overlay, Freestyle filters, "optimal settings", automatic tuning, driver update notifications. |

Need one of them? Flip its toggle in the tray, change the setting, flip it back — driver settings persist.

## Results (GU502GV, same day, battery, Windows 11 26200)

| | Before | After |
|---|---|---|
| dGPU in D3 (sleep), sampled | 4 / 24 | **24 / 24** (and 122 / 122 in a 30-min run) |
| Battery drain, normal light use (browser, terminal), 100 % brightness | ~32.5 W | **~14.2 W** |
| Battery drain, idle | — | **~6.3 W** |
| Estimated runtime on a full 76 Wh battery | ~2.3 h | ~5.4 h light use / ~12 h idle |

Workloads weren't identical (the "before" window also included diagnostic scripts), so treat the
battery numbers as an order of magnitude; the D3 sampling is the direct measurement.

## How it works

- Every `poll_ms` (2 s) it reads G-Helper's `performance_mode` from `%APPDATA%\GHelper\config.json`
  (0 = Balanced, 1 = Turbo, 2 = Silent). The file is re-parsed only when its modification time changes.
- It applies a per-mode policy to **both** services:

  | G-Helper mode | Default policy | Effect |
  |---|---|---|
  | Silent | `off` | Services stopped and kept stopped (if the driver or an update starts them again, they're stopped within a poll) |
  | Balanced | `off` | Same |
  | Turbo | `leave` | Nothing is touched — services stay in whatever state they are |

  Possible values: `off`, `on`, `leave`. E.g. `"turbo": "on"` brings the NVIDIA App back whenever you switch to Turbo for gaming.
- **Manual toggles** in the tray menu override the policy **until the next G-Helper mode change**;
  then automation takes over again.
- The **"Автоматика"** checkbox turns the policy off entirely (only manual toggles remain).
- If G-Helper's mode can't be read, the app does nothing automatically.
- Start/stop requests go straight to the Service Control Manager (fire-and-forget); state is re-read on the next poll.
  Already-running / not-active errors are treated as success.
- It doesn't detect games and doesn't need to: the dGPU wakes up on demand without these services.

### Tray icon and menu

- Icon: **green** — both services stopped (dGPU can sleep); **orange** — at least one is running;
  **grey** — state unknown (e.g. no admin rights to query/control services).
- Tooltip: current mode and the state of each service.
- Menu (UI is in Russian):
  - `G-Helper: <mode>` — current mode (info only)
  - `NVIDIA Display Container (панель NVIDIA)` — toggle
  - `NVIDIA App Container (оверлей, запись, телеметрия)` — toggle
    (suffixes: ` [вручную]` = manual override, ` ⚠` = last start/stop failed, see the log)
  - `Автоматика` — enable/disable the per-mode policy
  - `Открыть лог` / `Открыть настройки (config.json)` — open in Notepad
  - `Выход` — quit

## Settings

`%APPDATA%\nv-service-tray\config.json` — created with defaults on first run and re-read automatically when edited:

```json
{
  "ghelper_config": "",
  "poll_ms": 2000,
  "auto": true,
  "policy": { "silent": "off", "balanced": "off", "turbo": "leave" }
}
```

- `ghelper_config` — path to G-Helper's `config.json`; empty = `%APPDATA%\GHelper\config.json`.
- `poll_ms` — poll interval, clamped to 500–60000 ms.
- Log: `%LOCALAPPDATA%\nv-service-tray\nv-service-tray.log` (mode changes, every start/stop with its reason, errors;
  rotates to `.log.old` at 512 KB).

## Install

Controlling services requires admin rights. The installer copies the exe to
`C:\Program Files\nv-service-tray\` (admin-only folder, so the elevated task can't be hijacked by replacing the exe)
and registers a Task Scheduler logon task with *highest privileges* — no UAC prompt at logon, runs on battery,
no time limit, single instance.

```powershell
# from an elevated PowerShell, in the repo folder
cargo build --release
powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1          # -WhatIf to preview
```

Recommended (optional) one-time tweak: set the Display Container to manual start so it doesn't wake the dGPU
right after boot, before the app starts:

```powershell
Set-Service NVDisplay.ContainerLocalSystem -StartupType Manual   # undo: -StartupType Automatic
```

NVIDIA driver updates may reset it to Automatic — harmless, the app stops it anyway in Silent/Balanced.

## Uninstall

```powershell
# elevated PowerShell
powershell -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1 -RestoreServices [-Purge]
```

- removes the task and `C:\Program Files\nv-service-tray\`;
- `-RestoreServices` sets `NVDisplay.ContainerLocalSystem` back to Automatic and starts both services;
- `-Purge` also deletes the settings and the log.

## Build

Requirements: Rust (MSVC toolchain, `stable-x86_64-pc-windows-msvc`) and Visual Studio Build Tools (C++ workload).

```powershell
cargo test              # policy, G-Helper parsing, icon rendering
cargo build --release   # target\release\nv-service-tray.exe
```

Windows **Smart App Control** blocks unsigned, locally built binaries (`os error 4551`) — it has to be off to build and run.

Code layout:

| File | What |
|---|---|
| `src/main.rs` | tray, menu, message loop + timer, reconcile loop |
| `src/policy.rs` | pure decision logic (mode → desired state → start/stop), unit-tested |
| `src/services.rs` | minimal Service Control Manager wrapper (`windows-sys`) |
| `src/ghelper.rs` | reads `performance_mode` from G-Helper's config, mtime-cached |
| `src/config.rs` | settings file, paths |
| `src/util.rs` | log with rotation, safe file writes, single-instance mutex, generated tray icon |

## Security notes

The app runs **elevated** (it has to, to control services), so it is written to avoid becoming a
privilege-escalation path for non-admin processes of the same user:

- The binary lives in `C:\Program Files\nv-service-tray\` (writable only by admins); the scheduled task points there.
- Settings and log live in the user profile, which non-admin processes can modify. All writes refuse paths that go
  through a symlink/junction or hit a file with multiple hard links, so they can't be redirected into protected
  locations. Settings are only parsed as JSON; at worst a tampered file changes the policy for the two NVIDIA services.
- Notepad (for "open log/settings") is started by its absolute path from `GetSystemDirectoryW`, not via
  `PATH`, *App Paths* or `%windir%`, which are user-controllable.
- No network access, no telemetry, no secrets stored.

## Related tips for the same laptop

- Pin non-game GPU-accelerated apps (Discord, Steam, browsers, Electron apps) to the iGPU:
  *Settings → System → Display → Graphics → Power saving*. Games stay on "Let Windows decide" and go to the RTX via NVIDIA profiles.
- `nvidia-smi` itself wakes the dGPU. To check its power state without waking it:
  ```powershell
  $id = (Get-PnpDevice -Class Display | Where FriendlyName -like '*NVIDIA*').InstanceId
  $d = (Get-PnpDeviceProperty -InstanceId $id -KeyName DEVPKEY_Device_PowerData).Data
  'D' + ([BitConverter]::ToUInt32($d, 4) - 1)   # D0 = awake, D3 = asleep
  ```
- WSL2 with GPU passthrough keeps the dGPU awake — `wsl --shutdown` when you don't need it.

## License

MIT — see [LICENSE](LICENSE).
