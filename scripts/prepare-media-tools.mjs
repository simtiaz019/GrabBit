import { chmod, copyFile, mkdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new globalThis.URL('..', import.meta.url));
const lock = JSON.parse(await readFile(join(root, 'scripts', 'media-tools.lock.json'), 'utf8'));
const target = process.env.TAURI_ENV_TARGET_TRIPLE ?? process.argv[2];
if (!target) throw new Error('TAURI_ENV_TARGET_TRIPLE or a target triple argument is required.');
const selected = lock.targets[target];
if (!selected) throw new Error(`No verified media-tool bundle is defined for ${target}.`);

const cache = join(root, 'target', 'media-tools-cache');
const stage = join(root, 'target', 'media-tools-resources');
await mkdir(cache, { recursive: true });
await rm(stage, { recursive: true, force: true });
await mkdir(stage, { recursive: true });

async function sha256(path) {
  const hash = createHash('sha256');
  hash.update(await readFile(path));
  return hash.digest('hex');
}

async function fetchVerified(url, expected) {
  const cached = join(cache, expected);
  try {
    if ((await sha256(cached)) === expected) return cached;
  } catch { /* cache miss */ }
  let body;
  let lastError;
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    try {
      const response = await globalThis.fetch(url, { redirect: 'follow' });
      if (!response.ok) throw new Error(`Download failed (${response.status}): ${url}`);
      body = new Uint8Array(await response.arrayBuffer());
      break;
    } catch (error) {
      lastError = error;
    }
  }
  const partial = `${cached}.${process.pid}.partial`;
  if (body) {
    await writeFile(partial, body, { mode: 0o755 });
  } else {
    try {
      execFileSync('curl', ['--retry', '3', '--connect-timeout', '15', '-LfsS', '-o', partial, url], { stdio: 'inherit' });
    } catch {
      throw lastError;
    }
  }
  const actual = await sha256(partial);
  if (actual !== expected) throw new Error(`SHA-256 mismatch for ${url}: expected ${expected}, got ${actual}`);
  await rename(partial, cached);
  return cached;
}

const ytBase = `https://github.com/yt-dlp/yt-dlp/releases/download/${lock.ytDlpVersion}`;
const ffBase = `https://github.com/eugeneware/ffmpeg-static/releases/download/b${lock.ffmpegVersion}`;
const executableSuffix = selected.platform === 'win32' ? '.exe' : '';
const artifacts = [
  { name: `yt-dlp${executableSuffix}`, url: `${ytBase}/${selected.ytDlpAsset}`, sha: selected.ytDlpSha256, kind: 'yt-dlp' },
];

let macosBuild;
if (selected.ffmpegBuild === 'macos-source') {
  const source = lock.macosSourceBuild;
  const archives = await Promise.all([
    fetchVerified(source.ffmpegUrl, source.ffmpegSha256),
    fetchVerified(source.opusUrl, source.opusSha256),
    fetchVerified(source.lameUrl, source.lameSha256),
  ]);
  macosBuild = join(cache, `macos-v2-${selected.arch}-ffmpeg-${lock.ffmpegVersion}-opus-${source.opusVersion}-lame-${source.lameVersion}`);
  const ffmpeg = join(macosBuild, 'bin', 'ffmpeg');
  const ffprobe = join(macosBuild, 'bin', 'ffprobe');
  try {
    await Promise.all([stat(ffmpeg), stat(ffprobe)]);
  } catch {
    await rm(macosBuild, { recursive: true, force: true });
    execFileSync('sh', [join(root, 'scripts', 'build-ffmpeg-macos.sh'), selected.arch, ...archives, macosBuild], { cwd: root, stdio: 'inherit' });
  }
  artifacts.push(
    { name: 'ffmpeg', source: ffmpeg, kind: 'ffmpeg' },
    { name: 'ffprobe', source: ffprobe, kind: 'ffprobe' },
  );
} else {
  artifacts.push(
    { name: `ffmpeg${executableSuffix}`, url: `${ffBase}/ffmpeg-${selected.ffmpegPlatform}`, sha: selected.ffmpegSha256, kind: 'ffmpeg' },
    { name: `ffprobe${executableSuffix}`, url: `${ffBase}/ffprobe-${selected.ffmpegPlatform}`, sha: selected.ffprobeSha256, kind: 'ffprobe' },
  );
}

