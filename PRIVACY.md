# Privacy Policy — Velvet App Launcher

**Effective Date:** November 26, 2026

Velvet App Launcher ("Velvet") is a local desktop application launcher for Windows.

## Data Collection

**Velvet does not collect, store, sell, or transmit any personal data, telemetry, analytics, crash reports, or usage logs.**

All search indexing, app launch frequency ranking, pinned favorites, themes, and configuration settings remain strictly on your device under your local Windows user profile (`%APPDATA%\velvet\`) and `%LOCALAPPDATA%\Velvet\`.

## Network Access

- **Microsoft Store editions** make **no network requests** at all; app updates are handled exclusively by the Microsoft Store.
- **Standalone GitHub / installer builds** may optionally query the public GitHub Releases API (`https://api.github.com/repos/Starfalleso/Velvet/releases/latest`) solely to check whether a newer version is available (`[update] check_updates = true` in `config.toml`, which can be disabled at any time). No personal or device identifiers are sent.
- When you manually enter a URL using command mode (`> https://...`), Velvet opens that URL in your default web browser at your explicit request.

## Permissions & System Behavior

Velvet runs as a standard desktop utility (`runFullTrust`) only to:
- Index installed Start Menu and Store apps locally on your machine
- Register your configured global toggle hotkey (and optional lone `Win` key hook)
- Hide or restore the Windows taskbar when explicitly enabled in settings

## Contact & Source Code

Velvet is open-source software hosted at:
- Repository: https://github.com/Starfalleso/Velvet
- Support / Issues: https://github.com/Starfalleso/Velvet/issues
