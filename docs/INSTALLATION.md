# Installation and packaging

## Development

Install the platform prerequisites, then run `npm ci` and
`npm run tauri dev` from the repository root.

## Build an installer locally

```sh
npm ci
npm run tauri build
```

Tauri writes native bundles below `target/release/bundle`. Bundles are
platform-specific and must be produced on their target operating system.

### Windows 11

Install Microsoft C++ Build Tools with the Desktop development with C++
workload and ensure WebView2 is available. The release workflow produces the
configured Windows installer. Production distribution requires a Windows code
signing certificate to avoid untrusted-publisher warnings.

### macOS

Install Xcode Command Line Tools. Apple Silicon uses
`aarch64-apple-darwin`; Intel uses `x86_64-apple-darwin`. Public distribution
requires an Apple Developer ID certificate, hardened runtime signing, and
notarization. Local unsigned builds are suitable only for development.

### Linux

Install WebKitGTK 4.1, AppIndicator, librsvg, patchelf, and xdg-utils. Package
availability and names vary by distribution. The release workflow builds on
Ubuntu 22.04 and produces the Tauri-supported Linux bundles.

## Releases

Pushing a `v*` tag or manually dispatching the Release workflow builds Windows,
Linux, macOS Apple Silicon, and macOS Intel artifacts into a draft GitHub
release. Signing secrets are intentionally not embedded in the repository.

