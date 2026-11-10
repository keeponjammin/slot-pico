/*
 * Loaded ahead of PICO-8 with LD_PRELOAD.
 *
 * SDL_Init: PICO-8 asks SDL for every subsystem, and the stock 32-bit SDL2
 * BaseOS ships was built without haptics (and sensors), so SDL_Init fails
 * as a whole:
 *
 *   SDL Error: SDL not built with haptic (force feedback) support
 *   ** FATAL ERROR: Unable to initialize SDL
 *
 * PICO-8 needs neither, so leave them out of what it asks for.
 *
 * The rest is slot's volume and HUD while PICO-8 plays. `slot --pico8`
 * keeps the volume and brightness keys working as in slot's games, and
 * shares a small file with this shim, named by $SLOT_PICO8_HUD and laid out
 * as `struct hud` (crates/slot/src/pico8_hud.rs writes it):
 *
 * - SDL_OpenAudio: PICO-8 starts at full volume, and its audio callback is
 *   wrapped to scale every sample by slot's curve, (volume / 100)^2, as
 *   slot's own audio does (crates/slot/src/audio/volume.rs).
 * - SDL_RenderPresent: while slot shows its HUD, the strip it drew is laid
 *   over the top of each frame. PICO-8 only redraws pixels that change, so
 *   whatever is drawn over it stays until PICO-8 draws there again. The
 *   shim keeps its own copy of what PICO-8 has under the strip, learns from
 *   each frame which of those pixels PICO-8 changed, and writes that copy
 *   back once the HUD has faded.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

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

/* ---- the file slot shares ---------------------------------------------- */

#define HUD_MAGIC 0x44484c53u
#define HUD_HEADER 64

struct hud {
	volatile uint32_t magic;
	volatile uint32_t seq;    /* odd while slot writes the strip */
	volatile uint32_t volume; /* 0..100 */
	volatile uint32_t shown;  /* 1 while the strip should be on screen */
	volatile uint32_t w, h;   /* the strip, premultiplied RGBA, top down */
};

static struct hud *hud;

static struct hud *shared(void)
{
	static int tried;
	if (tried)
		return hud;
	tried = 1;
	const char *path = getenv("SLOT_PICO8_HUD");
	if (!path)
		return NULL;
	int fd = open(path, O_RDONLY);
	if (fd < 0)
		return NULL;
	off_t len = lseek(fd, 0, SEEK_END);
	void *p = len > HUD_HEADER ? mmap(NULL, len, PROT_READ, MAP_SHARED, fd, 0) : MAP_FAILED;
	close(fd);
	if (p == MAP_FAILED)
		return NULL;
	if (((struct hud *)p)->magic != HUD_MAGIC) {
		munmap(p, len);
		return NULL;
	}
	hud = p;
	return hud;
}

static const uint8_t *strip_pixels(void)
{
	return (const uint8_t *)hud + HUD_HEADER;
}

/* ---- volume -------------------------------------------------------------- */

#define AUDIO_S16LSB 0x8010
#define AUDIO_F32LSB 0x8120

typedef void (*audio_cb)(void *, uint8_t *, int);

typedef struct {
	int freq;
	uint16_t format;
	uint8_t channels;
	uint8_t silence;
	uint16_t samples;
	uint16_t padding;
	uint32_t size;
	audio_cb callback;
	void *userdata;
} AudioSpec;

static audio_cb game_cb;
static void *game_ud;
static uint16_t game_format;
static uint8_t game_silence;

static void at_slots_volume(void *unused, uint8_t *stream, int len)
{
	(void)unused;
	game_cb(game_ud, stream, len);
	uint32_t v = hud->volume;
	if (v >= 100)
		return;
	if (v == 0) {
		memset(stream, game_silence, len);
		return;
	}
	float g = (float)v / 100.0f;
	g *= g;
	if (game_format == AUDIO_S16LSB) {
		int16_t *s = (int16_t *)stream;
		for (int i = 0; i < len / 2; i++)
			s[i] = (int16_t)((float)s[i] * g);
	} else if (game_format == AUDIO_F32LSB) {
		float *s = (float *)stream;
		for (int i = 0; i < len / 4; i++)
			s[i] *= g;
	}
}

