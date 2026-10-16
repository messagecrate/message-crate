#!/usr/bin/env bash
# Check that the ffmpeg and ffprobe the desktop app downloads on Windows on
# ARM run there (#2370). nightly.yml runs it on GitHub's windows-11-arm
# runner, in Git Bash.
#
# The app gives Windows on ARM the Windows x64 files, which Windows runs
# under emulation. This script reads exactly the files
# src-tauri/src/tool_downloads/pins.rs pins for windows/aarch64, so a change
# to the pin is what it checks the next night. It downloads each .gz, checks
# its SHA-256, unpacks it and checks the program's SHA-256, as
# src-tauri/src/tool_downloads/fetch.rs does. Then it asks ffprobe about
# VIDEO with the arguments of probe_media in crates/libs/media/src/probe.rs,
# converts VIDEO with ffmpeg the two ways the media pass encodes a video in
# crates/libs/media/src/process.rs (H.264 for Convert, HEVC for Compress),
# and asks ffprobe what each wrote. Those arguments are copies: a change to
# them there is a change here too, and the comments beside them say so.
#
# Usage: scripts/check-windows-arm-ffmpeg.sh VIDEO
set -euo pipefail

video=${1:?usage: $0 VIDEO}
root=$(cd "$(dirname "$0")/.." && pwd)
pins="$root/src-tauri/src/tool_downloads/pins.rs"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() {
  echo "check-windows-arm-ffmpeg: $*" >&2
  exit 1
}

# The value of `const NAME: &str = "value";` in pins.rs.
const_str() {
  sed -n "s/^const $1: &str = \"\\(.*\\)\";\$/\\1/p" "$pins"
}

repo=$(const_str FFMPEG_STATIC)
release=$(const_str FFMPEG_RELEASE)
[ -n "$repo" ] && [ -n "$release" ] || fail "no FFMPEG_STATIC or FFMPEG_RELEASE in $pins"

