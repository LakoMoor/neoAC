#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Builds rustyac-play (macOS / Linux) and puts it, with a launcher, into the game's folder
# (default: ./ac, the folder with `content` and `system`), so that the game starts from there:
#   sh packaging/package_unix.sh [<game folder>]
# (builds the game and its GPUI launcher) then double-click `rustyac.command` (macOS) or run ./rustyac.sh
set -e
cd "$(dirname "$0")/.."
target="${1:-ac}"
[ -d "$target/content" ] || { echo "$target has no content folder: give the game's folder" >&2; exit 1; }
cargo build --release -p rustyac-native -p rustyac-launcher-gpui
cp target/release/rustyac-play "$target/rustyac-play"
cp target/release/rustyac-launch "$target/rustyac-launch"
cat > "$target/rustyac.sh" <<'LAUNCH'
#!/bin/sh
# rustyAC: the launcher (car, track, skins, assists, keys, settings) when started without options;
# with options (--car <id> --track <id> --layout <l> --race <race.ini> --join <host:port> ...) the game directly
cd "$(dirname "$0")"
if [ $# -eq 0 ] && [ -x ./rustyac-launch ]; then exec ./rustyac-launch --ac-root "$PWD"; fi
exec ./rustyac-play "$@"
LAUNCH
cp "$target/rustyac.sh" "$target/rustyac.command"
chmod +x "$target/rustyac.sh" "$target/rustyac.command" "$target/rustyac-play" "$target/rustyac-launch"
echo "ready: $target/rustyac.command (double-click on macOS) or $target/rustyac.sh"
