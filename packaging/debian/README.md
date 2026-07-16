# ClipH — paquet Debian

## Construction

Depuis la racine de ClipH :

```bash
bash packaging/debian/build-deb.sh
```

Le paquet généré sera placé dans :

```text
dist/cliph_0.1.0_amd64.deb
```

## Premier test

L'ancienne installation locale doit d'abord être retirée :

```bash
"$HOME/.local/bin/cliph" uninstall
```

Ensuite :

```bash
sudo apt install ./dist/cliph_0.1.0_amd64.deb
```

## Vérification

```bash
systemctl --user status app-com.cliph.ClipH.service --no-pager
```

Le raccourci est `Super/Windows + P`.

## Désinstallation

```bash
sudo apt remove cliph
```

Les données de l'utilisateur dans `~/.local/share/cliph` sont conservées.
