# OpenGL hardware rendering in v1, not Vulkan, not deferred

Game37 implements libretro OpenGL hardware rendering from v1, so PS1, N64, DS and PSP run embedded from the start. Deferring it would have shipped sooner, with N64 and PSP temporarily external, but the owner preferred everything embedded from day one. OpenGL was chosen over Vulkan because more Cores support it and the integration is much smaller. ParaLLEl-N64 (Vulkan-only) is therefore not an option.
