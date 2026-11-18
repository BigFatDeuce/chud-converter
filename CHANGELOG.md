# Changelog

All notable changes to Chud Converter. Versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Install FFmpeg and 7-Zip from the app**, in one click on the Engines screen (or
  `chud engines install ffmpeg|7zip`). Before anything is downloaded, the app shows the version,
  size, publisher and license, with a link to the original release. Each file comes from a fixed
  HTTPS address and is refused if its size or SHA-256 differs from the expected one; the engine is
  extracted under strict rules, tried, and only then activated. Installs live in the app's own
  folder: system installs are never touched or replaced. Cancel, remove and roll back are one
  click away (with confirmation). Available on Windows x86_64 and Linux x86_64 / ARM64 (FFmpeg
  for Linux comes from BtbN's month-end builds).
- A file with no possible conversion offers the missing engine right there ("install FFmpeg
  (110 MB)"); after the install the app goes back to the conversion, with the file still
  selected.
- `chud engines` suggests `chud engines install <id>` for engines the app can install.
- When an engine is already installed on the system, Chud Converter's own copy can be added
  alongside it, only on explicit request (`--alongside` on the command line); it is then used
  first, and removing it hands back to the system one, which is never touched.
- `chud engines uninstall <id> --yes` and `chud engines rollback <id>`.

### Fixed
- Presets screen could not scroll to the bottom (the list was squeezed instead of scrolling);
  the same fix applies to every scrolling screen and panel.
- Long install commands overflowed the engine cards. The app no longer shows any command to
  copy into a terminal: everything is installed from the app. Engines Chud Converter cannot use
  yet (Pandoc, LibreOffice…) are listed apart, under "Coming soon", with no install prompt.
- Installing an engine now shows the extraction progress.
- After the engines changed, the file list was rebuilt from scratch, losing the selection, the
  chosen format and options; they are now kept.

### Internal
- Every long-running UI process is an explicit state machine (`apps/desktop/src/lib/machines.ts`,
  tested by `npm test`): file analysis, conversion jobs (transition table), engine operations,
  engine list, app update (also on the Rust side). States are exposed as `data-state` attributes
  for tests, accessibility tools and agents.
- Weekly CI workflow (`engine-sources.yml`) that really installs each engine on every offered
  platform, so a removed or changed source is caught before users hit it.

## [0.1.1] — 2026-11-18

Reliability release: a full audit of every offered conversion. Nearly 2,000 real conversions
were swept over 97 sample files (common, hard and deliberately broken), and every bug found is
fixed and covered by a permanent test.

### Fixed
- **FLAC → AAC and FLAC → AMR were reported as failed** while the file was fine: ffprobe only
  estimates the duration of raw streams. The duration that counts is now the one measured by
  decoding the whole result.
- Sample rates are chosen within what each encoder accepts (AMR 8 kHz, AC-3, MP2, Opus, WMA…),
  and resampling is announced. Opus is always checked at 48 kHz, its real decoding rate.
- WavPack from MP3/AAC sources, MP2 at low sample rates, 8-bit PCM into CAF, H.264/HEVC copied
  into AVI, frames taken from MPEG-TS/PS, still WebP to video: all failed, all work now.
- CAF and MOV/TS no longer offer what FFmpeg cannot write reliably (AAC/Opus/ALAC/FLAC in CAF,
  AV1 in MOV and TS).
- Ogg (Vorbis, Opus) tags were lost; they are now carried over, or their loss is announced.
- VOB files were detected as MPEG.
- Single compressed files (.xz, .bz2, .zst, and .gz on some systems) could not be converted at
  all: their nameless 7-Zip listing was misread and every extraction was reported incomplete.
- A still image is no longer offered as APNG; TOML and CSV targets are only offered when the
  data really fits them.
- 16-bit images to PNM were wrongly called lossless; ICO files written from opaque images
  could not be read back.

### Verification
- Every check now matches what was asked: channel count, sample rate, dimensions (rotation
  allowed), forced frame rate, and audio/video sync compared with the source.
- “Lossless” is proven, not assumed: decoded audio samples (MD5) and image pixels are compared
  with the source.
- Outputs without a known duration (MPEG-PS, VOB…) are fully decoded too.
- Damaged sources are diagnosed: a truncated file whose readable part was fully converted
  passes with an explicit “incomplete source” note; inherited damage is named as such.
- Files that no engine can analyze (wrong content behind a known extension, truncated images)
  are reported as unreadable instead of being offered dozens of doomed conversions.

### App and command line
- Failed jobs say which kind of failure it was: verification failed (no file kept), unreadable
  source, impossible conversion, or conversion error, with the exact cause below.
- `chud` no longer panics when its output is piped into a command that closes early.

### Tests
- 127 automated tests, including a permanent matrix that runs every offered conversion of
  about 30 generated media, text, data and archive samples (840 conversions); every archive
  format written is read back, as are .gz, .bz2, .xz, .zst, .tar.zst, .deb, .cpio and .cab.
- Large files checked by hand: one hour of WAV to FLAC, MP3 and Opus, and ten minutes of
  1080p video to MP4 and WebM, each fully decoded at verification.

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