int SDL_OpenAudio(AudioSpec *desired, AudioSpec *obtained)
{
	int (*real)(AudioSpec *, AudioSpec *) = (int (*)(AudioSpec *, AudioSpec *))dlsym(RTLD_NEXT, "SDL_OpenAudio");
	if (!real)
		return -1;
	if (!desired || !desired->callback || !shared())
		return real(desired, obtained);
	game_cb = desired->callback;
	game_ud = desired->userdata;
	AudioSpec mine = *desired;
	mine.callback = at_slots_volume;
	mine.userdata = NULL;
	int r = real(&mine, obtained);
	if (obtained) {
		game_format = obtained->format;
		game_silence = obtained->silence;
		obtained->callback = game_cb;
		obtained->userdata = game_ud;
	} else {
		game_format = mine.format;
		game_silence = mine.silence;
		desired->size = mine.size;
		desired->silence = mine.silence;
	}
	fprintf(stderr, "slot: pico8: audio at slot's volume, format 0x%x\n", game_format);
	return r;
}

/* ---- the HUD over the frames --------------------------------------------- */

typedef unsigned int GLenum;
typedef unsigned int GLuint;
typedef int GLint;
typedef int GLsizei;
typedef unsigned char GLboolean;
typedef float GLfloat;
typedef char GLchar;

#define GL_TEXTURE_2D 0x0DE1
#define GL_TEXTURE0 0x84C0
#define GL_RGBA 0x1908
#define GL_UNSIGNED_BYTE 0x1401
#define GL_FLOAT 0x1406
#define GL_TRIANGLE_STRIP 0x0005
#define GL_NEAREST 0x2600
#define GL_CLAMP_TO_EDGE 0x812F
#define GL_TEXTURE_MIN_FILTER 0x2801
#define GL_TEXTURE_MAG_FILTER 0x2800
#define GL_TEXTURE_WRAP_S 0x2802
#define GL_TEXTURE_WRAP_T 0x2803
#define GL_VERTEX_SHADER 0x8B31
#define GL_FRAGMENT_SHADER 0x8B30
#define GL_LINK_STATUS 0x8B82
#define GL_COMPILE_STATUS 0x8B81
#define GL_BLEND 0x0BE2
#define GL_SCISSOR_TEST 0x0C11
#define GL_DEPTH_TEST 0x0B71
#define GL_STENCIL_TEST 0x0B90
#define GL_CULL_FACE 0x0B44
#define GL_CURRENT_PROGRAM 0x8B8D
#define GL_ACTIVE_TEXTURE 0x84E0
#define GL_TEXTURE_BINDING_2D 0x8069
#define GL_ARRAY_BUFFER 0x8892
#define GL_ARRAY_BUFFER_BINDING 0x8894
#define GL_FRAMEBUFFER 0x8D40
#define GL_FRAMEBUFFER_BINDING 0x8CA6
#define GL_VIEWPORT 0x0BA2
#define GL_COLOR_WRITEMASK 0x0C23
#define GL_PACK_ALIGNMENT 0x0D05
#define GL_UNPACK_ALIGNMENT 0x0CF5
#define GL_VERTEX_ATTRIB_ARRAY_ENABLED 0x8622
#define GL_VERTEX_ATTRIB_ARRAY_SIZE 0x8623
#define GL_VERTEX_ATTRIB_ARRAY_STRIDE 0x8624
#define GL_VERTEX_ATTRIB_ARRAY_TYPE 0x8625
#define GL_VERTEX_ATTRIB_ARRAY_NORMALIZED 0x886A
#define GL_VERTEX_ATTRIB_ARRAY_POINTER 0x8645
#define GL_VERTEX_ATTRIB_ARRAY_BUFFER_BINDING 0x889F

#define EGL_DRAW 0x3059
#define EGL_SWAP_BEHAVIOR 0x3093
#define EGL_BUFFER_PRESERVED 0x3094
#define EGL_BUFFER_AGE_EXT 0x313D