function binaryArchitectures(bytes) {
  if (bytes[0] === 0x4d && bytes[1] === 0x5a) {
    const pe = bytes.readUInt32LE(0x3c);
    const machine = bytes.readUInt16LE(pe + 4);
    return machine === 0x8664 ? ['x64'] : machine === 0xaa64 ? ['arm64'] : [];
  }
  if (bytes[0] === 0x7f && bytes.subarray(1, 4).toString() === 'ELF') {
    const machine = bytes.readUInt16LE(18);
    return machine === 62 ? ['x64'] : machine === 183 ? ['arm64'] : [];
  }
  const magic = bytes.readUInt32BE(0);
  if (magic === 0xcafebabe) {
    const count = bytes.readUInt32BE(4);
    return Array.from({ length: count }, (_, index) => bytes.readUInt32BE(8 + index * 20))
      .map((cpu) => cpu === 0x01000007 ? 'x64' : cpu === 0x0100000c ? 'arm64' : null).filter(Boolean);
  }
  const cpu = bytes.readUInt32LE(4);
  return cpu === 0x01000007 ? ['x64'] : cpu === 0x0100000c ? ['arm64'] : [];
}

for (const artifact of artifacts) {
  const cached = artifact.source ?? await fetchVerified(artifact.url, artifact.sha);
  const architectures = binaryArchitectures(await readFile(cached));
  if (!architectures.includes(selected.arch)) throw new Error(`${artifact.name} does not contain ${selected.arch} code.`);
  const output = join(stage, artifact.name);
  await copyFile(cached, output);
  if (selected.platform !== 'win32') await chmod(output, 0o755);
  artifact.output = output;
}

const noticeArtifacts = [
  { name: 'yt-dlp-LICENSE.txt', url: `https://raw.githubusercontent.com/yt-dlp/yt-dlp/${lock.ytDlpVersion}/LICENSE`, sha: lock.notices.ytDlpLicenseSha256 },
  { name: 'yt-dlp-THIRD_PARTY_LICENSES.txt', url: `https://raw.githubusercontent.com/yt-dlp/yt-dlp/${lock.ytDlpVersion}/THIRD_PARTY_LICENSES.txt`, sha: lock.notices.ytDlpThirdPartySha256 },
];
if (!macosBuild) noticeArtifacts.push(
  { name: 'FFmpeg-LICENSE.txt', url: `${ffBase}/${selected.ffmpegPlatform}.LICENSE`, sha: selected.licenseSha256 },
  { name: 'FFmpeg-BUILD-README.txt', url: `${ffBase}/${selected.ffmpegPlatform}.README`, sha: selected.readmeSha256 },
);
await mkdir(join(stage, 'licenses'), { recursive: true });
for (const notice of noticeArtifacts) {
  const cached = await fetchVerified(notice.url, notice.sha);
  await copyFile(cached, join(stage, 'licenses', notice.name));
}
if (macosBuild) {
  for (const name of ['FFmpeg-COPYING.GPLv2.txt', 'Opus-COPYING.txt', 'LAME-COPYING.txt']) {
    await copyFile(join(macosBuild, 'licenses', name), join(stage, 'licenses', name));
  }
  await copyFile(join(macosBuild, 'FFmpeg-BUILD.txt'), join(stage, 'licenses', 'FFmpeg-BUILD.txt'));
}
const ffmpegProvenance = macosBuild
  ? `Built from verified source with Opus ${lock.macosSourceBuild.opusVersion} and LAME ${lock.macosSourceBuild.lameVersion}.\nBuild identification and GPL/LGPL/BSD license texts are included under licenses/.`
  : `Binary distributor and build/source metadata: https://github.com/eugeneware/ffmpeg-static/tree/b${lock.ffmpegVersion}\nThe distributor README and applicable GPL license are included under licenses/.`;
await writeFile(join(stage, 'THIRD_PARTY_NOTICES.txt'), `GrabBit bundled media tools\n\nyt-dlp ${lock.ytDlpVersion}\nSource: https://github.com/yt-dlp/yt-dlp/tree/${lock.ytDlpVersion}\nRelease binaries and bundled dependencies are covered by the included yt-dlp license files.\n\nFFmpeg ${lock.ffmpegVersion}\nSource: https://github.com/FFmpeg/FFmpeg/tree/n${lock.ffmpegVersion}\n${ffmpegProvenance}\n`);

const hostPlatform = process.platform;
const hostArch = process.arch;
if (hostPlatform === selected.platform && hostArch === selected.arch) {
  execFileSync(artifacts[0].output, ['--version'], { stdio: 'pipe', timeout: 30000 });
  const encoders = execFileSync(artifacts[1].output, ['-hide_banner', '-encoders'], { encoding: 'utf8', timeout: 30000 });
  for (const encoder of ['libmp3lame', 'aac', 'libopus']) {
    if (!encoders.includes(encoder)) throw new Error(`Bundled FFmpeg lacks required ${encoder} encoder.`);
  }
  execFileSync(artifacts[2].output, ['-version'], { stdio: 'pipe', timeout: 30000 });
}
globalThis.console.log(`Prepared verified media tools ${lock.ytDlpVersion}/${lock.ffmpegVersion} for ${target}.`);
