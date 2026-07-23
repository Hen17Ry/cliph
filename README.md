# ClipH

ClipH is a modern persistent clipboard manager designed for Linux.

It keeps a history of copied content and allows users to quickly reopen their clipboard history with a global shortcut.

## Why ClipH?

Linux users often need a simple, fast, and persistent clipboard history that works across daily workflows. ClipH aims to provide a clean desktop experience while supporting multiple content types and modern Linux environments.

## Features

- Persistent clipboard history
- Plain text and rich text support
- Source code snippets
- Links
- Images
- Documents and files
- Emojis
- GIFs
- Kaomojis
- Special symbols
- Global shortcut: `Super + P`
- Background execution
- Auto-start at login
- Wayland and X11 support

## Tech Stack

- Rust
- GTK 4
- Libadwaita
- SQLite
- D-Bus
- Wayland / X11
- Flatpak
- Debian packaging

## Current Status

ClipH `0.2.0` is functional and distributed in Flatpak and Debian formats.

## Shortcut

- `Super / Windows + P`: show or hide ClipH.

On GNOME, ClipH automatically frees this key combination while preserving the hardware shortcut `XF86Display`.

## Project Goals

- Provide a reliable clipboard history for Linux users
- Offer a clean and native desktop experience
- Support multiple content types beyond plain text
- Remain lightweight, practical, and easy to use

## Author

Developed by [Henry GOSSOU](https://github.com/Hen17Ry).
