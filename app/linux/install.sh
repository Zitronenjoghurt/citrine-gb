#!/usr/bin/env sh
# Installs Citrine into the per-user XDG locations. No root required.
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"

install -Dm755 "$HERE/citrine"          "$HOME/.local/bin/citrine"
install -Dm644 "$HERE/citrine.desktop"  "$HOME/.local/share/applications/citrine.desktop"
install -Dm644 "$HERE/citrine.png"      "$HOME/.local/share/icons/hicolor/256x256/apps/citrine.png"

update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
gtk-update-icon-cache -qtf "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

echo "Citrine installed to ~/.local/bin/citrine"
case ":$PATH:" in
	*":$HOME/.local/bin:"*) ;;
	*) echo "Note: ~/.local/bin is not on your PATH; add it to launch from a terminal." ;;
esac
