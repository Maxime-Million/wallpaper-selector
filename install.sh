#!/usr/bin/env bash
#
# Installation de wallpaper-selector.
#
#   ./install.sh                  compile, installe dans ~/.local/bin, construit le cache
#   ./install.sh --no-cache       saute la construction du cache de vignettes
#   ./install.sh --no-build       réinstalle un binaire déjà compilé
#   ./install.sh --bin-dir DIR    autre destination
#   ./install.sh --keybind "SUPER, RETURN"
#
set -euo pipefail

BIN_NAME="wallpaper-selector"
BIN_DIR="${HOME}/.local/bin"
KEYBIND="SUPER, W"
SKIP_BUILD=0
SKIP_CACHE=0

usage() {
    sed -n '2,12p' "$0" | sed 's/^# \?//'
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)     usage ;;
        --no-build)    SKIP_BUILD=1; shift ;;
        --no-cache)    SKIP_CACHE=1; shift ;;
        --bin-dir)     BIN_DIR="${2:?--bin-dir attend un chemin}"; shift 2 ;;
        --keybind)     KEYBIND="${2:?--keybind attend une combinaison}"; shift 2 ;;
        *) echo "option inconnue : $1" >&2; usage ;;
    esac
done

cd "$(dirname "$(readlink -f "$0")")"

if [ "$SKIP_BUILD" -eq 0 ]; then
    echo "→ compilation (release)…"
    cargo build --release
fi

if [ ! -x "target/release/${BIN_NAME}" ]; then
    echo "erreur : target/release/${BIN_NAME} est absent, relancez sans --no-build" >&2
    exit 1
fi

install -Dm755 "target/release/${BIN_NAME}" "${BIN_DIR}/${BIN_NAME}"
echo "→ installé : ${BIN_DIR}/${BIN_NAME}"

case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) echo "⚠  ${BIN_DIR} n'est pas dans votre PATH."
       echo "   Ajoutez  export PATH=\"${BIN_DIR}:\$PATH\"  à votre ~/.bashrc ou ~/.zshrc," >&2
       echo "   ou relancez avec  --bin-dir ~/.cargo/bin  si ce dossier est déjà dans le PATH." >&2 ;;
esac

if [ "$SKIP_CACHE" -eq 0 ]; then
    echo "→ construction du cache de vignettes (une seule fois)…"
    "${BIN_DIR}/${BIN_NAME}" --rebuild-cache
fi

cat <<EOF

Il reste une ligne à ajouter dans ~/.config/hypr/hyprland.conf :

    hl.bind(mainMod .. " + W", hl.dsp.exec_cmd("${BIN_NAME} --toggle"))

puis recharger Hyprland :

    hyprctl reload

Essai immédiat sans raccourci :

    ${BIN_NAME} --toggle

EOF