static struct {
	void (*ActiveTexture)(GLenum);
	void (*AttachShader)(GLuint, GLuint);
	void (*BindAttribLocation)(GLuint, GLuint, const GLchar *);
	void (*BindBuffer)(GLenum, GLuint);
	void (*BindFramebuffer)(GLenum, GLuint);
	void (*BindTexture)(GLenum, GLuint);
	void (*ColorMask)(GLboolean, GLboolean, GLboolean, GLboolean);
	void (*CompileShader)(GLuint);
	GLuint (*CreateProgram)(void);
	GLuint (*CreateShader)(GLenum);
	void (*Disable)(GLenum);
	void (*DisableVertexAttribArray)(GLuint);
	void (*DrawArrays)(GLenum, GLint, GLsizei);
	void (*Enable)(GLenum);
	void (*EnableVertexAttribArray)(GLuint);
	void (*GenTextures)(GLsizei, GLuint *);
	void (*GetBooleanv)(GLenum, GLboolean *);
	void (*GetIntegerv)(GLenum, GLint *);
	void (*GetProgramiv)(GLuint, GLenum, GLint *);
	void (*GetShaderiv)(GLuint, GLenum, GLint *);
	void (*GetVertexAttribiv)(GLuint, GLenum, GLint *);
	void (*GetVertexAttribPointerv)(GLuint, GLenum, void **);
	GLint (*GetUniformLocation)(GLuint, const GLchar *);
	GLboolean (*IsEnabled)(GLenum);
	void (*LinkProgram)(GLuint);
	void (*PixelStorei)(GLenum, GLint);
	void (*ReadPixels)(GLint, GLint, GLsizei, GLsizei, GLenum, GLenum, void *);
	void (*ShaderSource)(GLuint, GLsizei, const GLchar *const *, const GLint *);
	void (*TexImage2D)(GLenum, GLint, GLint, GLsizei, GLsizei, GLint, GLenum, GLenum, const void *);
	void (*TexParameteri)(GLenum, GLenum, GLint);
	void (*Uniform1i)(GLint, GLint);
	void (*UseProgram)(GLuint);
	void (*VertexAttribPointer)(GLuint, GLint, GLenum, GLboolean, GLsizei, const void *);
	void (*Viewport)(GLint, GLint, GLsizei, GLsizei);
} gl;

