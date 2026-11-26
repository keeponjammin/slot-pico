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
 * - SDL_RenderCopy: what PICO-8 draws is tinted by slot's blue light and
 *   dim gain, as slot's compositor tints every frame of its own games.
 *   With slot's CRT shader chosen for PICO-8, the shim draws PICO-8's
 *   128x128 screen itself instead: scanlines, a glow around bright
 *   pixels and livelier colours (see "the CRT look" below).
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
	volatile uint32_t gain[3]; /* slot's screen tint, 65535 is 1 */
	volatile uint32_t shader; /* SHADER_*, the look PICO-8's screen gets */
};

#define SHADER_OFF 0u
#define SHADER_CRT 1u

#define GAIN_ONE 65535u

static struct hud *hud;

/* A gain as an 8-bit factor, the way SDL's colour modulation takes it. */
static uint8_t gain8(int i)
{
	uint32_t g = hud->gain[i];
	if (g > GAIN_ONE)
		g = GAIN_ONE;
	return (uint8_t)((g * 255u + GAIN_ONE / 2) / GAIN_ONE);
}

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

#define GL_TEXTURE1 0x84C1
#define GL_LINEAR 0x2601
#define GL_COLOR_ATTACHMENT0 0x8CE0
#define GL_FRAMEBUFFER_COMPLETE 0x8CD5

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
	GLenum (*CheckFramebufferStatus)(GLenum);
	void (*ColorMask)(GLboolean, GLboolean, GLboolean, GLboolean);
	void (*CompileShader)(GLuint);
	GLuint (*CreateProgram)(void);
	GLuint (*CreateShader)(GLenum);
	void (*Disable)(GLenum);
	void (*DisableVertexAttribArray)(GLuint);
	void (*DrawArrays)(GLenum, GLint, GLsizei);
	void (*Enable)(GLenum);
	void (*EnableVertexAttribArray)(GLuint);
	void (*FramebufferTexture2D)(GLenum, GLenum, GLenum, GLuint, GLint);
	void (*GenFramebuffers)(GLsizei, GLuint *);
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
	void (*Uniform1f)(GLint, GLfloat);
	void (*Uniform1i)(GLint, GLint);
	void (*Uniform2f)(GLint, GLfloat, GLfloat);
	void (*Uniform3f)(GLint, GLfloat, GLfloat, GLfloat);
	void (*UseProgram)(GLuint);
	void (*VertexAttribPointer)(GLuint, GLint, GLenum, GLboolean, GLsizei, const void *);
	void (*Viewport)(GLint, GLint, GLsizei, GLsizei);
} gl;

