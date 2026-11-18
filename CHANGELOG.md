# Changelog

All notable changes to Chud Converter. Versions follow [Semantic Versioning](https://semver.org/).

## [0.1.0] — 2026-11-18

First public version (alpha).

### Conversion
- Real format detection from file content; misleading extensions are flagged.
- Planner that picks the best path and chains engines when needed: 195 known formats,
  1,706 direct conversions with FFmpeg and 7-Zip.
- Fidelity report **before** converting (lossless, depends on content, lossy) with the exact
  list of what is lost; every result is verified (decoded when reasonable) and detected again.
- Video, audio, subtitles and animations (FFmpeg), images and data (built in), archives (7-Zip).
- Originals are never modified; name conflicts become `file (1).ext`; cancellation leaves no
  partial file.

### Desktop app
- Drag and drop, format picker with fidelity dots, generated options, queue, detailed reports.
- Presets (21 provided) and a quick-convert window.
- Right-click menu in Windows File Explorer, for your account only, removed cleanly.
- English and French, switchable in **Settings**.
- In-app updates: signed packages (Ed25519), automatic check at most once a day (can be
  turned off), nothing installed without your click.

### Command line
- `chud detect`, `targets`, `plan`, `convert`, `presets`, `shell`, `language`, `engines`,
  `formats`, `matrix`; `--json` for scripts.

### Downloads
- **Windows:** `…_x64-setup.exe` (installer, per user, no administrator rights), or the
  portable `…_x64_portable.zip`.
- **Linux:** `.AppImage` (updates itself), `.deb`, `.rpm`.
- Command line alone: `chud_…_windows_x64.zip`, `chud_…_linux_x86_64.tar.gz`.
- FFmpeg and 7-Zip are needed for video, audio and archives (`winget install Gyan.FFmpeg`,
  `winget install 7zip.7zip`, or your distribution's packages).
- The installers are not code-signed yet: Windows SmartScreen may warn on first launch
  (*More info* → *Run anyway*). Check the download against `SHA256SUMS.txt`.