static int load_gl(void)
{
	void *(*proc)(const char *) = (void *(*)(const char *))dlsym(RTLD_NEXT, "SDL_GL_GetProcAddress");
	if (!proc)
		return 0;
#define GL(name)                                     \
	do {                                         \
		*(void **)&gl.name = proc("gl" #name); \
		if (!gl.name)                        \
			return 0;                    \
	} while (0)
	GL(ActiveTexture); GL(AttachShader); GL(BindAttribLocation); GL(BindBuffer);
	GL(BindFramebuffer); GL(BindTexture); GL(ColorMask); GL(CompileShader);
	GL(CreateProgram); GL(CreateShader); GL(Disable); GL(DisableVertexAttribArray);
	GL(DrawArrays); GL(Enable); GL(EnableVertexAttribArray); GL(GenTextures);
	GL(GetBooleanv); GL(GetIntegerv); GL(GetProgramiv); GL(GetShaderiv);
	GL(GetVertexAttribiv); GL(GetVertexAttribPointerv); GL(GetUniformLocation);
	GL(IsEnabled); GL(LinkProgram); GL(PixelStorei); GL(ReadPixels); GL(ShaderSource);
	GL(TexImage2D); GL(TexParameteri); GL(Uniform1i); GL(UseProgram);
	GL(VertexAttribPointer); GL(Viewport);
#undef GL
	return 1;
}

/* How many frames ago the buffer PICO-8 just drew into was last shown. */
static int buffer_age(void)
{
	static void *(*display)(void);
	static void *(*surface)(GLint);
	static unsigned (*query)(void *, void *, GLint, GLint *);
	static int tried, logged;
	if (!tried) {
		tried = 1;
		const char *lib = getenv("SDL_VIDEO_EGL_DRIVER");
		void *egl = dlopen(lib ? lib : "libEGL.so.1", RTLD_NOW | RTLD_NOLOAD);
		if (!egl)
			egl = dlopen("libEGL.so.1", RTLD_NOW);
		if (egl) {
			*(void **)&display = dlsym(egl, "eglGetCurrentDisplay");
			*(void **)&surface = dlsym(egl, "eglGetCurrentSurface");
			*(void **)&query = dlsym(egl, "eglQuerySurface");
		}
	}
	GLint age = 0, behaviour = 0;
	if (display && surface && query) {
		void *d = display(), *s = surface(EGL_DRAW);
		if (!query(d, s, EGL_BUFFER_AGE_EXT, &age))
			age = 0;
		query(d, s, EGL_SWAP_BEHAVIOR, &behaviour);
	}
	if (!logged) {
		logged = 1;
		fprintf(stderr, "slot: pico8: hud: buffer age %d, swap behaviour 0x%x\n", age, behaviour);
	}
	if (age > 0)
		return age;
	return behaviour == EGL_BUFFER_PRESERVED ? 1 : 2;
}

#define HISTORY 4

static struct {
	int ready, failed;
	GLuint program, texture;
	GLint sampler;
	int w, rows;               /* the region: the top `rows` rows, full width */
	uint32_t *clean;           /* what PICO-8 has there, bottom row first */
	uint32_t *seen;            /* the region as read back this frame */
	uint32_t *out;             /* what goes on screen this frame */
	uint8_t *strip;            /* slot's strip, copied out of the shared file */
	uint32_t strip_seq;
	int strip_w, strip_h;
	int have_clean;
	int restoring;             /* frames left to write `clean` back */
	struct {
		long frame;
		uint32_t *pixels;  /* what was written into that frame's buffer */
	} history[HISTORY];
	long frame;
} ov;

static GLuint shader(GLenum kind, const char *src)
{
	GLuint s = gl.CreateShader(kind);
	gl.ShaderSource(s, 1, &src, NULL);
	gl.CompileShader(s);
	GLint ok = 0;
	gl.GetShaderiv(s, GL_COMPILE_STATUS, &ok);
	return ok ? s : 0;
}

static int set_up(int w, int rows)
{
	static const char *vs =
		"attribute vec2 pos;\n"
		"attribute vec2 uv;\n"
		"varying vec2 v;\n"
		"void main() { v = uv; gl_Position = vec4(pos, 0.0, 1.0); }\n";
	static const char *fs =
		"#ifdef GL_FRAGMENT_PRECISION_HIGH\n"
		"precision highp float;\n"
		"#else\n"
		"precision mediump float;\n"
		"#endif\n"
		"uniform sampler2D tex;\n"
		"varying vec2 v;\n"
		"void main() { gl_FragColor = texture2D(tex, v); }\n";
	if (!load_gl())
		return 0;
	GLuint v = shader(GL_VERTEX_SHADER, vs), f = shader(GL_FRAGMENT_SHADER, fs);
	if (!v || !f)
		return 0;
	ov.program = gl.CreateProgram();
	gl.AttachShader(ov.program, v);
	gl.AttachShader(ov.program, f);
	gl.BindAttribLocation(ov.program, 0, "pos");
	gl.BindAttribLocation(ov.program, 1, "uv");
	gl.LinkProgram(ov.program);
	GLint ok = 0;
	gl.GetProgramiv(ov.program, GL_LINK_STATUS, &ok);
	if (!ok)
		return 0;
	ov.sampler = gl.GetUniformLocation(ov.program, "tex");
	gl.GenTextures(1, &ov.texture);
	size_t n = (size_t)w * rows;
	ov.clean = calloc(n, 4);
	ov.seen = calloc(n, 4);
	ov.out = calloc(n, 4);
	for (int i = 0; i < HISTORY; i++) {
		ov.history[i].pixels = calloc(n, 4);
		ov.history[i].frame = -1;
	}
	if (!ov.clean || !ov.seen || !ov.out || !ov.history[HISTORY - 1].pixels)
		return 0;
	ov.w = w;
	ov.rows = rows;
	return 1;
}

/* GL state the overlay changes, put back afterwards so SDL's renderer finds
 * everything as it left it. */
struct saved_gl {
	GLint program, active, texture, array, framebuffer, viewport[4], pack, unpack;
	GLboolean blend, scissor, depth, stencil, cull, mask[4];
	struct {
		GLint enabled, size, type, normalized, stride, buffer;
		void *pointer;
	} attr[2];
};

static void save_gl(struct saved_gl *s)
{
	gl.GetIntegerv(GL_CURRENT_PROGRAM, &s->program);
	gl.GetIntegerv(GL_ACTIVE_TEXTURE, &s->active);
	gl.ActiveTexture(GL_TEXTURE0);
	gl.GetIntegerv(GL_TEXTURE_BINDING_2D, &s->texture);
	gl.GetIntegerv(GL_ARRAY_BUFFER_BINDING, &s->array);
	gl.GetIntegerv(GL_FRAMEBUFFER_BINDING, &s->framebuffer);
	gl.GetIntegerv(GL_VIEWPORT, s->viewport);
	gl.GetIntegerv(GL_PACK_ALIGNMENT, &s->pack);
	gl.GetIntegerv(GL_UNPACK_ALIGNMENT, &s->unpack);
	gl.GetBooleanv(GL_COLOR_WRITEMASK, s->mask);
	s->blend = gl.IsEnabled(GL_BLEND);
	s->scissor = gl.IsEnabled(GL_SCISSOR_TEST);
	s->depth = gl.IsEnabled(GL_DEPTH_TEST);
	s->stencil = gl.IsEnabled(GL_STENCIL_TEST);
	s->cull = gl.IsEnabled(GL_CULL_FACE);
	for (GLuint i = 0; i < 2; i++) {
		gl.GetVertexAttribiv(i, GL_VERTEX_ATTRIB_ARRAY_ENABLED, &s->attr[i].enabled);
		gl.GetVertexAttribiv(i, GL_VERTEX_ATTRIB_ARRAY_SIZE, &s->attr[i].size);
		gl.GetVertexAttribiv(i, GL_VERTEX_ATTRIB_ARRAY_TYPE, &s->attr[i].type);
		gl.GetVertexAttribiv(i, GL_VERTEX_ATTRIB_ARRAY_NORMALIZED, &s->attr[i].normalized);
		gl.GetVertexAttribiv(i, GL_VERTEX_ATTRIB_ARRAY_STRIDE, &s->attr[i].stride);
		gl.GetVertexAttribiv(i, GL_VERTEX_ATTRIB_ARRAY_BUFFER_BINDING, &s->attr[i].buffer);
		gl.GetVertexAttribPointerv(i, GL_VERTEX_ATTRIB_ARRAY_POINTER, &s->attr[i].pointer);
	}
}

static void set_enabled(GLenum cap, GLboolean on)
{
	if (on)
		gl.Enable(cap);
	else
		gl.Disable(cap);
}

static void restore_gl(const struct saved_gl *s)
{
	for (GLuint i = 0; i < 2; i++) {
		gl.BindBuffer(GL_ARRAY_BUFFER, (GLuint)s->attr[i].buffer);
		gl.VertexAttribPointer(i, s->attr[i].size, (GLenum)s->attr[i].type,
				       (GLboolean)s->attr[i].normalized, s->attr[i].stride, s->attr[i].pointer);
		if (s->attr[i].enabled)
			gl.EnableVertexAttribArray(i);
		else
			gl.DisableVertexAttribArray(i);
	}
	gl.BindBuffer(GL_ARRAY_BUFFER, (GLuint)s->array);
	set_enabled(GL_BLEND, s->blend);
	set_enabled(GL_SCISSOR_TEST, s->scissor);
	set_enabled(GL_DEPTH_TEST, s->depth);
	set_enabled(GL_STENCIL_TEST, s->stencil);
	set_enabled(GL_CULL_FACE, s->cull);
	gl.ColorMask(s->mask[0], s->mask[1], s->mask[2], s->mask[3]);
	gl.PixelStorei(GL_PACK_ALIGNMENT, s->pack);
	gl.PixelStorei(GL_UNPACK_ALIGNMENT, s->unpack);
	gl.Viewport(s->viewport[0], s->viewport[1], s->viewport[2], s->viewport[3]);
	gl.BindFramebuffer(GL_FRAMEBUFFER, (GLuint)s->framebuffer);
	gl.BindTexture(GL_TEXTURE_2D, (GLuint)s->texture);
	gl.ActiveTexture((GLenum)s->active);
	gl.UseProgram((GLuint)s->program);
}

/* Copies slot's strip out of the shared file when it has a new one. */
static void take_strip(void)
{
	uint32_t seq = hud->seq;
	if (seq & 1 || seq == ov.strip_seq)
		return;
	__sync_synchronize();
	int w = (int)hud->w, h = (int)hud->h;
	if (w <= 0 || h <= 0 || w > 1024 || h > 128)
		return;
	uint8_t *copy = realloc(ov.strip, (size_t)w * h * 4);
	if (!copy)
		return;
	ov.strip = copy;
	memcpy(ov.strip, strip_pixels(), (size_t)w * h * 4);
	__sync_synchronize();
	if (hud->seq != seq)
		return;
	ov.strip_seq = seq;
	ov.strip_w = w;
	ov.strip_h = h;
}

#define RGB(p) ((p) & 0x00ffffffu)

static void draw_region(int win_h)
{
	static const GLfloat quad[] = {
		/* pos x, y, uv s, t: the region's bottom-left, bottom-right, ... */
		-1.0f, 0.0f, 0.0f, 0.0f,
		1.0f, 0.0f, 1.0f, 0.0f,
		-1.0f, 1.0f, 0.0f, 1.0f,
		1.0f, 1.0f, 1.0f, 1.0f,
	};
	GLfloat v[16];
	memcpy(v, quad, sizeof v);
	GLfloat bottom = 1.0f - 2.0f * (GLfloat)ov.rows / (GLfloat)win_h;
	v[1] = v[5] = bottom;
	gl.BindTexture(GL_TEXTURE_2D, ov.texture);
	gl.TexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
	gl.TexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
	gl.TexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
	gl.TexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
	gl.PixelStorei(GL_UNPACK_ALIGNMENT, 4);
	gl.TexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, ov.w, ov.rows, 0, GL_RGBA, GL_UNSIGNED_BYTE, ov.out);
	gl.UseProgram(ov.program);
	gl.Uniform1i(ov.sampler, 0);
	gl.BindBuffer(GL_ARRAY_BUFFER, 0);
	gl.VertexAttribPointer(0, 2, GL_FLOAT, 0, 4 * sizeof(GLfloat), v);
	gl.VertexAttribPointer(1, 2, GL_FLOAT, 0, 4 * sizeof(GLfloat), v + 2);
	gl.EnableVertexAttribArray(0);
	gl.EnableVertexAttribArray(1);
	gl.DrawArrays(GL_TRIANGLE_STRIP, 0, 4);
}

