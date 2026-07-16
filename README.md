# ClipH

ClipH est un gestionnaire moderne et persistant de presse-papiers conçu pour Linux.

## Objectifs

- Historique persistant du presse-papiers
- Texte simple et texte enrichi
- Code source
- Liens
- Images
- Documents et fichiers
- Émojis
- GIF
- Kaomojis
- Symboles spéciaux
- Ouverture avec Super + P

## Technologies

- Rust
- GTK 4
- Libadwaita
- SQLite
- D-Bus
- Wayland et X11

## État actuel

ClipH 0.2.0 est fonctionnel et distribué aux formats Flatpak et Debian.

Fonctionnalités principales :

- historique persistant du presse-papiers ;
- prise en charge des textes, images et fichiers ;
- raccourci global Super + P ;
- exécution en arrière-plan ;
- démarrage automatique à l'ouverture de session.

## Raccourcis

- **Super/Windows + P** : affiche ou masque ClipH.
- Sous GNOME, ClipH libère automatiquement cette combinaison tout en
  conservant le raccourci matériel `XF86Display`.
