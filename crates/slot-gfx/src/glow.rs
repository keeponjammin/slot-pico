//! The glow of the CRT look, at the game texture's own size: the bright part of the game's
//! picture blurred a little (near) and then a lot (wide), across and down, the two added up in
//! one texture. The PICO-8 SDL shim draws the same glow for the owner's PICO-8
//! (cores/pico8-sdl/sdl_init.c); this one is for fake-08's frames.
//!
//! All four passes are small (one texel per game pixel), and run when a new frame is uploaded,
//! before anything is drawn to the screen: the Mali draws a frame in tiles, and turning to
//! another framebuffer once the screen has draws pending would make it write the screen out
//! and read it back in.

use std::cell::Cell;

use crate::pipeline::{SRC_H, SRC_W};
use crate::quad::Quad;

/// The near glow is the bright part blurred by `NEAR_SIGMA` game pixels; the wide one is the
/// near one blurred again by `WIDE_SIGMA`, five pixels in all. As cores/pico8-sdl/sdl_init.c.
const NEAR_SIGMA: f32 = 1.2;
const WIDE_SIGMA: f32 = 4.85;
const NEAR_GLOW: f32 = 0.35;
const WIDE_GLOW: f32 = 0.30;

pub(crate) struct Glow {
    prog: gl::types::GLuint,
    u_step: gl::types::GLint,
    u_sigma: gl::types::GLint,
    u_pairs: gl::types::GLint,
    u_bright: gl::types::GLint,
    u_weigh: gl::types::GLint,
    /// Near across, near down, wide across, then the glow.
    tex: [gl::types::GLuint; 4],
    fbo: [gl::types::GLuint; 4],
    /// The texture `tex[3]` holds the glow of, 0 for none.
    of: Cell<gl::types::GLuint>,
}

impl Glow {
    pub fn new() -> Result<Self, crate::surface::GfxError> {
        let prog = crate::shaders::program(crate::shaders::BLIT_VERT, crate::shaders::GLOW_FRAG)?;
        let tex = [0, 1, 2, 3].map(|_| {
            crate::gl::texture(SRC_W, SRC_H, gl::LINEAR, gl::CLAMP_TO_EDGE, gl::RGBA, None)
        });
        let mut fbo = [0; 4];
        unsafe {
            gl::GenFramebuffers(4, fbo.as_mut_ptr());
            for (f, t) in fbo.iter().zip(tex) {
                gl::BindFramebuffer(gl::FRAMEBUFFER, *f);
                gl::FramebufferTexture2D(
                    gl::FRAMEBUFFER,
                    gl::COLOR_ATTACHMENT0,
                    gl::TEXTURE_2D,
                    t,
                    0,
                );
            }
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::UseProgram(prog);
            gl::Uniform1i(crate::gl::uniform_location(prog, "u_tex"), 0);
            gl::Uniform1i(crate::gl::uniform_location(prog, "u_add"), 1);
            gl::Uniform4f(
                crate::gl::uniform_location(prog, "u_uv"),
                0.0,
                0.0,
                1.0,
                1.0,
            );
        }
        Ok(Glow {
            prog,
            u_step: crate::gl::uniform_location(prog, "u_step"),
            u_sigma: crate::gl::uniform_location(prog, "u_sigma"),
            u_pairs: crate::gl::uniform_location(prog, "u_pairs"),
            u_bright: crate::gl::uniform_location(prog, "u_bright"),
            u_weigh: crate::gl::uniform_location(prog, "u_weigh"),
            tex,
            fbo,
            of: Cell::new(0),
        })
    }

    /// Forget the glow: the next `of` blurs again.
    pub fn reset(&self) {
        self.of.set(0);
    }

    /// The glow of `game`, blurred now unless it already is.
    pub fn of(&self, game: gl::types::GLuint, quad: &Quad) -> gl::types::GLuint {
        if self.of.get() != game {
            self.blur(game, quad);
        }
        self.tex[3]
    }

    /// Blurs `game`, whose pixels have just changed.
    pub fn blur(&self, game: gl::types::GLuint, quad: &Quad) {
        let (x, y) = (1.0 / SRC_W as f32, 1.0 / SRC_H as f32);
        // Source, step, sigma, pairs, bright, then how much of the blur and of tex[1].
        let passes = [
            (game, [x, 0.0], NEAR_SIGMA, 0.0, 1.0, [1.0, 0.0]),
            (self.tex[0], [0.0, y], NEAR_SIGMA, 0.0, 0.0, [1.0, 0.0]),
            (self.tex[1], [x, 0.0], WIDE_SIGMA, 1.0, 0.0, [1.0, 0.0]),
            (
                self.tex[2],
                [0.0, y],
                WIDE_SIGMA,
                1.0,
                0.0,
                [WIDE_GLOW, NEAR_GLOW],
            ),
        ];
        unsafe {
            let mut fb = 0;
            let mut view = [0; 4];
            let mut unit1 = 0;
            gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fb);
            gl::GetIntegerv(gl::VIEWPORT, view.as_mut_ptr());
            gl::ActiveTexture(gl::TEXTURE1);
            gl::GetIntegerv(gl::TEXTURE_BINDING_2D, &mut unit1);
            gl::Viewport(0, 0, SRC_W as i32, SRC_H as i32);
            gl::UseProgram(self.prog);
            for (pass, (src, step, sigma, pairs, bright, weigh)) in passes.into_iter().enumerate() {
                // Never with tex[1] bound while it is drawn into.
                gl::ActiveTexture(gl::TEXTURE1);
                gl::BindTexture(gl::TEXTURE_2D, if pass == 3 { self.tex[1] } else { 0 });
                gl::ActiveTexture(gl::TEXTURE0);
                gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo[pass]);
                gl::Uniform2f(self.u_step, step[0], step[1]);
                gl::Uniform1f(self.u_sigma, sigma);
                gl::Uniform1f(self.u_pairs, pairs);
                gl::Uniform1f(self.u_bright, bright);
                gl::Uniform2f(self.u_weigh, weigh[0], weigh[1]);
                gl::BindTexture(gl::TEXTURE_2D, src);
                quad.draw();
            }
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, unit1 as gl::types::GLuint);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindFramebuffer(gl::FRAMEBUFFER, fb as gl::types::GLuint);
            gl::Viewport(view[0], view[1], view[2], view[3]);
        }
        self.of.set(game);
    }
}

impl Drop for Glow {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteFramebuffers(4, self.fbo.as_ptr());
            gl::DeleteTextures(4, self.tex.as_ptr());
            gl::DeleteProgram(self.prog);
        }
    }
}
