# wallpaper-selector

**Un sélecteur de fonds d'écran instantané pour Hyprland.**

Ouvrez une fenêtre, tapez quelques lettres, appuyez sur `Entrée` : le fond
d'écran est appliqué. La fenêtre liste **tous** vos fonds d'écran au même
endroit — les scènes du Workshop de Wallpaper Engine et vos propres images et
vidéos — avec une vignette, la possibilité de filtrer par type de fichier et
d'épingler vos préférés en tête de liste.

Il fait le même travail qu'un script `rofi`/`fuzzel`, en beaucoup plus rapide :
la fenêtre s'ouvre en **~25 ms** au lieu de ~0,5-1 s, parce qu'elle ne relit pas
652 Mo d'images à chaque ouverture.

```
╭─────────────────────────────────────────────────────────────────────────╮
│  Rechercher…                                                       3/47  │
├─────────────────────────────────────────────────────────────────────────┤
│  ★ FAVORIS                                                              │
│  ★  pond_shed.png          perso · png · perso                      ★  │
│  ★  Cabin                  perso · jpg · perso                      ★  │
│  TOUS LES FONDS D'ÉCRAN                                                 │
│     Dragon Bones           1355236618 · pkg · steam                     │
│  ▸  Ghost of Tsushima      1223062915 · mp4 · steam                     │
│     neocity.png            perso · png · perso                          │
╰─────────────────────────────────────────────────────────────────────────╯
   ↑↓ naviguer   Entrée appliquer   Tab favori   Ctrl+T filtre   Esc fermer
```

---

## Sommaire

