# wallpaper-selector

Sélecteur de fonds d'écran pour Hyprland, avec favoris, en Rust + GTK4.

## Ce que ça change

| | Ancien script bash + rofi | wallpaper-selector |
|---|---|---|
| Scan de l'index (70 Steam + 388 perso) | 434 ms, 400+ processus forkés | **3-6 ms**, aucun fork |
| Décodage des vignettes | 652 Mo d'images pleine résolution à chaque ouverture | **20 Mo de cache** généré une fois |
| Ouverture de la fenêtre | rofi + recherche plate | **5 ms** (processus résident) |
| Ouverture perçue (raccourci → fenêtre) | ~0,5-1 s | **~25 ms** |
| Favoris | absent | section ★ en tête de liste |

Les mesures sont reproductibles sur cette machine ; voir
[« Comment ça marche »](#comment-ça-marche).

## Dépendances

GTK 4 ≥ 4.12, `gtk4-layer-shell` ≥ 1.0, `gdk-pixbuf`, Rust ≥ 1.80.

Sur Arch :

```bash
sudo pacman -S --needed gtk4 gtk4-layer-shell rust
```

Les moteurs d'affichage utilisés sont ceux du script d'origine et sont détectés
à l'exécution : `awww` (images), `mpvpaper` (vidéos),
`linux-wallpaperengine` (scènes Wallpaper Engine).

## Installation

Le plus simple, un seul script :

```bash
./install.sh
```

Il compile, installe dans `~/.local/bin`, construit le cache de vignettes, et
affiche la ligne exacte à recopier dans la configuration Hyprland. Options :
`--no-cache`, `--no-build`, `--bin-dir DIR`, `--keybind "SUPER, RETURN"`.

Sinon, à la main :

```bash
cargo install --path .        # ou : cargo build --release
wallpaper-selector --rebuild-cache
```

`--rebuild-cache` ne se lance qu'une fois : ~15 s sur 458 fonds d'écran. Ensuite
le cache se complète tout seul, en arrière-plan, pour les nouveaux fichiers.

## Raccourci Hyprland

```
# ~/.config/hypr/hyprland.conf
hl.bind(mainMod .. " + W", hl.dsp.exec_cmd("wallpaper-selector --toggle"))
```

Puis `hyprctl reload`. Le premier appui démarre le processus résident et affiche
la fenêtre ; les suivants ne font que basculer sa visibilité.

## Le processus résident, et pourquoi la commande rend la main

Le premier lancement passe en **tâche de fond** : il se détache du terminal
(`setsid`) et la commande rend la main en ~20 ms. Sans ça, lancer
`wallpaper-selector` à la main bloquerait le shell jusqu'à un `Ctrl+C`, puisque
le processus ne se termine jamais de lui-même.

- Sa sortie d'erreur va dans `~/.cache/wallpaper-selector/daemon.log`.
- Pour le garder attaché au terminal (débogage, `WALLPAPER_SELECTOR_TIMING`),
  utiliser `-F` / `--foreground`.
- Pour l'arrêter : `wallpaper-selector --quit` (ou `pkill -f wallpaper-selector`).
- `resident = false` dans la configuration désactive complètement ce mode :
  l'application se comporte alors comme un programme classique, qu'on lance et
  qui se termine à la fermeture.

## Raccourcis dans l'interface

| Touche | Action |
|---|---|
| `↑` `↓` | Naviguer (les en-têtes de section sont sautés) |
| `Ctrl+N` / `Ctrl+P` | Idem, façon Emacs |
| `Page↑` ou `Début` | Remonter tout en haut des résultats |
| `Page↓` ou `Fin` | Descendre tout en bas des résultats |
| `Entrée` | Appliquer le fond d'écran sélectionné |
| `Tab` ou `Ctrl+F` | Épingler / désépingler le favori |
| `Ctrl+T` | Faire tourner le filtre de type |
| `Échap` | Fermer (la fenêtre est masquée, le processus reste) |
| frappe au clavier | Filtre la liste en direct |

## Recherche par type de fichier

Un jeton préfixé par `#` filtre sur l'**extension réelle**, et ne participe pas
à la recherche textuelle :

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

`Ctrl+T` fait tourner le filtre sans rien taper : il écrit `#pkg`, puis `#mp4`,
`#gif`, `#img`, `#perso`, `#steam`, puis retire le filtre. Le filtre actif reste
toujours visible dans le champ de recherche et dans la barre d'état en bas à
droite, donc rien n'est jamais filtré « en cachette ».

## Favoris

Les favoris apparaissent dans une section **★ FAVORIS** en tête de liste, suivie
d'une section **TOUS LES FONDS D'ÉCRAN**.

Ils sont stockés dans `~/.config/wallpaper-selector/favorites`, un fichier texte
dont chaque ligne porte la variable « favori : oui/non » :

```
# Wallpaper Selector — favoris
perso:neocity.png       → favori : oui
steam:1355236618        → favori : oui
!perso:kirokaze.gif     → favori : non (explicite, prioritaire)
```

L'identifiant d'une entrée est `steam:<id du Workshop>` ou `perso:<nom du
fichier>`. On l'obtient avec `--print` ou `--json`.

Le fichier est éditable à la main, et modifiable depuis un script :

```bash
wallpaper-selector --favorite perso:cabin.png
wallpaper-selector --unfavorite perso:cabin.png
wallpaper-selector --favorites          # liste les identifiants favoris
wallpaper-selector --json | jq '.[] | select(.favorite)'
```

Le processus résident relit le fichier avant chaque modification : un
`--favorite` lancé depuis un script n'est jamais écrasé.

## Configuration

`~/.config/wallpaper-selector/config`, au format `clé = valeur`. Toutes les clés
sont optionnelles :

```ini
# Sources
steam_dir = ~/.local/share/Steam/steamapps/workshop/content/431960
perso_dir = ~/Images/wallpapers

# Fenêtre
width_pct  = 35      # largeur, en % de l'écran
height_pct = 55      # hauteur, en % de l'écran
thumb_px   = 80      # taille des vignettes à l'écran

# Application du fond d'écran
transition_type     = grow
transition_duration = 1.2
monitor             = auto    # écran pour linux-wallpaperengine (auto = écran focalisé)
mpv_output          = *       # sortie mpvpaper

# Comportement
resident = true      # garder le processus en mémoire pour l'instantanéité
```

Les variables d'environnement `WALLPAPER_STEAM_DIR` et `WALLPAPER_PERSO_DIR`
sont aussi reconnues.

### Thème

`~/.config/wallpaper-selector/style.css` remplace complètement le thème intégré
si le fichier existe. Le thème par défaut reprend les couleurs et la géométrie
du `wallpaper-grid.rasi` (Catppuccin, carte `#11111b`, bordure
`#b4befe`, sélection `#6277e3`).

## Ligne de commande

```
wallpaper-selector [OPTIONS]

  -t, --toggle        bascule la fenêtre (défaut)
  -s, --show          affiche
      --hide          masque
      --reload        relit l'index sans rouvrir
  -q, --quit          arrête le processus résident
      --rebuild-cache régénère toutes les vignettes
      --print         index en texte
      --json          index en JSON (champ « favorite » inclus)
      --favorites     identifiants favoris
  -f, --favorite ID   marque comme favori
      --unfavorite ID retire des favoris
  -F, --foreground    reste attaché au terminal (débogage)
```

## Tests

```bash
cargo test
```

Couvre l'analyse de la recherche (`#png`, alias de type, cumul texte + filtre)
et la construction des sections favoris / tout le reste.

## Comment ça marche

Trois décisions expliquent la vitesse :

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
   Si l'idée d'un processus en fond ne plaît pas, `resident = false` dans la
   configuration fait retomber sur une application classique (~150 ms).

Deux pièges de performance ont été mesurés et évités, si jamais vous touchez au
code :

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

## Migration depuis l'ancien script

Le fichier `~/.cache/last_wallpaper_id` est réécrit au format
(`AWWW|chemin`, `MPV|chemin`, `LWE|dossier`)