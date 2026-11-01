/* The log callback handed to Cores (RETRO_ENVIRONMENT_GET_LOG_INTERFACE). It is C because a
 * libretro log callback is variadic, which stable Rust can call but not define. Some Cores
 * (Mupen64Plus-Next) call it without checking that they got one, so every Core gets it.
 * Warnings and errors go to stderr; debug and info messages are dropped. */
#include <stdarg.h>
#include <stdio.h>

enum { RETRO_LOG_WARN = 2 };

void game37_core_log(int level, const char *fmt, ...)
{
    va_list args;
    if (level < RETRO_LOG_WARN || !fmt)
        return;
    fputs("game37: Core: ", stderr);
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
}
