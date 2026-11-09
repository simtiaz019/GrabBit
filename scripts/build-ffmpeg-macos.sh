#!/bin/sh
set -eu

if [ "$#" -ne 5 ]; then
  echo "usage: build-ffmpeg-macos.sh <arch> <ffmpeg.tar.xz> <opus.tar.gz> <lame.tar.gz> <output-dir>" >&2
  exit 2
fi

arch=$1
ffmpeg_archive=$2
opus_archive=$3
lame_archive=$4
output_dir=$5
work_dir="$output_dir/work"
prefix="$output_dir/prefix"

mkdir -p "$work_dir" "$prefix" "$output_dir/bin" "$output_dir/licenses"
tar -xf "$ffmpeg_archive" -C "$work_dir"
tar -xf "$opus_archive" -C "$work_dir"
tar -xf "$lame_archive" -C "$work_dir"

case "$arch" in
  arm64) clang_arch=arm64; ffmpeg_arch=aarch64 ;;
  x64) clang_arch=x86_64; ffmpeg_arch=x86_64 ;;
  *) echo "unsupported macOS architecture: $arch" >&2; exit 2 ;;
esac

export MACOSX_DEPLOYMENT_TARGET=11.0
export CFLAGS="-arch $clang_arch -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET -O2"
export CXXFLAGS="$CFLAGS"
export LDFLAGS="-arch $clang_arch -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"

cd "$work_dir/opus-1.5.2"
./configure --prefix="$prefix" --disable-shared --enable-static
make -j"$(sysctl -n hw.logicalcpu)"
make install

cd "$work_dir/lame-3.100"
./configure --prefix="$prefix" --disable-shared --enable-static --disable-frontend
make -j"$(sysctl -n hw.logicalcpu)"
make install

cd "$work_dir/ffmpeg-6.1.1"
PKG_CONFIG_PATH="$prefix/lib/pkgconfig" ./configure \
  --prefix="$prefix" \
  --arch="$ffmpeg_arch" \
  --cc=clang \
  --disable-shared \
  --enable-static \
  --disable-autodetect \
  --disable-doc \
  --disable-debug \
  --disable-ffplay \
  --enable-gpl \
  --enable-libmp3lame \
  --enable-libopus \
  --pkg-config-flags=--static \
  --extra-cflags="-I$prefix/include $CFLAGS" \
  --extra-ldflags="-L$prefix/lib $LDFLAGS"
make -j"$(sysctl -n hw.logicalcpu)" ffmpeg ffprobe

cp ffmpeg ffprobe "$output_dir/bin/"
cp COPYING.GPLv2 "$output_dir/licenses/FFmpeg-COPYING.GPLv2.txt"
cp "$work_dir/opus-1.5.2/COPYING" "$output_dir/licenses/Opus-COPYING.txt"
cp "$work_dir/lame-3.100/COPYING" "$output_dir/licenses/LAME-COPYING.txt"
./ffmpeg -version > "$output_dir/FFmpeg-BUILD.txt"
