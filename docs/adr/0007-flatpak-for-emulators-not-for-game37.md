# Emulators install as Flatpaks; Game37 itself is not a Flatpak

First-run Setup only generates `flatpak install` commands, because Flatpak behaves the same on every distro. Game37 itself is built from source rather than shipped on Flathub, because a sandboxed Game37 would need `flatpak-spawn --host` workarounds to start other Flatpak emulators and to read the Games Root freely.

## Consequences

The owner's DuckStation and Mesen are AppImages (not on Flathub), which is fine because the owner's Config is hand-written and Game37 doesn't use them anyway (see ADR 0008).
