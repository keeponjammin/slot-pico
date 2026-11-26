//! The glow of the CRT look: the bright part of the game's picture, blurred across and then
//! down, at the game texture's own size. The PICO-8 SDL shim draws the same glow for the owner's
//! PICO-8 (cores/pico8-sdl/sdl_init.c); this one is for fake-08's frames.
//!
//! Both passes are small (one texel per game pixel), and run when a new frame is uploaded,
//! before anything is drawn to the screen: the Mali draws a frame in tiles, and turning to
//! another framebuffer once the screen has draws pending would make it write the screen out
//! and read it back in.

use std::cell::Cell;

use crate::pipeline::{SRC_H, SRC_W};
use crate::quad::Quad;

pub(crate) struct Glow {
    prog: gl::types::GLuint,
    u_step: gl::types::GLint,
    u_bright: gl::types::GLint,
    /// Across, then down: the second is the glow.
    tex: [gl::types::GLuint; 2],
    fbo: [gl::types::GLuint; 2],
    /// The texture `tex[1]` holds the glow of, 0 for none.
    of: Cell<gl::types::GLuint>,
}

impl Glow {
    pub fn new() -> Result<Self, crate::surface::GfxError> {
        let prog = crate::shaders::program(crate::shaders::BLIT_VERT, crate::shaders::GLOW_FRAG)?;
        let tex = [0, 1].map(|_| {
            crate::gl::texture(SRC_W, SRC_H, gl::LINEAR, gl::CLAMP_TO_EDGE, gl::RGBA, None)
        });
        let mut fbo = [0; 2];
        unsafe {
            gl::GenFramebuffers(2, fbo.as_mut_ptr());
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
            u_bright: crate::gl::uniform_location(prog, "u_bright"),
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
        self.tex[1]
    }

    /// Blurs `game`, whose pixels have just changed.
    pub fn blur(&self, game: gl::types::GLuint, quad: &Quad) {
        unsafe {
            let mut fb = 0;
            let mut view = [0; 4];
            gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fb);
            gl::GetIntegerv(gl::VIEWPORT, view.as_mut_ptr());
            gl::Viewport(0, 0, SRC_W as i32, SRC_H as i32);
            gl::UseProgram(self.prog);
            gl::ActiveTexture(gl::TEXTURE0);
            for (pass, (src, step, bright)) in [
                (game, [2.0 / SRC_W as f32, 0.0], 1.0),
                (self.tex[0], [0.0, 2.0 / SRC_H as f32], 0.0),
            ]
            .into_iter()
            .enumerate()
            {
                gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo[pass]);
                gl::Uniform2f(self.u_step, step[0], step[1]);
                gl::Uniform1f(self.u_bright, bright);
                gl::BindTexture(gl::TEXTURE_2D, src);
                quad.draw();
            }
            gl::BindFramebuffer(gl::FRAMEBUFFER, fb as gl::types::GLuint);
            gl::Viewport(view[0], view[1], view[2], view[3]);
        }
        self.of.set(game);
    }
}

impl Drop for Glow {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteFramebuffers(2, self.fbo.as_ptr());
            gl::DeleteTextures(2, self.tex.as_ptr());
            gl::DeleteProgram(self.prog);
        }
    }
}
