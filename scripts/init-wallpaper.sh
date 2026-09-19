#!/usr/bin/env bash
#
# init-wallpaper.sh — restaure le dernier fond d'écran appliqué.
#
# Appelé au démarrage de la session, il relit ~/.cache/last_wallpaper_id et
# relance le moteur correspondant (awww, mpvpaper ou linux-wallpaperengine).
#
# Il peut être lancé de deux façons :
#
#   1. depuis la configuration Hyprland (exec-once / hl.exec_cmd) ;
#   2. depuis le service systemd utilisateur fourni
#      (scripts/wallpaper-selector-restore.service) — dans ce cas les variables
#      de la session Wayland ne sont pas transmises au service : le script les
#      retrouve lui-même, en attendant que le compositeur soit prêt.
#
# Variables reconnues :
#   WALLPAPER_SELECTOR_BIN     chemin du binaire (sinon recherche dans le PATH)
#   WALLPAPER_RESTORE_TIMEOUT  attente du compositeur, en secondes (défaut : 30)
#   WALLPAPER_CACHE_FILE       autre emplacement du fichier de cache
#   MONITOR                    écran cible du repli linux-wallpaperengine
#
set -euo pipefail

CACHE_FILE="${WALLPAPER_CACHE_FILE:-$HOME/.cache/last_wallpaper_id}"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
TIMEOUT="${WALLPAPER_RESTORE_TIMEOUT:-30}"

log() { printf '[init-wallpaper] %s\n' "$*" >&2; }

# --- 1. Retrouver la session Wayland -------------------------------------
# Sous systemd, WAYLAND_DISPLAY et HYPRLAND_INSTANCE_SIGNATURE sont absents de
# l'environnement du service : on les redécouvre à partir du socket.
if [ -z "${WAYLAND_DISPLAY:-}" ]; then
    wayland_socket=""
    for _ in $(seq 1 "$TIMEOUT"); do
        for sock in "$RUNTIME_DIR"/wayland-[0-9]*; do
            if [ -S "$sock" ]; then
                wayland_socket="${sock##*/}"
                break
            fi
        done
        [ -n "$wayland_socket" ] && break
        sleep 1
    done

    if [ -z "$wayland_socket" ]; then
        log "aucun compositeur Wayland après ${TIMEOUT}s — rien à restaurer."
        exit 0
    fi
    export WAYLAND_DISPLAY="$wayland_socket"
fi

# hyprctl (position du curseur, écran focalisé) a besoin de cette signature.
if [ -z "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]; then
    for dir in "$RUNTIME_DIR"/hypr/*/; do
        if [ -S "${dir}.socket.sock" ]; then
            export HYPRLAND_INSTANCE_SIGNATURE="$(basename "$dir")"
            break
        fi
    done
fi
export XDG_SESSION_TYPE="${XDG_SESSION_TYPE:-wayland}"

# --- 2. Déterminer le binaire -------------------------------------------
BIN="${WALLPAPER_SELECTOR_BIN:-}"
if [ -z "$BIN" ]; then
    BIN="$(command -v wallpaper-selector || true)"
fi
if [ -z "$BIN" ] && [ -x "$HOME/.local/bin/wallpaper-selector" ]; then
    BIN="$HOME/.local/bin/wallpaper-selector"
fi

if [ -n "$BIN" ]; then
    # Premier démarrage après une installation neuve : rien à restaurer. On
    # sort en succès pour ne pas marquer le service systemd en échec.
    if [ ! -f "$CACHE_FILE" ]; then
        log "aucun fond d'écran enregistré — rien à restaurer."
        exit 0
    fi
    exec "$BIN" --restore
fi

# --- 3. Repli sans le binaire -------------------------------------------
# Reprend la logique du script bash d'origine, pour rester utilisable même si
# wallpaper-selector n'est pas installé.
log "wallpaper-selector introuvable, repli sur le script d'origine."

if [ ! -f "$CACHE_FILE" ]; then
    log "aucun fond d'écran enregistré dans $CACHE_FILE."
    awww-daemon --format argb &
    exit 0
fi

TYPE="$(cut -d'|' -f1 "$CACHE_FILE")"
TARGET="$(cut -d'|' -f2- "$CACHE_FILE")"

case "$TYPE" in
    LWE)
        pkill awww 2>/dev/null || true
        pkill -f mpvpaper 2>/dev/null || true
        linux-wallpaperengine "$TARGET" --screen-root "${MONITOR:-eDP-1}" &
        ;;
    MPV)
        pkill awww 2>/dev/null || true
        pkill -f linux-wallpaperengine 2>/dev/null || true
        mpvpaper -o "no-audio --loop-playlist" '*' "$TARGET" &
        ;;
    AWWW)
        pkill -f mpvpaper 2>/dev/null || true
        pkill -f linux-wallpaperengine 2>/dev/null || true
        pgrep -x awww-daemon >/dev/null || awww-daemon --format argb &
        awww img "$TARGET" &
        ;;
    *)
        log "type inconnu « $TYPE » dans $CACHE_FILE."
        exit 1
        ;;
esac