- [À quoi ça sert](#à-quoi-ça-sert)
- [Installation](#installation)
- [Utilisation au quotidien](#utilisation-au-quotidien)
- [Raccourcis de l'interface](#raccourcis-de-linterface)
- [Rechercher](#rechercher)
- [Favoris](#favoris)
- [Fond d'écran au démarrage](#fond-décran-au-démarrage)
- [Configuration](#configuration)
- [Ligne de commande](#ligne-de-commande)
- [Dépannage](#dépannage)
- [Pourquoi c'est rapide](#pourquoi-cest-rapide)
- [Notes pour les développeurs](#notes-pour-les-développeurs)

---

## À quoi ça sert

Vous avez probablement plusieurs dossiers de fonds d'écran qui ne se parlent
pas : le Workshop de Wallpaper Engine d'un côté, vos images et vidéos de
l'autre. `wallpaper-selector` les rassemble dans une seule liste et s'occupe
d'appliquer le bon moteur selon ce que vous choisissez :

| Vous choisissez… | Ce qui est lancé |
|---|---|
| une scène Wallpaper Engine (`.pkg`) | `linux-wallpaperengine` |
| une vidéo (`.mp4`, `.webm`, `.mkv`, `.mov`) | `mpvpaper` |
| une image ou un GIF | `awww` |

Concrètement :

- **Une seule liste** pour toutes vos sources.
- **Des vignettes** : une fois générées, elles s'affichent instantanément.
- **Recherche en direct** en tapant au clavier, plus des filtres par type
  (`#png`, `#video`, `#pkg`…).
- **Des favoris** : `Tab` épingle un fond d'écran, il remonte dans une section
  « ★ Favoris » en haut de la liste.
- **Une restauration au démarrage** : le fond d'écran choisi est rejoué
  automatiquement à chaque connexion, via Hyprland **ou** via un service
  systemd utilisateur (voir [plus bas](#fond-décran-au-démarrage)).

---

## Installation

### Ce qu'il faut

- **Hyprland** (l'outil a besoin de `hyprctl` pour connaître la position du
  curseur et l'écran focalisé).
- **GTK 4 ≥ 4.12**, **gtk4-layer-shell ≥ 1.0**, **gdk-pixbuf**, **Rust ≥ 1.80**.
- Les moteurs que vous utilisez : `awww`, `mpvpaper`,
  `linux-wallpaperengine`. Seuls ceux qui vous servent sont nécessaires ; les
  autres sont simplement ignorés.

Sur Arch :

```bash
sudo pacman -S --needed gtk4 gtk4-layer-shell rust awww mpvpaper
```

(`linux-wallpaperengine` s'installe depuis les [dépôts du projet
FlameComics](https://github.com/Almamu/linux-wallpaperengine) ou l'AUR.)

### Installer l'application

```bash
cd wallpaper-selector
./install.sh
```

Le script compile, installe dans `~/.local/bin` le binaire et le script
`init-wallpaper.sh` ([restauration du fond d'écran au démarrage](#fond-décran-au-démarrage)),
génère une fois pour toutes le cache de vignettes (~15 s sur 458 fonds d'écran)
et affiche la ligne à recopier dans votre configuration Hyprland.

Options utiles :

```bash
./install.sh --service              # + restauration au démarrage via systemd
./install.sh --no-cache             # saute la génération des vignettes
./install.sh --bin-dir ~/.cargo/bin # autre destination
```

À la main, si vous préférez :

```bash
cargo build --release
install -Dm755 target/release/wallpaper-selector ~/.local/bin/wallpaper-selector
wallpaper-selector --rebuild-cache
```

### Ajouter le raccourci

Dans votre configuration Hyprland, ajoutez une ligne qui lance
`wallpaper-selector --toggle` :

```lua
-- ~/.config/hypr/conf/keybindings.lua (Hyprland ≥ 0.5x, config Lua)
local mainMod = "SUPER"
hl.bind(mainMod .. " + W", hl.dsp.exec_cmd("wallpaper-selector --toggle"))
```

Puis `hyprctl reload`.

C'est tout. Le premier appui démarre l'application en tâche de fond, les
suivants ne font qu'afficher ou masquer la fenêtre.

---

## Utilisation au quotidien

`SUPER + W` ouvre la fenêtre. Vous tapez pour filtrer, `↑`/`↓` pour vous
déplacer, `Entrée` pour appliquer, `Échap` pour fermer.

La première fois, le processus reste en mémoire : réappuyer sur le raccourci
fait réapparaître la fenêtre **quasi instantanément**, parce que l'index et les
vignettes sont déjà chargés. Deux conséquences à connaître :

- La commande rend la main tout de suite, elle ne bloque pas le terminal : le
  processus résident vit en arrière-plan.
- Pour l'arrêter complètement : `wallpaper-selector --quit`. Sa sortie
  d'erreur est dans `~/.cache/wallpaper-selector/daemon.log`.

Si vous ne voulez pas d'un processus en arrière-plan, mettez `resident = false`
dans la [configuration](#configuration) : l'application se comporte alors comme
un programme classique, qui s'ouvre et se ferme.

---

## Raccourcis de l'interface

| Touche | Action |
|---|---|
| `↑` `↓` | Naviguer (les en-têtes de section sont sautés) |
| `Ctrl+N` / `Ctrl+P` | Idem, façon Emacs |
| `Page↑` ou `Début` | Remonter tout en haut des résultats |
| `Page↓` ou `Fin` | Descendre tout en bas des résultats |
| `Entrée` | Appliquer le fond d'écran sélectionné |
| `Tab` ou `Ctrl+F` | Épingler / désépingler le favori |
| `Ctrl+T` | Faire tourner le filtre de type |
| `Échap` | Fermer |

La frappe au clavier filtre la liste en direct.

---

## Rechercher

Tout ce que vous tapez est comparé au titre, à l'extension et à la provenance.
Un jeton préfixé par `#` filtre en plus sur le **type réel** du fichier, sans
participer à la recherche textuelle :

| Jeton | Ce qu'il sélectionne |
|---|---|
| `#png` `#jpg` `#jpeg` `#webp` `#avif` `#bmp` `#tiff` | cette extension |
| `#pkg` (ou `#scene`) | scènes Wallpaper Engine |
| `#mp4` `#webm` `#mkv` `#mov` | cette extension |
| `#video` | toutes les vidéos |
| `#gif` (ou `#anim`) | GIF animés |
| `#img` (ou `#image` `#photo`) | toutes les images |
| `#steam` / `#perso` | provenance, pas extension |

```
#png montagne        les PNG dont le titre contient « montagne »
#png #jpg            les PNG et les JPEG (les filtres se cumulent en OU)
#video               toutes les vidéos
```

`Ctrl+T` fait tourner le filtre sans rien taper : `#pkg`, puis `#mp4`, `#gif`,
`#img`, `#perso`, `#steam`, puis retire le filtre. Le filtre actif reste
toujours visible dans le champ de recherche et dans la barre d'état en bas à
droite — rien n'est jamais filtré en cachette.

---

## Favoris

`Tab` sur une entrée l'épingle. Les favoris apparaissent dans une section
**★ Favoris** en tête de liste, suivie de **Tous les fonds d'écran**.

Ils sont stockés dans `~/.config/wallpaper-selector/favorites` : un fichier
texte, une ligne par entrée, où chaque identifiant porte sa variable
« favori : oui/non ». Il est lisible et éditable à la main.

```
# Wallpaper Selector — favoris
perso:neocity.png       → favori : oui
steam:1355236618        → favori : oui
!perso:kirokaze.gif     → favori : non (explicite, prioritaire)
```

L'identifiant d'une entrée est `steam:<id du Workshop>` ou `perso:<nom du
fichier>`. On l'obtient avec `--print` ou `--json`. Le fichier est aussi
modifiable depuis un script :

```bash
wallpaper-selector --favorite perso:cabin.png
wallpaper-selector --unfavorite perso:cabin.png
wallpaper-selector --favorites          # liste les identifiants favoris
wallpaper-selector --json | jq '.[] | select(.favorite)'
```

---

## Fond d'écran au démarrage

Un ordinateur ne se souvient pas de votre fond d'écran : à chaque connexion, il
faut le réappliquer. `wallpaper-selector` enregistre votre choix dans
`~/.cache/last_wallpaper_id` et sait le rejouer avec :

```bash
wallpaper-selector --restore
```

Reste à appeler cette commande au bon moment. Deux méthodes, au choix.

### Méthode A — depuis Hyprland

Dans votre configuration Hyprland (Lua) :

```lua
-- ~/.config/hypr/conf/auto_start.lua
hl.on("hyprland.start", function()
  hl.exec_cmd("~/.local/bin/init-wallpaper.sh &")
end)
```

Sur une configuration classique :

```
exec-once = ~/.local/bin/init-wallpaper.sh
```

`init-wallpaper.sh` est installé par `./install.sh`, en même temps que le
binaire.

### Méthode B — service systemd utilisateur (sans toucher à Hyprland)

```bash
./install.sh --service
```

Cela installe `scripts/init-wallpaper.sh` et active
`wallpaper-selector-restore.service` dans votre session systemd :
le fond d'écran est restauré à chaque connexion.

```bash
systemctl --user status wallpaper-selector-restore.service
systemctl --user restart wallpaper-selector-restore.service   # pour rejouer
systemctl --user disable --now wallpaper-selector-restore.service  # annuler
```

Le script a été écrit pour ce cas : un service systemd ne reçoit pas les
variables de la session Wayland (`WAYLAND_DISPLAY`,
`HYPRLAND_INSTANCE_SIGNATURE`). Il les retrouve donc lui-même en attendant que
le compositeur soit prêt, puis appelle `wallpaper-selector --restore`. Si le
binaire n'est pas installé, il retombe sur l'ancienne logique bash.

> Après un redémarrage de Hyprland **sans** vous reconnecter, le service est
> déjà « actif » et ne se relance pas tout seul : `systemctl --user restart
> wallpaper-selector-restore.service` si besoin.

---

## Configuration

`~/.config/wallpaper-selector/config`, au format `clé = valeur`. Toutes les clés
sont optionnelles — le fichier `config.example` du dépôt liste les valeurs par
défaut :

```ini
# Sources
steam_dir = ~/.local/share/Steam/steamapps/workshop/content/431960
perso_dir = ~/Images/wallpapers

# Fenêtre
width_pct  = 35      # largeur, en % de l'écran
height_pct = 55      # hauteur, en % de l'écran
thumb_px   = 80      # taille des vignettes à l'écran

# Application du fond d'écran
transition_type     = grow    # transition awww (grow, wipe, fade, none…)
transition_duration = 1.2
monitor             = auto    # écran pour linux-wallpaperengine (auto = focalisé)
mpv_output          = *       # sortie mpvpaper (* = tous les écrans)

# Comportement
resident = true               # garder le processus en mémoire pour l'instantanéité
```

Les variables d'environnement `WALLPAPER_STEAM_DIR` et `WALLPAPER_PERSO_DIR`
sont aussi reconnues.

### Thème

`~/.config/wallpaper-selector/style.css` remplace complètement le thème intégré
si le fichier existe. Le thème par défaut reprend les couleurs et la géométrie
du `wallpaper-grid.rasi` d'origine (Catppuccin, carte `#11111b`, bordure
`#b4befe`, sélection `#6277e3`).

---

## Ligne de commande

```
wallpaper-selector [OPTIONS]

  -t, --toggle         bascule la fenêtre (défaut)
  -s, --show           affiche la fenêtre
      --hide           masque la fenêtre
      --reload         relit l'index sans rouvrir la fenêtre
      --restore        réapplique le dernier fond d'écran enregistré
  -q, --quit           arrête le processus résident
      --rebuild-cache  régénère toutes les vignettes
      --print          expose l'index en texte
      --json           expose l'index en JSON (champ « favorite » inclus)
      --favorites      liste les identifiants favoris
  -f, --favorite ID    marque une entrée comme favorite
      --unfavorite ID  retire une entrée des favoris
  -F, --foreground     reste attaché au terminal (débogage)
  -h, --help           affiche l'aide
  -V, --version        affiche la version
```

`--print` et `--json` sont pratiques pour scripter : ils exposent tout l'index
sans ouvrir de fenêtre.

---

## Dépannage

**Le raccourci ne fait rien.**
Vérifiez que le binaire est bien trouvé : `command -v wallpaper-selector`. S'il
n'y a rien, `~/.local/bin` n'est pas dans votre `PATH` — réinstallez avec
`./install.sh --bin-dir ~/.cargo/bin`, ou utilisez le chemin complet dans le
raccourci.

**La fenêtre ne s'affiche pas / l'application plante au démarrage.**
Lancez-la au premier plan pour voir les erreurs :

```bash
wallpaper-selector --show --foreground
```

Sinon, le journal du processus résident est dans
`~/.cache/wallpaper-selector/daemon.log`.

**La fenêtre met une seconde à s'ouvrir.**
Le processus résident est probablement arrêté. Vérifiez avec
`wallpaper-selector --quit` (s'il répond « aucune instance en cours », il n'y en
a pas). Vérifiez aussi que `resident = true` dans la configuration.

**Les vignettes sont floues ou trop grandes.**
Augmentez `thumb_cache_px` (défaut 256) puis
`wallpaper-selector --rebuild-cache`. Utile sur écran HiDPI. `thumb_px` règle
la taille d'affichage.

**Le fond d'écran n'est pas restauré à la connexion.**
Vérifiez qu'un fond d'écran a bien été appliqué au moins une fois :
`cat ~/.cache/last_wallpaper_id`. Puis, si vous utilisez le service :

```bash
systemctl --user status wallpaper-selector-restore.service
journalctl --user -u wallpaper-selector-restore.service -n 50
```

**Mes vidéos ne s'appliquent pas.**
`mpvpaper` n'est probablement pas installé. Même logique pour
`linux-wallpaperengine` et les scènes du Workshop.

**Un fond d'écran ajouté à l'instant n'apparaît pas.**
L'index est relu **à chaque ouverture** : rouvrez la fenêtre. Seules les
vignettes nouvelles se génèrent en arrière-plan.

**Désinstaller.**

```bash
wallpaper-selector --quit
systemctl --user disable --now wallpaper-selector-restore.service
rm -f ~/.local/bin/wallpaper-selector ~/.local/bin/init-wallpaper.sh
rm -rf ~/.cache/wallpaper-selector ~/.config/wallpaper-selector
```

---

## Pourquoi c'est rapide

| | Ancien script bash + rofi | wallpaper-selector |
|---|---|---|
| Scan de l'index (70 Steam + 388 perso) | 434 ms, 400+ processus forkés | **3-6 ms**, aucun fork |
| Décodage des vignettes | 652 Mo d'images pleine résolution à chaque ouverture | **20 Mo de cache** généré une fois |
| Ouverture de la fenêtre | rofi + recherche plate | **5 ms** (processus résident) |
| Ouverture perçue (raccourci → fenêtre) | ~0,5-1 s | **~25 ms** |
| Favoris | absent | section ★ en tête de liste |

Les mesures sont reproductibles sur la machine de développement.

Trois décisions expliquent cet écart :

1. **Aucun sous-processus pour scanner.** L'ancien script forkait deux `grep`
   par dossier Steam. Ici c'est un `read_dir` par dossier et une lecture JSON,
   soit 3-6 ms pour 458 entrées. L'index est relu **à chaque ouverture**, donc
   un fond d'écran ajouté à l'instant apparaît immédiatement.

2. **Les vignettes sont décodées une fois.** Le dossier perso pèse 652 Mo avec
   des PNG de 21 Mo ; les décoder en pleine résolution à chaque ouverture était
   le vrai coût. Ils sont réduits une fois vers
   `~/.cache/wallpaper-selector/thumbs/` (20 Mo au total), puis pré-décodés en
   mémoire au démarrage du processus résident.

3. **Un processus résident.** Le premier lancement garde l'index, les vignettes
   décodées et la fenêtre en mémoire. Les appuis suivants envoient `toggle` sur
   `$XDG_RUNTIME_DIR/wallpaper-selector.sock` : la fenêtre réapparaît en ~5 ms.

---

## Notes pour les développeurs

Le code est en Rust + GTK4, en modules :

```
src/
  config.rs      chemins, constantes, lecture du fichier de configuration
  model.rs       Wallpaper, Source, Kind, la variable « favorite »
  scan.rs        index des fonds d'écran (Steam + dossier perso), sans fork
  favorites.rs   lecture / écriture du fichier de favoris
  thumbs.rs      cache de vignettes (parallélisé)
  apply.rs       awww / mpvpaper / linux-wallpaperengine, --restore
  ipc.rs         instance unique et bascule via socket Unix
  ui.rs          fenêtre GTK4 layer-shell, liste, recherche, keybindings
  main.rs        ligne de commande
scripts/
  init-wallpaper.sh                    restauration au démarrage
  wallpaper-selector-restore.service   unité systemd utilisateur
```

Deux pièges de performance ont été mesurés et évités :

- Remplir le `GListStore` ligne par ligne (`append` en boucle) coûtait **650 ms**
  pour 458 lignes, chaque ajout émettant son propre `items-changed`. Un unique
  `splice` règle le problème.
- Reconstruire le modèle à chaque ouverture faisait retomber la vue sur ~200
  liaisons de lignes et autant de décodages de vignettes. `refilter` compare
  désormais les lignes à celles déjà affichées et se contente de replacer la
  sélection quand rien n'a bougé.

Pour instrumenter :

```bash
WALLPAPER_SELECTOR_TIMING=1 wallpaper-selector --show
```

affiche sur la sortie d'erreur le détail `rescan` / `refilter` / `present`.

### Tests

```bash
cargo test
```

Couvre l'analyse de la recherche (`#png`, alias de type, cumul texte + filtre),
la construction des sections favoris, et l'analyse du fichier
`last_wallpaper_id` utilisé par `--restore`.

### Migration depuis l'ancien script

Le fichier `~/.cache/last_wallpaper_id` est réécrit **au même format**
(`AWWW|chemin`, `MPV|chemin`, `LWE|dossier`), donc les autres scripts de la
session continuent de fonctionner sans modification.
