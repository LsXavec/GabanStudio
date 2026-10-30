#!/usr/bin/env bash
# GM19 smoke render: the RELEASE exe renders the in-memory sample cut with no
# window (the GM11 batch render, `--render --sample`) under three delivery
# presets, each into its own folder of a temp dir, and ffprobe checks what
# came out:
#   Master PNG 16-bit  exactly the sample's frame count of PNG files
#   ProRes 422 HQ      one .mov: video prores, that many frames, one pcm_s24le audio stream
#   H.264 review       one .mp4: video h264, that many frames, one aac audio stream
# ffmpeg and ffprobe are found the way the app's find_ffmpeg finds ffmpeg
# (PATH, then the winget Gyan.FFmpeg package dir); missing ones are a clear
# failure, never a skip. Nothing here starts the GUI, an installer or a
# browser, and the temp dir goes on exit. Run from anywhere; exit 0 = pass.
set -euo pipefail

cd "$(dirname "$0")/.."
EXE=target/release/animstudio.exe
# app/src/sample.rs FRAMES: the sample cut's length.
FRAMES=$(sed -n 's/^pub const FRAMES: u32 = \([0-9]*\);$/\1/p' app/src/sample.rs)

fail() {
    echo "smoke: FAIL — $*" >&2
    exit 1
}

[ -n "$FRAMES" ] || fail "could not read FRAMES from app/src/sample.rs"
[ -f "$EXE" ] || fail "$EXE is missing — run cargo build --release -p animstudio first"

# PATH first, then %LOCALAPPDATA%\Microsoft\WinGet\Packages\Gyan.FFmpeg_*\ffmpeg-*\bin.
find_tool() {
    local name=$1 found
    found=$(command -v "$name" 2>/dev/null || true)
    if [ -z "$found" ] && [ -n "${LOCALAPPDATA:-}" ]; then
        for bin in "$LOCALAPPDATA"/Microsoft/WinGet/Packages/Gyan.FFmpeg*/ffmpeg-*/bin; do
            if [ -f "$bin/$name.exe" ]; then
                found="$bin/$name.exe"
                break
            fi
        done
    fi
    [ -n "$found" ] || fail "$name not found on PATH or in the winget Gyan.FFmpeg package dir — install ffmpeg (winget install Gyan.FFmpeg)"
    echo "$found"
}
FFPROBE=$(find_tool ffprobe)
FFMPEG=$(find_tool ffmpeg)
echo "smoke: ffmpeg  $FFMPEG"
echo "smoke: ffprobe $FFPROBE"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
# The exe is a Windows program: hand it a Windows path.
win() {
    if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else echo "$1"; fi
}

render() {
    local preset=$1 dir=$2
    mkdir -p "$TMP/$dir"
    echo "smoke: rendering \"$preset\" into $dir/"
    "$EXE" --render --sample --preset "$preset" --out "$(win "$TMP/$dir")" ||
        fail "the batch render of \"$preset\" exited $?"
}

probe() {
    "$FFPROBE" -v error "$@" | tr -d '\r'
}

# One container file of the extension in the folder; its checks.
check_video() {
    local dir=$1 ext=$2 vcodec=$3 acodec=$4 files file got
    files=$(find "$TMP/$dir" -maxdepth 1 -name "*.$ext")
    [ "$(echo "$files" | grep -c .)" -eq 1 ] || fail "$dir/ holds $(echo "$files" | grep -c .) .$ext files, not 1"
    file=$(win "$files")
    got=$(probe -select_streams v:0 -show_entries stream=codec_name -of default=nw=1:nk=1 "$file")
    [ "$got" = "$vcodec" ] || fail "$dir/*.$ext video codec is \"$got\", not $vcodec"
    got=$(probe -count_frames -select_streams v:0 -show_entries stream=nb_read_frames -of default=nw=1:nk=1 "$file")
    [ "$got" = "$FRAMES" ] || fail "$dir/*.$ext has $got frames, not $FRAMES"
    got=$(probe -select_streams a -show_entries stream=codec_name -of default=nw=1:nk=1 "$file")
    [ "$(echo "$got" | grep -c .)" -eq 1 ] || fail "$dir/*.$ext has $(echo "$got" | grep -c .) audio streams, not 1"
    [ "$got" = "$acodec" ] || fail "$dir/*.$ext audio codec is \"$got\", not $acodec"
    echo "smoke: $dir/ ok — $vcodec, $FRAMES frames, one $acodec audio stream"
}

render "Master PNG 16-bit" png
pngs=$(find "$TMP/png" -maxdepth 1 -name '*.png' | grep -c . || true)
[ "$pngs" -eq "$FRAMES" ] || fail "png/ holds $pngs PNG files, not $FRAMES"
echo "smoke: png/ ok — $pngs PNG files"

render "ProRes 422 HQ" prores
check_video prores mov prores pcm_s24le

render "H.264 review" h264
check_video h264 mp4 h264 aac

echo "SMOKE PASS: PNG16 $FRAMES files, ProRes and H.264 with $FRAMES frames and one audio stream each"
