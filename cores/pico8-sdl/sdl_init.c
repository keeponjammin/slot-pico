/*
 * Loaded ahead of PICO-8 with LD_PRELOAD. PICO-8 asks SDL for every
 * subsystem, and the stock 32-bit SDL2 BaseOS ships was built without
 * haptics (and sensors), so SDL_Init fails as a whole:
 *
 *   SDL Error: SDL not built with haptic (force feedback) support
 *   ** FATAL ERROR: Unable to initialize SDL
 *
 * PICO-8 needs neither, so leave them out of what it asks for.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>

#define SDL_INIT_HAPTIC 0x00001000u
#define SDL_INIT_SENSOR 0x00008000u
#define UNBUILT (SDL_INIT_HAPTIC | SDL_INIT_SENSOR)

typedef int (*init_fn)(uint32_t);

static int call(const char *name, uint32_t flags)
{
	init_fn real = (init_fn)dlsym(RTLD_NEXT, name);
	if (!real) {
		fprintf(stderr, "slot: pico8: %s not found\n", name);
		return -1;
	}
	if (flags & UNBUILT)
		fprintf(stderr, "slot: pico8: %s without haptics or sensors\n", name);
	return real(flags & ~UNBUILT);
}

int SDL_Init(uint32_t flags)
{
	return call("SDL_Init", flags);
}

int SDL_InitSubSystem(uint32_t flags)
{
	return call("SDL_InitSubSystem", flags);
}
