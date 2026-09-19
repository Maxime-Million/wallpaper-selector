#!/usr/bin/env bash
#
# Installation de wallpaper-selector.
#
#   ./install.sh                    compile, installe dans ~/.local/bin, construit le cache
#   ./install.sh --service          installe en plus le service de restauration au démarrage
#   ./install.sh --no-cache         saute la construction du cache de vignettes
#   ./install.sh --no-build         réinstalle un binaire déjà compilé
#   ./install.sh --bin-dir DIR      autre destination
#   ./install.sh --keybind "SUPER, RETURN"
#
# `--service` installe un service systemd utilisateur qui rejoue le dernier fond
# d'écran à chaque ouverture de session
#
set -euo pipefail

BIN_NAME="wallpaper-selector"
BIN_DIR="${HOME}/.local/bin"
KEYBIND="SUPER, W"
SKIP_BUILD=0
SKIP_CACHE=0
WITH_SERVICE=0

usage() {
    sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)     usage ;;
        --no-build)    SKIP_BUILD=1; shift ;;
        --no-cache)    SKIP_CACHE=1; shift ;;
        --service)     WITH_SERVICE=1; shift ;;
        --bin-dir)     BIN_DIR="${2:?--bin-dir attend un chemin}"; shift 2 ;;
        --keybind)     KEYBIND="${2:?--keybind attend une combinaison}"; shift 2 ;;
        *) echo "option inconnue : $1" >&2; usage ;;
    esac
done

cd "$(dirname "$(readlink -f "$0")")"

# --- 1. Compilation -------------------------------------------------------
if [ "$SKIP_BUILD" -eq 0 ]; then
    echo "→ compilation (release)…"
    cargo build --release
fi

if [ ! -x "target/release/${BIN_NAME}" ]; then
    echo "erreur : target/release/${BIN_NAME} est absent, relancez sans --no-build" >&2
    exit 1
fi

# --- 2. Binaire -----------------------------------------------------------
install -Dm755 "target/release/${BIN_NAME}" "${BIN_DIR}/${BIN_NAME}"
echo "→ installé : ${BIN_DIR}/${BIN_NAME}"

case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) echo "⚠  ${BIN_DIR} n'est pas dans votre PATH."
       echo "   Ajoutez  export PATH=\"${BIN_DIR}:\$PATH\"  à votre ~/.bashrc ou ~/.zshrc," >&2
       echo "   ou relancez avec  --bin-dir ~/.cargo/bin  si ce dossier est déjà dans le PATH." >&2 ;;
esac

# --- 3. Cache de vignettes ------------------------------------------------
if [ "$SKIP_CACHE" -eq 0 ]; then
    echo "→ construction du cache de vignettes (une seule fois)…"
    "${BIN_DIR}/${BIN_NAME}" --rebuild-cache
fi

# --- 4. Restauration du fond d'écran au démarrage -------------------------
# Le script est toujours installé : il sert aussi bien au service systemd
# (--service) qu'à une ligne exec-once dans la configuration Hyprland.
install -Dm755 "scripts/init-wallpaper.sh" "${BIN_DIR}/init-wallpaper.sh"
echo "→ installé : ${BIN_DIR}/init-wallpaper.sh"

SERVICE_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
if [ "$WITH_SERVICE" -eq 1 ]; then
    mkdir -p "$SERVICE_DIR"
    sed "s|@BIN_DIR@|${BIN_DIR}|g" \
        "scripts/wallpaper-selector-restore.service" \
        > "${SERVICE_DIR}/wallpaper-selector-restore.service"

    echo "→ service installé : ${SERVICE_DIR}/wallpaper-selector-restore.service"

    if command -v systemctl >/dev/null 2>&1; then
        systemctl --user daemon-reload
        systemctl --user enable wallpaper-selector-restore.service
        echo "→ service activé. Désactiver : systemctl --user disable wallpaper-selector-restore.service"
    else
        echo "⚠  systemctl introuvable : activez le service à la main." >&2
    fi
fi

# --- 5. Récapitulatif -----------------------------------------------------
cat <<EOF

Il reste une ligne à ajouter dans ~/.config/hypr/conf/keybindings.lua

    hl.bind(mainMod .. " + W", hl.dsp.exec_cmd("${BIN_NAME} --toggle"))

puis recharger Hyprland :

    hyprctl reload

Essai immédiat, sans raccourci :

    ${BIN_NAME} --toggle

EOF

if [ "$WITH_SERVICE" -eq 1 ]; then
cat <<EOF
Le fond d'écran est restauré au démarrage par le service systemd : vous pouvez
retirer la ligne « exec-once » / hl.exec_cmd qui appelait init-wallpaper.sh.

Vérifier l'état du service :

    systemctl --user status wallpaper-selector-restore.service

EOF
else
cat <<EOF
Pour restaurer le fond d'écran au démarrage, deux possibilités :

  - l'appeler depuis la configuration Hyprland :
        exec-once = ${BIN_DIR}/init-wallpaper.sh
  - ou installer le service systemd:
        ./install.sh --service

EOF
fi