# Every `FfmpegFiles { ... }` in pins.rs, one line each:
#   name|os|arch|base|ffmpeg asset|.gz sha|program sha|ffprobe asset|.gz sha|program sha
# `name` is the const the block is, if any, and `base` the const a
# `..BASE` spreads from, if any. A field the block leaves out is empty.
blocks=$(awk '
  function field(line) { sub(/^[^"]*"/, "", line); sub(/".*$/, "", line); return line }
  /FfmpegFiles \{/ {
    inside = 1; name = ""; os = ""; arch = ""; base = ""; tuple = ""
    ff[1] = ff[2] = ff[3] = fp[1] = fp[2] = fp[3] = ""; nf = np = 0
    if (match($0, /const [A-Z0-9_]+:/)) name = substr($0, RSTART + 6, RLENGTH - 7)
    next
  }
  !inside { next }
  /^[[:space:]]*os: "/ { os = field($0); next }
  /^[[:space:]]*arch: "/ { arch = field($0); next }
  /^[[:space:]]*\.\.[A-Z0-9_]+/ { base = $0; gsub(/[[:space:].,]/, "", base); next }
  /^[[:space:]]*ffmpeg: \(/ { tuple = "ffmpeg"; next }
  /^[[:space:]]*ffprobe: \(/ { tuple = "ffprobe"; next }
  /^[[:space:]]*\),?[[:space:]]*$/ { tuple = ""; next }
  tuple == "ffmpeg" && /"/ { ff[++nf] = field($0); next }
  tuple == "ffprobe" && /"/ { fp[++np] = field($0); next }
  /^[[:space:]]*\}[,;]?[[:space:]]*$/ {
    inside = 0
    print name "|" os "|" arch "|" base "|" ff[1] "|" ff[2] "|" ff[3] "|" fp[1] "|" fp[2] "|" fp[3]
  }
' "$pins")

# The block for windows/aarch64, with what it spreads from filled in.
entry=""
while IFS='|' read -r name os arch base f1 f2 f3 p1 p2 p3; do
  [ "$arch" = aarch64 ] || continue
  if [ -n "$base" ]; then
    from=$(printf '%s\n' "$blocks" | awk -F'|' -v n="$base" '$1 == n')
    [ -n "$from" ] || fail "pins.rs spreads ..$base, which is not an FfmpegFiles const"
    IFS='|' read -r _ bos _ _ bf1 bf2 bf3 bp1 bp2 bp3 <<<"$from"
    os=${os:-$bos} f1=${f1:-$bf1} f2=${f2:-$bf2} f3=${f3:-$bf3}
    p1=${p1:-$bp1} p2=${p2:-$bp2} p3=${p3:-$bp3}
  fi
  [ "$os" = windows ] || continue
  [ -z "$entry" ] || fail "pins.rs has more than one windows/aarch64 entry"
  entry="$f1|$f2|$f3|$p1|$p2|$p3"
done <<<"$blocks"
[ -n "$entry" ] || fail "pins.rs has no windows/aarch64 entry in FFMPEG_FILES"
IFS='|' read -r ffmpeg_asset ffmpeg_gz_sha ffmpeg_sha ffprobe_asset ffprobe_gz_sha ffprobe_sha <<<"$entry"
for value in "$ffmpeg_asset" "$ffmpeg_gz_sha" "$ffmpeg_sha" "$ffprobe_asset" "$ffprobe_gz_sha" "$ffprobe_sha"; do
  [ -n "$value" ] || fail "the windows/aarch64 entry in pins.rs is missing a file or a SHA-256"
done

# Download ASSET, check the .gz and the program it unpacks to against their
# pinned SHA-256, and write the program to OUT.
fetch() {
  local asset=$1 gz_sha=$2 program_sha=$3 out=$4
  local url="https://github.com/$repo/releases/download/$release/$asset"
  echo "Downloading $url"
  curl -sSfL --retry 3 -o "$work/$asset" "$url"
  echo "$gz_sha  $work/$asset" | sha256sum -c -
  gzip -dc "$work/$asset" >"$out"
  echo "$program_sha  $out" | sha256sum -c -
  chmod +x "$out"
}

fetch "$ffmpeg_asset" "$ffmpeg_gz_sha" "$ffmpeg_sha" "$work/ffmpeg.exe"
fetch "$ffprobe_asset" "$ffprobe_gz_sha" "$ffprobe_sha" "$work/ffprobe.exe"

"$work/ffmpeg.exe" -hide_banner -version | head -n 1
"$work/ffprobe.exe" -hide_banner -version | head -n 1

# The question the media pass asks about a video before it converts it.
probe() {
  "$work/ffprobe.exe" -v error -select_streams v:0 \
    -show_entries stream=codec_name,width,height,avg_frame_rate,bit_rate \
    -of csv=p=0 "$1" | tr -d '\r' | { grep -v '^[[:space:]]*$' || true; } | head -n 1
}

before=$(probe "$video")
echo "ffprobe on $video: $before"
[ -n "$before" ] || fail "ffprobe reported no video stream in $video"

# Convert VIDEO to NAME.mp4 with the encoder arguments that follow, and
# check that ffprobe reports CODEC for what ffmpeg wrote.
convert() {
  local name=$1 codec=$2
  shift 2
  local out="$work/$name.mp4"
  "$work/ffmpeg.exe" -hide_banner -y -i "$video" "$@" "$out"
  local after
  after=$(probe "$out")
  echo "ffprobe on the $name file: $after"
  case "$after" in
    "$codec",*) ;;
    *) fail "the $name file is not $codec: '$after'" ;;
  esac
}

# Convert's re-encode (convert_video): H.264 at 30 frames a second, AAC.
convert converted h264 -vf fps=30 -c:v libx264 -crf 23 -preset medium \
  -c:a aac -b:a 128k -movflags +faststart

# Compress's encode (compress_video) at its 720p setting: HEVC, AAC.
convert compressed hevc \
  -vf "scale='if(gt(iw,ih),min(1280,iw),-2)':'if(gt(iw,ih),-2,min(1280,ih))'" \
  -c:v libx265 -crf 22 -preset medium -tag:v hvc1 -c:a aac -b:a 128k \
  -movflags +faststart

echo "ffmpeg and ffprobe run on this computer."
