# Bundled media tools

Release builds prepare these tools from `scripts/media-tools.lock.json`. The
lock records immutable URLs and SHA-256 values; no third-party executable is
stored in Git or downloaded when GrabBit starts.

## Versions and provenance

- yt-dlp 2026.08.19: official standalone release binaries. yt-dlp code is
  Unlicense; its binary dependency notices are copied from the matching tag.
- FFmpeg 6.1.1 on macOS: built on the native release runner from verified
  FFmpeg 6.1.1, LAME 3.100, and Opus 1.5.2 sources. The build enables GPL,
  libmp3lame, and libopus, disables dependency autodetection, and links only
  pinned static libraries plus operating-system frameworks.
- FFmpeg static distribution b6.1.1 on Windows x64 and Linux x64/ARM64:
  verified release assets from `eugeneware/ffmpeg-static`. Their platform
  README and GPL license are bundled and identify the original builder and
  corresponding FFmpeg source/build.

The installed `media-tools/licenses` directory contains applicable license
texts, yt-dlp bundled-dependency notices, FFmpeg build identification, and
source links. GPL-covered FFmpeg source is available from the exact upstream
tag/commit identified there; GrabBit does not modify FFmpeg.

## Supported targets

- Windows x64
- macOS Intel and Apple Silicon
- Linux x64 and ARM64 (glibc)

Windows ARM64 is intentionally not emitted yet: the reviewed static FFmpeg
distribution does not publish a retained, versioned ARM64 release. The build
fails for unlisted targets rather than substituting an unverified binary.