static int load_gl(void)
{
	static int loaded;
	if (loaded)
		return 1;
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
	GL(BindFramebuffer); GL(BindTexture); GL(CheckFramebufferStatus); GL(ColorMask);
	GL(CompileShader); GL(CreateProgram); GL(CreateShader); GL(Disable);
	GL(DisableVertexAttribArray); GL(DrawArrays); GL(Enable); GL(EnableVertexAttribArray);
	GL(FramebufferTexture2D); GL(GenFramebuffers); GL(GenTextures);
	GL(GetBooleanv); GL(GetIntegerv); GL(GetProgramiv); GL(GetShaderiv);
	GL(GetVertexAttribiv); GL(GetVertexAttribPointerv); GL(GetUniformLocation);
	GL(IsEnabled); GL(LinkProgram); GL(PixelStorei); GL(ReadPixels); GL(ShaderSource);
	GL(TexImage2D); GL(TexParameteri); GL(Uniform1f); GL(Uniform1i); GL(Uniform2f);
	GL(Uniform3f); GL(UseProgram); GL(VertexAttribPointer); GL(Viewport);
#undef GL
	loaded = 1;
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
 * the region with the nearest pixel. Both region buffers run bottom up.
 * The strip gets slot's tint, as slot's compositor tints its HUD with the
 * rest of the frame; `clean` already has it from SDL_RenderCopy. */
static void composite(void)
{
	uint32_t tint[3] = { gain8(0), gain8(1), gain8(2) };
	for (int y = 0; y < ov.rows; y++) {
		int sy = (ov.rows - 1 - y) * ov.strip_h / ov.rows;
		for (int x = 0; x < ov.w; x++) {
			int sx = x * ov.strip_w / ov.w;
			const uint8_t *s = ov.strip + ((size_t)sy * ov.strip_w + sx) * 4;
			uint32_t c = ov.clean[(size_t)y * ov.w + x];
			uint32_t keep = 255u - s[3], o = 0;
			for (int ch = 0; ch < 3; ch++) {
				uint32_t under = (c >> (8 * ch)) & 0xff;
				uint32_t ink = (s[ch] * tint[ch] + 127) / 255;
				uint32_t v = ink + (under * keep + 127) / 255;
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

/* ---- the CRT look -------------------------------------------------------- *
 *
 * PICO-8 keeps its 128x128 screen in one SDL texture and copies it to the
 * window. With the CRT shader on, the shim draws that texture itself:
 *
 * 1. the bright part of the screen, blurred across and then down, into a
 *    128x128 texture of its own (two small passes, cheap on the Mali);
 * 2. the screen, pixel for pixel as PICO-8 drew it, with livelier colours,
 *    one soft dark scanline at the foot of every PICO-8 row, the blur laid
 *    over it as glow (near and wide), and a faint vignette, tinted by slot's
 *    blue light and dim gain.
 *
 * Everything is drawn where SDL would have drawn the copy, so the HUD and
 * the rest of the shim see the same frames they always did. */

#define CRT_SIZE 128

/* SDL_PIXELFORMAT_*, as SDL_QueryTexture reports them. */
#define SDL_ARGB8888 0x16362004u
#define SDL_XRGB8888 0x16161804u

typedef struct {
	int x, y, w, h;
} Rect;

static struct {
	int ready, failed;
	GLuint blur, final;           /* programs */
	GLint blur_tex, blur_step, blur_bright, blur_swap;
	GLint final_tex, final_glow, final_swap, final_foot, final_gain;
	GLuint fbo[2], tex[2];       /* across, then down */
	void *screen;                /* the SDL texture PICO-8 copies */
	GLuint screen_gl;            /* its GL name */
	int swap;                    /* -1 until known: 1 if red and blue are swapped in GL */
	unsigned copies;             /* copies drawn, to pace the read-backs */
	unsigned looked;             /* times read back to learn `swap` */
	uint32_t *probe;
	unsigned long uploads;       /* times PICO-8 changed `screen`'s pixels */
	int upload_seen;             /* 1 once an upload to `screen` was seen */
	unsigned long blurred_at;    /* `uploads` when tex[1] was last blurred */
	int blurred;                 /* 1 while tex[1] holds a blur of `screen` */
	/* The last texture that looked like PICO-8's screen, whichever look it
	 * was drawn with, and where all of it goes (dst as SDL takes it), so a
	 * change of look can draw the whole screen again. */
	void *last;
	Rect whole;
	int drawn_as;                /* -1 at first, then the SHADER_* last drawn */
	int redraw;                  /* frames left to draw `last` whole again */
} crt = { .swap = -1, .drawn_as = -1 };

/* Read the screen back to learn `swap` at most this often, this many times:
 * each read makes the CPU wait for the GPU. */
#define SWAP_EVERY 30u
#define SWAP_TRIES 40u

/* PICO-8 changed the pixels of `texture`. */
static void crt_uploaded(void *texture)
{
	if (texture && texture == crt.screen) {
		crt.uploads++;
		crt.upload_seen = 1;
	}
}

static const char *crt_vs =
	"attribute vec2 pos;\n"
	"attribute vec2 uv;\n"
	"varying vec2 v;\n"
	"void main() { v = uv; gl_Position = vec4(pos, 0.0, 1.0); }\n";

/* One direction of a 9-tap Gaussian (sigma 1.6 PICO-8 pixels). In the first
 * pass `bright` keeps only what glows: nothing of dark colours, all of the
 * brightest. */
static const char *crt_blur_fs =
	"precision mediump float;\n"
	"uniform sampler2D tex;\n"
	"uniform vec2 step;\n"
	"uniform float bright;\n"
	"uniform float swap;\n"
	"varying vec2 v;\n"
	"vec3 at(float k) {\n"
	"    vec3 c = texture2D(tex, v + step * k).rgb;\n"
	"    c = mix(c, c.bgr, swap);\n"
	"    float m = max(c.r, max(c.g, c.b));\n"
	"    return c * mix(1.0, smoothstep(0.35, 1.0, m), bright);\n"
	"}\n"
	"void main() {\n"
	"    vec3 s = at(0.0) * 0.2466;\n"
	"    s += (at(-1.0) + at(1.0)) * 0.2006;\n"
	"    s += (at(-2.0) + at(2.0)) * 0.1081;\n"
	"    s += (at(-3.0) + at(3.0)) * 0.0386;\n"
	"    s += (at(-4.0) + at(4.0)) * 0.0091;\n"
	"    gl_FragColor = vec4(s, 1.0);\n"
	"}\n";

/* `foot` is how many PICO-8 rows one window pixel spans. */
static const char *crt_final_fs =
	"#ifdef GL_FRAGMENT_PRECISION_HIGH\n"
	"precision highp float;\n"
	"#else\n"
	"precision mediump float;\n"
	"#endif\n"
	"uniform sampler2D tex;\n"
	"uniform sampler2D glow;\n"
	"uniform float swap;\n"
	"uniform float foot;\n"
	"uniform vec3 gain;\n"
	"varying vec2 v;\n"
	"const float SIZE = 128.0;\n"
	"const float GAMMA = 0.85;\n"
	"const float SATURATION = 1.25;\n"
	"const float BRIGHT = 1.04;\n"
	"const float SCAN_DEPTH = 0.22;\n"
	"const float SCAN_WIDTH = 0.13;\n"
	"const float GLOW = 0.18;\n"
	"const float GLOW_WIDE = 0.10;\n"
	"const float VIGNETTE = 0.07;\n"
	/* The dark line sits at the foot of each row, centred on the last
	 * window row of it when PICO-8 is scaled by a whole number. */
	"float scan(float y) {\n"
	"    float centre = 1.0 - 0.5 * min(foot, 0.34);\n"
	"    float d = abs(fract(y) - centre);\n"
	"    d = min(d, 1.0 - d);\n"
	"    return 1.0 - SCAN_DEPTH * exp(-(d * d) / (SCAN_WIDTH * SCAN_WIDTH));\n"
	"}\n"
	"void main() {\n"
	"    vec2 p = v * SIZE;\n"
	"    mediump vec3 c = texture2D(tex, (floor(p) + 0.5) / SIZE).rgb;\n"
	"    c = mix(c, c.bgr, swap);\n"
	"    c = pow(c, vec3(GAMMA));\n"
	"    mediump float l = dot(c, vec3(0.299, 0.587, 0.114));\n"
	"    c = clamp(mix(vec3(l), c, SATURATION) * BRIGHT, 0.0, 1.0);\n"
	"    float q = foot * 0.25;\n"
	"    mediump float s = 0.25 * (scan(p.y - 1.5 * q) + scan(p.y - 0.5 * q)\n"
	"                    + scan(p.y + 0.5 * q) + scan(p.y + 1.5 * q));\n"
	"    mediump vec3 near = texture2D(glow, v).rgb;\n"
	"    vec2 o = vec2(5.0 / SIZE);\n"
	"    mediump vec3 wide = 0.25 * (texture2D(glow, v + o).rgb + texture2D(glow, v - o).rgb\n"
	"                      + texture2D(glow, v + vec2(o.x, -o.y)).rgb\n"
	"                      + texture2D(glow, v + vec2(-o.x, o.y)).rgb);\n"
	"    c = c * s + near * GLOW + wide * GLOW_WIDE;\n"
	"    vec2 e = v * 2.0 - 1.0;\n"
	"    c *= 1.0 - VIGNETTE * dot(e, e);\n"
	"    gl_FragColor = vec4(clamp(c, 0.0, 1.0) * gain, 1.0);\n"
	"}\n";

static GLuint program(const char *vs, const char *fs)
{
	GLuint v = shader(GL_VERTEX_SHADER, vs), f = shader(GL_FRAGMENT_SHADER, fs);
	if (!v || !f)
		return 0;
	GLuint p = gl.CreateProgram();
	gl.AttachShader(p, v);
	gl.AttachShader(p, f);
	gl.BindAttribLocation(p, 0, "pos");
	gl.BindAttribLocation(p, 1, "uv");
	gl.LinkProgram(p);
	GLint ok = 0;
	gl.GetProgramiv(p, GL_LINK_STATUS, &ok);
	return ok ? p : 0;
}

static int crt_set_up(void)
{
	if (!load_gl())
		return 0;
	crt.blur = program(crt_vs, crt_blur_fs);
	crt.final = program(crt_vs, crt_final_fs);
	if (!crt.blur || !crt.final)
		return 0;
	crt.blur_tex = gl.GetUniformLocation(crt.blur, "tex");
	crt.blur_step = gl.GetUniformLocation(crt.blur, "step");
	crt.blur_bright = gl.GetUniformLocation(crt.blur, "bright");
	crt.blur_swap = gl.GetUniformLocation(crt.blur, "swap");
	crt.final_tex = gl.GetUniformLocation(crt.final, "tex");
	crt.final_glow = gl.GetUniformLocation(crt.final, "glow");
	crt.final_swap = gl.GetUniformLocation(crt.final, "swap");
	crt.final_foot = gl.GetUniformLocation(crt.final, "foot");
	crt.final_gain = gl.GetUniformLocation(crt.final, "gain");
	gl.GenTextures(2, crt.tex);
	gl.GenFramebuffers(2, crt.fbo);
	for (int i = 0; i < 2; i++) {
		gl.BindTexture(GL_TEXTURE_2D, crt.tex[i]);
		gl.TexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
		gl.TexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
		gl.TexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
		gl.TexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
		gl.TexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, CRT_SIZE, CRT_SIZE, 0, GL_RGBA, GL_UNSIGNED_BYTE, NULL);
		gl.BindFramebuffer(GL_FRAMEBUFFER, crt.fbo[i]);
		gl.FramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, crt.tex[i], 0);
		if (gl.CheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE)
			return 0;
	}
	crt.probe = calloc(CRT_SIZE * CRT_SIZE, 4);
	return crt.probe != NULL;
}

/* PICO-8's 32 colours, as 0xRRGGBB. */
static const uint32_t pico8_colours[32] = {
	0x000000, 0x1d2b53, 0x7e2553, 0x008751, 0xab5236, 0x5f574f, 0xc2c3c7, 0xfff1e8,
	0xff004d, 0xffa300, 0xffec27, 0x00e436, 0x29adff, 0x83769c, 0xff77a8, 0xffccaa,
	0x291814, 0x111d35, 0x422136, 0x125359, 0x742f29, 0x49333b, 0xa28879, 0xf3ef7d,
	0xbe1250, 0xff6c24, 0xa8e72e, 0x00b543, 0x065ab5, 0x754665, 0xff6e59, 0xff9d81,
};

static int is_pico8_colour(uint32_t rgb)
{
	for (int i = 1; i < 32; i++)
		if (pico8_colours[i] == rgb)
			return 1;
	return 0;
}

/* SDL may keep the texture with red and blue swapped in GL and swap them
 * back as it draws. Read the screen back and see which way round PICO-8's
 * own colours come out. Runs with crt.fbo[0] bound. */
static void learn_swap(void)
{
	gl.FramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, crt.screen_gl, 0);
	if (gl.CheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE) {
		gl.PixelStorei(GL_PACK_ALIGNMENT, 4);
		gl.ReadPixels(0, 0, CRT_SIZE, CRT_SIZE, GL_RGBA, GL_UNSIGNED_BYTE, crt.probe);
		int straight = 0, swapped = 0;
		for (int i = 0; i < CRT_SIZE * CRT_SIZE; i++) {
			const uint8_t *b = (const uint8_t *)&crt.probe[i];
			uint32_t rgb = (uint32_t)b[0] << 16 | (uint32_t)b[1] << 8 | b[2];
			uint32_t bgr = (uint32_t)b[2] << 16 | (uint32_t)b[1] << 8 | b[0];
			straight += is_pico8_colour(rgb);
			swapped += is_pico8_colour(bgr);
		}
		if (straight + swapped > 256 && straight != swapped) {
			crt.swap = swapped > straight;
			fprintf(stderr, "slot: pico8: crt: colours %s (%d against %d)\n",
				crt.swap ? "swapped" : "straight", straight, swapped);
		}
	}
	gl.FramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, crt.tex[0], 0);
}

static void quad(GLfloat x0, GLfloat y0, GLfloat x1, GLfloat y1)
{
	/* pos x, y then uv s, t; t is 0 at the top row of PICO-8's screen. */
	GLfloat v[16] = {
		x0, y1, 0.0f, 0.0f,
		x1, y1, 1.0f, 0.0f,
		x0, y0, 0.0f, 1.0f,
		x1, y0, 1.0f, 1.0f,
	};
	gl.BindBuffer(GL_ARRAY_BUFFER, 0);
	gl.VertexAttribPointer(0, 2, GL_FLOAT, 0, 4 * sizeof(GLfloat), v);
	gl.VertexAttribPointer(1, 2, GL_FLOAT, 0, 4 * sizeof(GLfloat), v + 2);
	gl.EnableVertexAttribArray(0);
	gl.EnableVertexAttribArray(1);
	gl.DrawArrays(GL_TRIANGLE_STRIP, 0, 4);
}

/* Whether `texture` is PICO-8's screen: square, 128 pixels or a whole
 * multiple of it (PICO-8 can scale its screen up before it copies it), and
 * copied straight to the window. Says once why a copy is not. */
static int is_screen(void *renderer, void *texture, uint32_t *format, int *tw, int *th)
{
	static int (*query)(void *, uint32_t *, int *, int *, int *);
	static void *(*target)(void *);
	static int said[4][2], said_target;
	if (!query) {
		*(void **)&query = dlsym(RTLD_NEXT, "SDL_QueryTexture");
		*(void **)&target = dlsym(RTLD_NEXT, "SDL_GetRenderTarget");
	}
	int access = 0;
	if (!query || !target || query(texture, format, &access, tw, th))
		return 0;
	if (*tw != *th || *tw < CRT_SIZE || *tw % CRT_SIZE != 0 || *tw > 8 * CRT_SIZE) {
		/* The first few sizes, so a cursor's texture cannot hide the screen's. */
		for (int i = 0; i < 4; i++) {
			if (said[i][0] == *tw && said[i][1] == *th)
				break;
			if (said[i][0] == 0) {
				said[i][0] = *tw;
				said[i][1] = *th;
				fprintf(stderr, "slot: pico8: crt: passes over a %dx%d texture (format 0x%x)\n",
					*tw, *th, *format);
				break;
			}
		}
		return 0;
	}
	if (target(renderer)) {
		if (!said_target) {
			said_target = 1;
			fprintf(stderr, "slot: pico8: crt: PICO-8 copies its %dx%d screen into a texture, not the window\n",
				*tw, *th);
		}
		return 0;
	}
	return 1;
}

static int round_int(float f)
{
	return (int)(f < 0.0f ? f - 0.5f : f + 0.5f);
}

/* Where all of `texture` goes when `src` of it is copied to `dst`, as a dst
 * SDL_RenderCopy takes (inside the viewport, in the renderer's units). */
static int whole_rect(void *renderer, int tw, int th, const Rect *src, const Rect *dst, Rect *whole)
{
	static void (*viewport)(void *, Rect *);
	if (!viewport)
		*(void **)&viewport = dlsym(RTLD_NEXT, "SDL_RenderGetViewport");
	if (!viewport)
		return 0;
	Rect vp;
	viewport(renderer, &vp);
	Rect s = src ? *src : (Rect){ 0, 0, tw, th };
	Rect d = dst ? *dst : (Rect){ 0, 0, vp.w, vp.h };
	if (s.w <= 0 || s.h <= 0 || d.w <= 0 || d.h <= 0)
		return 0;
	float kx = (float)d.w / (float)s.w, ky = (float)d.h / (float)s.h;
	whole->x = round_int((float)d.x - (float)s.x * kx);
	whole->y = round_int((float)d.y - (float)s.y * ky);
	whole->w = round_int((float)tw * kx);
	whole->h = round_int((float)th * ky);
	return 1;
}

/* Draws `texture` (PICO-8's screen) the way SDL_RenderCopy would place it,
 * with the CRT look. Returns 0 when it did not, so SDL draws it instead. */
static int crt_copy(void *renderer, void *texture, const Rect *src, const Rect *dst)
{
	static int (*query)(void *, uint32_t *, int *, int *, int *);
	static int (*bind)(void *, float *, float *);
	static int (*unbind)(void *);
	static void (*viewport)(void *, Rect *);
	static void (*scale)(void *, float *, float *);
	static int (*output_size)(void *, int *, int *);
	static void *(*target)(void *);
	static int (*flush)(void *);
	static int logged;
	if (crt.failed)
		return 0;
	if (!query) {
		*(void **)&query = dlsym(RTLD_NEXT, "SDL_QueryTexture");
		*(void **)&bind = dlsym(RTLD_NEXT, "SDL_GL_BindTexture");
		*(void **)&unbind = dlsym(RTLD_NEXT, "SDL_GL_UnbindTexture");
		*(void **)&viewport = dlsym(RTLD_NEXT, "SDL_RenderGetViewport");
		*(void **)&scale = dlsym(RTLD_NEXT, "SDL_RenderGetScale");
		*(void **)&output_size = dlsym(RTLD_NEXT, "SDL_GetRendererOutputSize");
		*(void **)&target = dlsym(RTLD_NEXT, "SDL_GetRenderTarget");
		*(void **)&flush = dlsym(RTLD_NEXT, "SDL_RenderFlush");
		if (!query || !bind || !unbind || !viewport || !scale || !output_size || !target) {
			crt.failed = 1;
			fprintf(stderr, "slot: pico8: crt: SDL lacks what it needs, no CRT\n");
			return 0;
		}
	}
	uint32_t format = 0;
	int tw = 0, th = 0;
	if (!is_screen(renderer, texture, &format, &tw, &th))
		return 0;
	int win_w = 0, win_h = 0;
	if (output_size(renderer, &win_w, &win_h) || win_w <= 0 || win_h <= 0)
		return 0;

	/* Where SDL would put the whole texture, in window pixels. */
	Rect vp;
	float sx = 1.0f, sy = 1.0f;
	viewport(renderer, &vp);
	scale(renderer, &sx, &sy);
	Rect s = src ? *src : (Rect){ 0, 0, tw, th };
	Rect d = dst ? *dst : (Rect){ 0, 0, vp.w, vp.h };
	if (s.w <= 0 || s.h <= 0 || d.w <= 0 || d.h <= 0)
		return 0;
	float kx = (float)d.w / (float)s.w, ky = (float)d.h / (float)s.h;
	float fx = ((float)vp.x + (float)d.x - (float)s.x * kx) * sx;
	float fy = ((float)vp.y + (float)d.y - (float)s.y * ky) * sy;
	float fw = (float)tw * kx * sx, fh = (float)th * ky * sy;

	struct saved_gl saved;
	if (!crt.ready) {
		if (!load_gl()) {
			crt.failed = 1;
			fprintf(stderr, "slot: pico8: crt: no GL, no CRT\n");
			return 0;
		}
		save_gl(&saved);
		int ok = crt_set_up();
		restore_gl(&saved);
		if (!ok) {
			crt.failed = 1;
			fprintf(stderr, "slot: pico8: crt: could not set up GL, no CRT\n");
			return 0;
		}
		crt.ready = 1;
	}
	save_gl(&saved);
	GLint unit1 = 0;
	gl.ActiveTexture(GL_TEXTURE1);
	gl.GetIntegerv(GL_TEXTURE_BINDING_2D, &unit1);
	gl.ActiveTexture(GL_TEXTURE0);
	if (texture != crt.screen) {
		/* SDL_GL_BindTexture binds it and tells SDL so; unbinding tells SDL
		 * nothing is bound, so it binds its own again when it next draws. */
		GLint name = 0;
		float bw = 0.0f, bh = 0.0f;
		bind(texture, &bw, &bh);
		gl.GetIntegerv(GL_TEXTURE_BINDING_2D, &name);
		unbind(texture);
		crt.screen = texture;
		crt.screen_gl = (GLuint)name;
		crt.swap = -1;
		crt.looked = 0;
		crt.blurred = 0;
		crt.upload_seen = 0;
	}
	if (!logged) {
		logged = 1;
		fprintf(stderr, "slot: pico8: crt: format 0x%x, %dx%d to %.1f,%.1f %.1fx%.1f of %dx%d\n",
			format, tw, th, fx, fy, fw, fh, win_w, win_h);
	}
	gl.Disable(GL_BLEND);
	gl.Disable(GL_SCISSOR_TEST);
	gl.Disable(GL_DEPTH_TEST);
	gl.Disable(GL_STENCIL_TEST);
	gl.Disable(GL_CULL_FACE);
	gl.ColorMask(1, 1, 1, 1);

	/* Blur the bright part across into tex[0], then down into tex[1]. Each
	 * pass draws its top row (t = 0) into the texture's last row, so tex[0]
	 * is upside down and tex[1], flipped twice, the right way up again.
	 *
	 * This runs before SDL's queued draws reach the window: the Mali draws a
	 * frame in tiles, and turning to another framebuffer once the window has
	 * draws pending would make it write the window out and read it back in.
	 * A screen PICO-8 has not changed since keeps its blur. */
	if (crt.swap < 0 && crt.looked < SWAP_TRIES && crt.copies % SWAP_EVERY == 0) {
		gl.BindFramebuffer(GL_FRAMEBUFFER, crt.fbo[0]);
		crt.looked++;
		learn_swap();
		if (crt.swap >= 0)
			crt.blurred = 0;
	}
	crt.copies++;
	float swap = crt.swap < 0 ? (format == SDL_ARGB8888 || format == SDL_XRGB8888) : (float)crt.swap;
	if (!crt.blurred || !crt.upload_seen || crt.blurred_at != crt.uploads) {
		gl.BindFramebuffer(GL_FRAMEBUFFER, crt.fbo[0]);
		gl.Viewport(0, 0, CRT_SIZE, CRT_SIZE);
		gl.UseProgram(crt.blur);
		gl.Uniform1i(crt.blur_tex, 0);
		gl.BindTexture(GL_TEXTURE_2D, crt.screen_gl);
		gl.Uniform2f(crt.blur_step, 1.0f / CRT_SIZE, 0.0f);
		gl.Uniform1f(crt.blur_bright, 1.0f);
		gl.Uniform1f(crt.blur_swap, swap);
		quad(-1.0f, -1.0f, 1.0f, 1.0f);
		gl.BindFramebuffer(GL_FRAMEBUFFER, crt.fbo[1]);
		gl.BindTexture(GL_TEXTURE_2D, crt.tex[0]);
		gl.Uniform2f(crt.blur_step, 0.0f, 1.0f / CRT_SIZE);
		gl.Uniform1f(crt.blur_bright, 0.0f);
		gl.Uniform1f(crt.blur_swap, 0.0f);
		quad(-1.0f, -1.0f, 1.0f, 1.0f);
		crt.blurred = 1;
		crt.blurred_at = crt.uploads;
	}
	gl.ActiveTexture(GL_TEXTURE1);
	gl.BindTexture(GL_TEXTURE_2D, (GLuint)unit1);
	gl.ActiveTexture(GL_TEXTURE0);
	restore_gl(&saved);

	/* SDL batches draws: run what PICO-8 queued before this copy, then draw
	 * the screen over them. */
	if (flush)
		flush(renderer);
	save_gl(&saved);
	gl.ActiveTexture(GL_TEXTURE1);
	gl.GetIntegerv(GL_TEXTURE_BINDING_2D, &unit1);
	gl.ActiveTexture(GL_TEXTURE0);
	gl.Disable(GL_BLEND);
	gl.Disable(GL_SCISSOR_TEST);
	gl.Disable(GL_DEPTH_TEST);
	gl.Disable(GL_STENCIL_TEST);
	gl.Disable(GL_CULL_FACE);
	gl.ColorMask(1, 1, 1, 1);

	/* The screen itself, where SDL would have put it. */
	gl.BindFramebuffer(GL_FRAMEBUFFER, (GLuint)saved.framebuffer);
	gl.Viewport(0, 0, win_w, win_h);
	gl.UseProgram(crt.final);
	gl.Uniform1i(crt.final_tex, 0);
	gl.Uniform1i(crt.final_glow, 1);
	gl.Uniform1f(crt.final_swap, swap);
	gl.Uniform1f(crt.final_foot, (float)CRT_SIZE / fh);
	gl.Uniform3f(crt.final_gain, (float)hud->gain[0] / GAIN_ONE, (float)hud->gain[1] / GAIN_ONE,
		     (float)hud->gain[2] / GAIN_ONE);
	gl.ActiveTexture(GL_TEXTURE1);
	gl.BindTexture(GL_TEXTURE_2D, crt.tex[1]);
	gl.ActiveTexture(GL_TEXTURE0);
	gl.BindTexture(GL_TEXTURE_2D, crt.screen_gl);
	quad(fx / (float)win_w * 2.0f - 1.0f, 1.0f - (fy + fh) / (float)win_h * 2.0f,
	     (fx + fw) / (float)win_w * 2.0f - 1.0f, 1.0f - fy / (float)win_h * 2.0f);

	gl.ActiveTexture(GL_TEXTURE1);
	gl.BindTexture(GL_TEXTURE_2D, (GLuint)unit1);
	gl.ActiveTexture(GL_TEXTURE0);
	restore_gl(&saved);
	return 1;
}

/* PICO-8 puts its screen in its texture one of these two ways. Either tells
 * the CRT look the screen changed, so it blurs it again. */
int SDL_UpdateTexture(void *texture, const void *rect, const void *pixels, int pitch)
{
	static int (*real)(void *, const void *, const void *, int);
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_UpdateTexture");
	if (!real)
		return -1;
	int r = real(texture, rect, pixels, pitch);
	crt_uploaded(texture);
	return r;
}

void SDL_UnlockTexture(void *texture)
{
	static void (*real)(void *);
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_UnlockTexture");
	if (real)
		real(texture);
	crt_uploaded(texture);
}

/* What SDL_RenderCopy does without the CRT look: tinted by slot's blue
 * light and dim gain, as slot's compositor tints its frames. */
static int plain_copy(void *renderer, void *texture, const void *src, const void *dst)
{
	static int (*real)(void *, void *, const void *, const void *);
	static int (*color_mod)(void *, uint8_t, uint8_t, uint8_t);
	static int logged;
	if (!real) {
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_RenderCopy");
		*(void **)&color_mod = dlsym(RTLD_NEXT, "SDL_SetTextureColorMod");
	}
	if (!real)
		return -1;
	if (shared() && color_mod && texture) {
		uint8_t r = gain8(0), g = gain8(1), b = gain8(2);
		color_mod(texture, r, g, b);
		if (!logged) {
			logged = 1;
			fprintf(stderr, "slot: pico8: tint %u %u %u, copies %s to %s\n", r, g, b,
				src ? "part" : "all", dst ? "a rect" : "the window");
		}
	}
	return real(renderer, texture, src, dst);
}

/* PICO-8 draws its screen with SDL_RenderCopy: tint it, or draw it with the
 * CRT look when slot's PICO-8 shader is CRT. Either way, keep where the
 * whole screen goes, for when the look changes. */
int SDL_RenderCopy(void *renderer, void *texture, const void *src, const void *dst)
{
	if (texture && shared()) {
		uint32_t format = 0;
		int tw = 0, th = 0;
		Rect whole;
		if (is_screen(renderer, texture, &format, &tw, &th) &&
		    whole_rect(renderer, tw, th, (const Rect *)src, (const Rect *)dst, &whole)) {
			crt.last = texture;
			crt.whole = whole;
		}
		if (hud->shader == SHADER_CRT && crt_copy(renderer, texture, (const Rect *)src, (const Rect *)dst))
			return 0;
	}
	return plain_copy(renderer, texture, src, dst);
}

/* PICO-8 only redraws what changes, so after the look changes (in slot's
 * menu, say) the rest of its screen would keep the old one. Draw the whole
 * screen again with the new look, for as many frames as the window keeps
 * buffers, before the frame is shown. */
static void crt_present(void *renderer)
{
	int as = hud->shader == SHADER_CRT ? SHADER_CRT : SHADER_OFF;
	if (crt.drawn_as < 0)
		crt.drawn_as = as;
	if (as != crt.drawn_as) {
		crt.drawn_as = as;
		crt.redraw = HISTORY;
		fprintf(stderr, "slot: pico8: crt: %s\n", as == SHADER_CRT ? "on" : "off");
	}
	if (crt.redraw <= 0 || !crt.last)
		return;
	crt.redraw--;
	if (as == SHADER_CRT && crt_copy(renderer, crt.last, NULL, &crt.whole))
		return;
	plain_copy(renderer, crt.last, NULL, &crt.whole);
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
	if (shared())
		crt_present(renderer);
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
