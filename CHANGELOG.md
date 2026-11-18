# Changelog

All notable changes to Chud Converter. Versions follow [Semantic Versioning](https://semver.org/).

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