/* Lays `strip` (premultiplied, top down, sw by sh) over `clean`, scaled to
 * the region with the nearest pixel. Both region buffers run bottom up. */
static void composite(void)
{
	for (int y = 0; y < ov.rows; y++) {
		int sy = (ov.rows - 1 - y) * ov.strip_h / ov.rows;
		for (int x = 0; x < ov.w; x++) {
			int sx = x * ov.strip_w / ov.w;
			const uint8_t *s = ov.strip + ((size_t)sy * ov.strip_w + sx) * 4;
			uint32_t c = ov.clean[(size_t)y * ov.w + x];
			uint32_t keep = 255u - s[3], o = 0;
			for (int ch = 0; ch < 3; ch++) {
				uint32_t under = (c >> (8 * ch)) & 0xff;
				uint32_t v = s[ch] + (under * keep + 127) / 255;
				o |= (v > 255 ? 255 : v) << (8 * ch);
			}
			ov.out[(size_t)y * ov.w + x] = o | 0xff000000u;
		}
	}
}

/* Runs before SDL presents a frame PICO-8 has finished drawing. */
static void overlay(void *renderer)
{
	static int (*output_size)(void *, int *, int *);
	if (!output_size)
		*(void **)&output_size = dlsym(RTLD_NEXT, "SDL_GetRendererOutputSize");
	int win_w = 0, win_h = 0;
	if (!output_size || output_size(renderer, &win_w, &win_h) || win_w <= 0 || win_h <= 0)
		return;
	if (hud->shown)
		take_strip();
	int rows = ov.strip_h > 0 ? ov.strip_h * win_w / (ov.strip_w > 0 ? ov.strip_w : win_w) : 0;
	if (!ov.ready) {
		if (!hud->shown || rows <= 0)
			return;
		if (!set_up(win_w, rows)) {
			ov.failed = 1;
			fprintf(stderr, "slot: pico8: hud: could not set up GL, no HUD\n");
			return;
		}
		ov.ready = 1;
	}
	if (win_w != ov.w || rows > ov.rows)
		return;
	int age = buffer_age();
	struct saved_gl s;
	save_gl(&s);
	gl.BindFramebuffer(GL_FRAMEBUFFER, 0);
	gl.Viewport(0, 0, win_w, win_h);
	gl.Disable(GL_BLEND);
	gl.Disable(GL_SCISSOR_TEST);
	gl.Disable(GL_DEPTH_TEST);
	gl.Disable(GL_STENCIL_TEST);
	gl.Disable(GL_CULL_FACE);
	gl.ColorMask(1, 1, 1, 1);
	gl.PixelStorei(GL_PACK_ALIGNMENT, 4);
	gl.ReadPixels(0, win_h - ov.rows, ov.w, ov.rows, GL_RGBA, GL_UNSIGNED_BYTE, ov.seen);
	size_t n = (size_t)ov.w * ov.rows;
	long then = ov.frame - age;
	uint32_t *wrote = NULL;
	for (int i = 0; i < HISTORY; i++)
		if (ov.history[i].frame == then && then >= 0)
			wrote = ov.history[i].pixels;
	if (!ov.have_clean || !wrote) {
		/* Nothing of ours is in this buffer: it is all PICO-8's. */
		memcpy(ov.clean, ov.seen, n * 4);
		ov.have_clean = 1;
	} else {
		/* What differs from what was written there is what PICO-8 drew. */
		for (size_t i = 0; i < n; i++)
			if (RGB(ov.seen[i]) != RGB(wrote[i]))
				ov.clean[i] = ov.seen[i];
	}
	if (hud->shown) {
		composite();
		ov.restoring = HISTORY;
	} else {
		memcpy(ov.out, ov.clean, n * 4);
		ov.restoring--;
	}
	draw_region(win_h);
	ov.history[ov.frame % HISTORY].frame = ov.frame;
	memcpy(ov.history[ov.frame % HISTORY].pixels, ov.out, n * 4);
	if (ov.restoring <= 0) {
		ov.have_clean = 0;
		for (int i = 0; i < HISTORY; i++)
			ov.history[i].frame = -1;
	}
	restore_gl(&s);
}

void SDL_RenderPresent(void *renderer)
{
	static void (*real)(void *);
	static int (*flush)(void *);
	if (!real) {
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_RenderPresent");
		*(void **)&flush = dlsym(RTLD_NEXT, "SDL_RenderFlush");
	}
	if (!real)
		return;
	if (shared() && !ov.failed && (hud->shown || ov.restoring > 0)) {
		/* SDL batches PICO-8's draws until it presents: run them first,
		 * so the strip goes over this frame and not under it. */
		if (flush)
			flush(renderer);
		overlay(renderer);
	}
	ov.frame++;
	real(renderer);
}
