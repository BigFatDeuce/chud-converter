# Changelog

All notable changes to Chud Converter. Versions follow [Semantic Versioning](https://semver.org/).

## [0.5.1] — 2026-11-24

Fixes from an independent code review.

### Fixed
- **Missing targets**: a cheaper but longer route could hide a shorter one that leads further,
  so some reachable formats were not offered. The planner now checks it against an exhaustive
  search on thousands of small graphs.
- **Stuck conversions** (Linux): a helper program left running by an engine could keep the
  conversion waiting until it ended. The conversion now ends as soon as the engine does.
- An engine writing very fast can no longer pile up its output in memory.
- **Folder results** (extracted archives, split PDFs) appear in one go, complete, instead of
  file by file.
- Windows: if process containment cannot be set up, Chud no longer keeps an unusable one.
- Office results (DOCX, XLSX, PPTX) must contain their main part (text, workbook or slides) to
  pass the check.

## [0.5.0] — 2026-11-23

A new **Tools** screen: merge, split and rotate PDFs, put several files into one archive.

### Added
- **Tools** screen (**Ctrl+2**), and `chud tool` on the command line:
  - **Merge PDFs**: several PDFs into one, in the order you choose (move up or down, sort by
    name). One bookmark per file with its own bookmarks below, or only the files' bookmarks, or
    none.
  - **Split a PDF**: one file per page, every N pages, or your own ranges (`1-3; 4-6; 7-z`). Each
    part keeps its bookmarks, the title and the language, and nothing of the other pages stays
    hidden in it.
  - **Rotate a PDF**: every page or some, by 90°, 180° or 270°, without recompressing anything.
  - **Create an archive**: several files and folders into one ZIP, 7z, TAR, TGZ, TBZ2 or TXZ.
    Two files with the same name are both kept (the second becomes `name (2)`).
- Same rules as conversions: what is lost is said **before** running (accessibility tags,
  attachments, named links, signatures), originals are never modified, the result is read back
  (structure, page count, bookmarks, rotation; integrity, names and sizes for archives) and never
  replaces one of the chosen files. Password-protected, damaged or non-PDF files are refused with
  a clear reason.
- The Convert screen offers these tools for the selected files; a missing engine (qpdf, 7-Zip)
  can be installed from the Tools screen.

### Changed
- The keyboard shortcuts now go from **Ctrl+1** to **Ctrl+6** (Tools is second).

### Fixed
- **Calibre could not be installed from the app** (404 Not Found): its author removes the files of a
  GitHub release as soon as the next one is out. The app now downloads the very same file (same
  SHA-256) from Calibre's official archive, which keeps every version.
- **Calibre crashing while closing** (after the book was written and announced) failed the
  conversion when Windows is not in English; the result is now kept and checked, as intended.

## [0.4.0] — 2026-11-22

A new interface: the whole desktop app was redesigned around a "deep space" look. Nothing was
removed: every button, option and screen of 0.3.0 is still there.

### Added
- **Deep-space interface**: night-sky background with slow, subtle stars and orbits, glass
  panels, a new navigation sidebar with a glowing marker on the active screen, three steps
  shown on the Convert screen (Files, Format, Conversion), new icons and status pills, the same
  style in every screen, dialog, empty state, loading and error message, and in the quick
  conversion window.
- **Settings → Appearance**: theme (system, dark, light) and animations (automatic, reduced,
  off). "Automatic" follows the system's reduced-motion setting; "off" stops every animation.
  The choice applies to every Chud Converter window at once.
- Keyboard shortcuts: **Ctrl+1** to **Ctrl+5** open the screens, **Ctrl+O** adds files.
- Narrow windows keep only the icons in the sidebar (names on hover).
- The starry background is drawn once, then only moved by slow CSS animations: no drawing loop
  runs while the app sits open.

## [0.3.0] — 2026-11-20

Linux works for real (AppImage, Fedora, Nobara), and every document, spreadsheet, presentation,
PDF, e-book and image format the app offers was converted to each of its targets, then read
back, on Windows and on Linux (about 1,700 real conversions each): every failure found is fixed.

### Added
- **LibreOffice installed without some modules** (Fedora and Nobara ship Writer, Calc and Impress
  as separate packages): the app now sees which ones are there, only offers the files it can
  really convert, says what is missing ("Incomplete installation, missing: Calc
  (spreadsheets)…"), and adds the missing packages in one click.
- `chud targets` says why nothing can be converted, and what to install.

### Fixed
- **Icons**: an image larger than 256 px could not become an ICO with ImageMagick ("width or
  height exceeds limit"); it is now reduced to 256 px, and the plan says so.
- **TGA** files were taken for Windows cursors (CUR), which start with the same four bytes.
  TGA has no signature: a file whose header is consistent is now recognised with medium
  confidence instead of being a mere guess from its name.
- **APNG** was offered for still images through a detour (PCX, JPEG 2000, PSD, PDF…) and gave a
  plain PNG named `.apng`; it is now offered only when the result is really animated.
- **PPM** files written by the app were PAM files (P7) that libvips and many viewers refuse; they
  are now real PPM files (transparency, which PPM cannot hold, is announced as lost, whichever
  engine writes it).
- **Radiance HDR** written by libvips on a French system had an unreadable header
  (`EXPOSURE=1,000000e+00`, with a decimal comma); libvips no longer writes HDR, the built-in
  engine and ImageMagick do. HDR and EXR to JPEG 2000 no longer go through libvips, which
  refused them ("not an integer format").
- A **DDS** whose sides are not multiples of 4, or a **floating-point TIFF**, was called
  "damaged" because the built-in decoder does not read that variant; such files are now converted
  by ImageMagick or libvips.
- **Calibre**: an ODT made by LibreOffice with pictures failed (Calibre's ODF reader); ODT now
  goes through LibreOffice or Pandoc. A comic book to plain text now says "no text to extract"
  instead of "Calibre produced an empty file". On Windows, Calibre crashing while closing, after
  writing the file, no longer loses the result (it is checked like any other).
- **Pandoc**: a Markdown, Typst or LaTeX file with linked pictures could not become a Jupyter
  notebook ("could not fetch resource"): for safety Pandoc only reads the converted file, not the
  pictures next to it, and its notebook writer stops on the first missing one. Each picture is
  now replaced by its caption, and the result says so.
- **LibreOffice** could show "The application cannot be started. User installation could not be
  completed" and wait for a click: when the engines list was reloaded (after an install) during
  conversions, the old LibreOffice engine deleted the temporary profiles the new one was using.
  Each engine now has its own folder of profiles.
- Installing LibreOffice and Calibre at the same time on Windows: both unpacked their package in
  the same temporary folder, and Windows Installer refuses two packages at once; the second now
  waits for the first.
- **E-books to AsciiDoc**: page-break anchors left by Calibre at the very end of the text
  (`[[…_pb_2]]`) made the AsciiDoc file unreadable by Pandoc; they are now removed.
- Engines list on Linux: "install … (LibreOffice your distribution's, 0 MB)" now names the
  packages, and the confirmation reads "Install LibreOffice from your distribution's packages?".
- Welcome screen: ImageMagick and libvips showed "for:" followed by nothing; they now say
  "for: advanced images (PSD, HEIC, RAW, JPEG XL…)".
- **Linux AppImage**: LibreOffice, libvips, Tesseract and MediaInfo installed by the system
  showed "installed, but not responding … symbol lookup error … undefined symbol: nghttp2_…"
  (seen on Nobara/Fedora). The AppImage put its own, older libraries first for every program it
  started; engines (and the terminal opened to install them) now get the system's environment.
- **Linux AppImage**: links (website, licence, engine pages) open with the system's environment
  too, so the browser starts.
- **Fedora/Nobara**: installing ExifTool asked dnf for `perl-image-exiftool`, which does not
  exist; the package is `perl-Image-ExifTool` (dnf is case-sensitive).

## [0.2.1] — 2026-11-19

Fixes for converting a whole album at once (several files to another drive), and a new
"Original quality" choice that gets audio, subtitles, video or images out without any loss.

### Added
- **Original quality**: above the formats, the app now suggests what gets the content out
  without losing anything — the audio track as it is (an MP3 inside an MKV comes out as an MP3,
  without re-encoding; FLAC or WAV when it cannot be copied), the original subtitles, the video
  remuxed to MKV, an image as PNG. Only offered when the planned conversion is really lossless.
- **Album folders**: adding a folder that holds only one kind of file (an album with its cover,
  `.txt` or `.cue`) adds the files themselves, ready to convert; a "Keep the whole folder" button
  brings back the old behaviour (to make an archive). Sub-folders ("CD 1", "CD 2") are included.
- When the selected files have no common format (46 tracks and their cover), one button per
  kind of file keeps only those checked.

### Changed
- A preset asking for a bitrate higher than the source's no longer re-encodes for nothing: an
  MP3 at 96 kbit/s stays as it is with the MP3 preset (192 kbit/s), instead of getting bigger and
  worse.

### Fixed
- Several conversions at once to another drive than the app's (music on `M:`, app on `C:`)
  shared the same temporary file: some failed with "the process cannot access the file because
  it is being used by another process (os error 32)". Each conversion now has its own, and a file
  briefly held by Explorer, the search indexer or an antivirus is tried again.
- An MKV holding only audio (game rips), an MP4 holding only audio and an Ogg file with Opus
  inside are no longer flagged "misleading extension".

## [0.2.0] — 2026-11-19

Every engine, from the app: Pandoc, LibreOffice, Calibre, ImageMagick, libvips, qpdf,
Ghostscript, Tesseract, ExifTool, MediaInfo and MKVToolNix are now really used, and each one
installs in one click — no terminal, nothing to copy.

### Added
- **Documents** with Pandoc (Markdown, HTML, DOCX, ODT, RTF, LaTeX, reStructuredText, Org,
  AsciiDoc, Typst, EPUB, Jupyter, FB2, plain text) and **office files** with LibreOffice
  (DOCX/DOC/ODT/RTF, XLSX/XLS/ODS/CSV, PPTX/PPT/ODP, to PDF and between each other).
- **E-books** with Calibre (EPUB, MOBI, AZW3, FB2, PDF, TXT…). DRM-protected books are refused
  with a clear message.
- **Images** with libvips (very large images, HEIC, AVIF, JPEG XL) and ImageMagick (more than
  200 formats, PSD, RAW camera files…).
- **PDF** with qpdf (repair, linearize for the web, compress, extract pages, losslessly) and
  Ghostscript (PDF, PostScript, EPS, AI to PNG/JPEG/TIFF at the chosen resolution; PostScript
  to PDF).
- **Text recognition (OCR)** with Tesseract: image to text or searchable PDF, in the installed
  languages (French and English by default). Always marked as approximate.
- **Lossless MKV** with MKVToolNix: every track, chapter and attachment copied without
  re-encoding; a track MKV cannot hold is reported, not silently dropped.
- **Richer file analysis** with MediaInfo (HDR, Dolby Vision, bitrates, track languages) and
  ExifTool (EXIF, XMP, IPTC, ICC profile), shown in the file details.
- **Install from the app on Windows without admin rights**: official portable builds, archives
  and installers opened as archives (never run), MSI packages unfolded without touching the
  system. ImageMagick and Ghostscript need Microsoft's Visual C++ runtime: when it is missing,
  the confirmation says so and Windows asks for permission once.
- **Linux**: engines without a portable build install from the distribution's packages (apt,
  dnf, pacman, zypper). The app asks the system for the password itself (pkexec), or opens a
  terminal with the command already running. The confirmation says the system package manager
  is used; those packages stay the system's.

### Changed
- The "Coming soon" engine section is gone: every known engine is supported.
- Each conversion now verifies its result with the engine's own checks (re-reading the
  document, decoding the image, checking every page or track), never just the exit code.
- File names with `[`, `%` or `:` are never interpreted by an engine: tools only ever see a
  simple name in a private work folder.

### Fixed
- In English, sizes in reports were written the French way ("10,7 Ko"); they now read "10.7 KB".
- Installing an engine could fail with "access denied" when Windows' antivirus was still
  scanning the new files: the app now waits a few seconds and tries again.
- An engine whose files would exceed Windows' 260-character path limit is refused with a clear
  explanation (move the app to a shorter folder), instead of failing halfway.
- Symbolic links inside ZIP archives (the Linux build of qpdf has some) are copied as plain files
  when they point inside the archive, like in TAR archives, instead of failing the install.
- Pandoc builds that do not embed their data files (Debian, Ubuntu and Fedora packages) cannot
  write DOCX, ODT or EPUB in sandbox mode: those outputs are now tried at startup and left out when
  they fail (LibreOffice and Calibre still offer them), instead of failing at conversion time.
- A download cut by the network, or refused for a moment by the server, is tried again twice
  before the install gives up.
- libvips builds whose HEIF library has no AV1 encoder (Ubuntu packages) no longer offer AVIF:
  writing is tried at startup. Tesseract no longer reads GIF directly (Ubuntu's build fails on
  animated GIFs; another engine turns them into PNG first) nor formats its Leptonica build lacks.
- Jupyter notebooks produced by Pandoc are checked by reading their cells directly: older Pandoc
  versions (Ubuntu 24.04) lost tables when re-reading them, which failed correct conversions.
- Comic book archives (CBZ, CBR, CB7) are no longer used as an intermediate step: a folder or
  archive of arbitrary files turned into a "comic" made Calibre fail with "no pages found".

### Internal
- Unit tests of the native image and data engines no longer leave folders in the temporary
  directory.
- Linux CI installs every engine (ImageMagick 7 from its official portable build, checked by
  SHA-256) and runs the real adapter tests with `CHUD_REQUIRE_TOOLS=1`.
- `engine-sources.yml` now installs the thirteen engines on Windows, Linux x64 and ARM64.

## [0.1.2] — 2026-11-18

Engines without the terminal: FFmpeg and 7-Zip now install from the app, verified, on Windows
and Linux (x64 and ARM64). No command to type or copy anywhere in the app.

### Added
- **First-start welcome**: when engines are missing, the app says so at first launch, explains
  what each one is for (video, audio, subtitles, archives), with its size, version, source and
  license, and installs them one by one or all at once. Shown once; "Later" is always possible.
- **The app says what a file needs**: a file that cannot be converted yet shows "To convert
  “vacances.mkv”, you need FFmpeg" in the Convert panel, with the button to install it, and an
  install link on its line in the file list.
- **Install all missing engines in one click.** A banner on the Convert screen (and at the top
  of Engines) lists what is missing and what it is needed for; one confirmation shows every
  download (size, source, license), then the engines install one after the other, with progress
  and a Stop button. A failure does not stop the others. Each engine can also be installed on
  its own.
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
