#!/bin/sh
set -eu
export PATH="/usr/bin:$PATH"
cd "$(dirname "$0")/.."
root=$(pwd)
source_dir="$root/tmp/librempeg-ims"
revision=34d5e43dc39f6e25da9497a57278032db348ba3c
export PATH="$(cygpath -u "${SDA_MINGW_BIN:-C:/msys64/mingw64/bin}"):/usr/bin:$PATH"
export TMPDIR="$root/tmp"
mkdir -p "$TMPDIR"
if [ ! -d "$source_dir" ]; then
    git clone --filter=blob:none --no-checkout https://github.com/librempeg/librempeg.git "$source_dir"
    git -C "$source_dir" switch --detach "$revision"
fi
test "$(git -C "$source_dir" rev-parse HEAD)" = "$revision" || {
    echo 'Unexpected Librempeg revision; refusing to change an existing checkout.' >&2
    exit 1
}
cd "$source_dir"
patch_file="$root/apps/ac4-decoder/librempeg-noasm.patch"
if git apply --check --unidiff-zero "$patch_file" 2>/dev/null; then
    git apply --unidiff-zero "$patch_file"
else
    git apply --reverse --check --unidiff-zero "$patch_file"
fi
if [ ! -f config.h ]; then
    ./configure --disable-everything --disable-autodetect --disable-doc \
      --disable-network --disable-x86asm --disable-debug --disable-programs \
      --enable-static --disable-shared --enable-decoder=ac4 \
      --enable-demuxer=mov,ac4 --enable-protocol=file,pipe \
      --cc=gcc --target-os=mingw32 --arch=x86_64
fi
mingw32-make -j4 libavcodec/libavcodec.a libavformat/libavformat.a libavutil/libavutil.a
cd "$root"
gcc -O2 -std=c17 -Wall -Wextra -municode -I "$source_dir" \
  apps/ac4-decoder/main.c -o "$source_dir/sda-ac4-stereo.exe" \
  -Wl,--start-group "$source_dir/libavformat/libavformat.a" \
  "$source_dir/libavcodec/libavcodec.a" "$source_dir/libavutil/libavutil.a" \
  -Wl,--end-group -lm -latomic -lbcrypt -lole32 -luser32 -static
