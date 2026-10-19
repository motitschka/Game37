# Rust + SDL3 with a custom-drawn UI

Game37 is written in Rust on SDL3, with its own controller-first UI drawn by hand rather than a widget toolkit. Hosting libretro Cores needs a C-compatible language with precise audio/video timing, which rules out Python. Godot's UI would need a C++ extension to host Cores, making two projects. Dear ImGui's controller navigation is too weak for a couch UI.

## Considered Options

C++ + SDL3 + Dear ImGui, Python + pygame, Godot 4.
