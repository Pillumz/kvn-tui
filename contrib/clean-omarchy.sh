#!/bin/bash
set -e

BACKUP_SUFFIX=".bak.before-kvn-tui"
FILES=(
  "${HOME}/.config/omarchy/shell.json"
  "${HOME}/.config/hypr/bindings.lua"
  "${HOME}/.config/hypr/hyprland.lua"
  "${HOME}/.config/waybar/config.jsonc"
  "${HOME}/.config/waybar/style.css"
  "${HOME}/.config/hypr/autostart.conf"
  "${HOME}/.config/hypr/bindings.conf"
  "${HOME}/.config/hypr/hyprland.conf"
)

removed=0
for file in "${FILES[@]}"; do
  backup="${file}${BACKUP_SUFFIX}"
  if [ -f "$backup" ]; then
    rm -- "$backup"
    echo "Removed $backup"
    removed=$((removed + 1))
  fi
done

if [ "$removed" -eq 0 ]; then
  echo "No kvn-tui Omarchy backup files found."
else
  echo "Removed $removed kvn-tui Omarchy backup file(s)."
fi
