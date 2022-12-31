//! Fidget is a library of infrastructure and algorithms for complex closed-form
//! implicit surfaces.
//!
//! An **implicit surface** is a function `f(x, y, z)`, where `x`, `y`, and `z`
//! represent a position in 3D space.  By convention, if `f(x, y, z) < 0`, then
//! that position is **inside** the shape; if it's `> 0`, then that position is
//! **outside** the shape; otherwise, it's on the boundary of the shape.
//!
//! A **closed-form** implicit surface means that the function is given as a
//! fixed expression built from closed-form operations (addition, subtraction,
//! etc), with no mutable state.  This is in contrast to
//! [ShaderToy](https://www.shadertoy.com/)-style implicit surface functions,
//! which often include mutable state and make control-flow decisions at
//! runtime.
//!
//! Finally, **complex** means that that the library scales to expressions with
//! thousands of clauses.
//!
//! Details on overall project status are in the
//! [project's README](https://github.com/mkeeter/fidget);
//! the rest of this page is a quick tour through the library APIs.
//!
//! # Shape construction
//! Shapes are constructed within a
//! [`fidget::context::Context`](crate::context::Context).  A context serves as
//! an arena-style allocator, doing local deduplication and other simple
//! optimizations (e.g. constant folding).
//!
//! Shapes can be constructed manually, using functions on a context:
//! ```
//! use fidget::context::Context;
//!
//! let mut ctx = Context::new();
//! let x = ctx.x();
//! let y = ctx.y();
//! let sum = ctx.add(x, y).unwrap();
//! ```
//!
//! As an alternative, Fidget includes bindings to [Rhai](https://rhai.rs), a
//! simple Rust-native scripting language, in the [`fidget::rhai`
//! namespace](crate::rhai).  These bindings offer a terser way to construct
//! a shape from a script:
//!
//! ```
//! use fidget::rhai::eval;
//!
//! let (sum, ctx) = eval("x + y").unwrap();
//! ```
//!
//! # Evaluation
//! The main operation performed on an implicit surface is **evaluation**, i.e.
//! passing it some position `(x, y, z)` and getting back a result.  This will
//! be done _a lot_, so it has to be fast.
//!
//! Before evaluation, a shape must be baked into a [`Tape`](crate::eval::Tape).
//! This is performed by [`Context::get_tape`](crate::Context::get_tape):
//! ```
//! use fidget::{rhai::eval, vm};
//!
//! let (sum, ctx) = eval("x + y").unwrap();
//! let tape = ctx.get_tape::<vm::Eval>(sum).unwrap();
//! assert_eq!(tape.len(), 3); // X, Y, and (X + Y)
//! ```
//!
//! A tape is a set of operations for a very simple virtual machine; the
//! expression above would be something like
//! ```text
//! $0 = INPUT 0   // X
//! $1 = INPUT 1   // Y
//! $2 = ADD $0 $1 // (X + Y)
//! ```
//!
//! The `Tape` is parameterized by a particular
//! [evaluator family](crate::eval::Family); in `ctx.get_tape::<vm::Eval>(...)`,
//! the associated family is `vm::Eval`.
//!
//! (Parameterizing the tape is required because different evaluator families
//! have different numbers of [available
//! registers](crate::eval::Family::REG_LIMIT), which affects tape planning;
//! don't worry, this won't be on the test)
//!
//! At the moment, Fidget implements two evaluator families:
//!
//! - [`fidget::jit::Eval`](crate::jit::Eval) performs fast evaluation by
//!   compiling shapes down to native code.  This is only functional on an ARM64
//!   system running natively.
//! - [`fidget::vm::Eval`](crate::vm::Eval) evaluates
//!   using an interpreter.  This is slower, but can run in more situations (e.g.
//!   x86 machines or in WebAssembly).
//!
//! Looking at the [`eval::Family`](crate::eval::Family) trait, you may notice
//! that it requires four different kinds of evaluation:
//!
//! - Single-point evaluation
//! - Interval evaluation
//! - Evaluation on an array of points, returning `f32` values
//! - Evaluation on an array of points, returning partial derivatives with
//!   respect to `x, y, z`
//!
//! These evaluation flavors are used in rendering:
//! - Interval evaluation can conservatively prove large regions of space to be
//!   empty or full, at which point they don't need to be considered further.
//! - Array-of-points evaluation speeds up calculating occupancy (inside /
//!   outside) when given a set of voxels by amortizing dispatch overhead.
//! - At the surface of the model, partial derivatives represent normals and
//!   can be used for shading.
//!
//! Here's a simple example of interval evaluation:
//! ```
//! use fidget::{rhai::eval, vm};
//!
//! let (sum, ctx) = eval("x + y").unwrap();
//! let tape = ctx.get_tape::<vm::Eval>(sum).unwrap();
//! let mut interval_eval = tape.new_interval_evaluator();
//! let (out, _) = interval_eval.eval(
//!         [0.0, 1.0], // X
//!         [2.0, 3.0], // Y
//!         [0.0, 0.0], // Z
//!         &[]         // variables (unused)
//!     ).unwrap();
//! assert_eq!(out, [2.0, 4.0].into());
//! ```
//!
//! # Tape simplification
//! Interval evaluation serves two purposes.  As we already mentioned, it can be
//! used to prove large regions empty or filled, which lets us do less work when
//! rendering.  In addition, it can discover **sections of the tape** that are
//! always inactive in a particular spatial region.
//!
//! Consider evaluating `f(x, y, z) = max(x, y)` with `x = [0, 1]` and
//! `y = [2, 3]`:
//! ```
//! use fidget::{rhai::eval, vm};
//!
//! let (sum, ctx) = eval("min(x, y)").unwrap();
//! let tape = ctx.get_tape::<vm::Eval>(sum).unwrap();
//! let mut interval_eval = tape.new_interval_evaluator();
//! let (out, simplify) = interval_eval.eval(
//!         [0.0, 1.0], // X
//!         [2.0, 3.0], // Y
//!         [0.0, 0.0], // Z
//!         &[]         // variables (unused)
//!     ).unwrap();
//! assert_eq!(out, [0.0, 1.0].into());
//! ```
//!
//! In the evaluation region `x = [0, 1]; y = [2, 3]`, `x` is **strictly less
//! than** `y` in the `min(x, y)` clause.  This means that we can simplify the
//! tape from `f(x, y, z) = min(x, y) → f(x, y, z) = x`.
//!
//! Simplification is done with
//! [`TracingEvalResult::simplify`](crate::eval::tracing::TracingEvalResult::simplify),
//! using the `TracingEvalResult` returned from
//! [`IntervalEval::eval`](crate::eval::interval::IntervalEval::eval).
//!
//! ```
//! # use fidget::{rhai::eval, vm};
//! # let (sum, ctx) = eval("min(x, y)").unwrap();
//! # let tape = ctx.get_tape::<vm::Eval>(sum).unwrap();
//! # let mut interval_eval = tape.new_interval_evaluator();
//! # let (out, simplify) = interval_eval.eval(
//! #         [0.0, 1.0], // X
//! #         [2.0, 3.0], // Y
//! #         [0.0, 0.0], // Z
//! #         &[]         // variables (unused)
//! #     ).unwrap();
//! // (same code as above)
//! assert_eq!(interval_eval.tape().len(), 3);
//! let new_tape = simplify.unwrap().simplify().unwrap();
//! assert_eq!(new_tape.len(), 1); // just the 'X' term
//! ```
//!
//! Remember that this simplified tape is only valid for points (or intervals)
//! within the interval region `x = [0, 1]; y = [2, 3]`.  It's up to you to make
//! sure this is upheld!
//!
//! # Rasterization
//! At the moment, Fidget uses all of this machinery to build two user-facing
//! algorithms: rasterization of implicit surfaces in 2D and 3D.
//!
//! They are implemented in the [`fidget::render` namespace](crate::render).
//!
//! Here's a quick example:
//! ```
//! use fidget::context::Context;
//! use fidget::rhai::eval;
//! use fidget::vm;
//! use fidget::render::{BitRenderMode, RenderConfig};
//!
//! let (shape, ctx) = eval("sqrt(x*x + y*y) - 1").unwrap();
//! let cfg = RenderConfig::<2> {
//!     image_size: 32,
//!     ..RenderConfig::default()
//! };
//! let out = cfg.run::<vm::Eval, _>(shape, ctx, &BitRenderMode).unwrap();
//! let mut iter = out.iter();
//! for y in 0..cfg.image_size {
//!     for x in 0..cfg.image_size {
//!         if *iter.next().unwrap() {
//!             print!("XX");
//!         } else {
//!             print!("  ");
//!         }
//!     }
//!     println!();
//! }
//!
//! // This will print
//! //           XXXXXXXXXX
//! //       XXXXXXXXXXXXXXXXXX
//! //     XXXXXXXXXXXXXXXXXXXXXX
//! //   XXXXXXXXXXXXXXXXXXXXXXXXXX
//! //   XXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! //   XXXXXXXXXXXXXXXXXXXXXXXXXX
//! //   XXXXXXXXXXXXXXXXXXXXXXXXXX
//! //     XXXXXXXXXXXXXXXXXXXXXX
//! //       XXXXXXXXXXXXXXXXXX
//! //           XXXXXXXXXX
//! ```
//!
//! # Similar projects
//! Fidget overlaps with various projects in the implicit modeling space:
//!
//! - [Antimony: CAD from a parallel universe](https://mattkeeter.com/projects/antimony)*
//! - [`libfive`: Infrastructure for solid modeling](https://libfive.com)*
//! - [Massively Parallel Rendering of Complex Closed-Form Implicit Surfaces (MPR)](https://github.com/mkeeter/mpr)*
//! - [ImplicitCAD: Powerful, Open-Source, Programmatic CAD](https://implicitcad.org/)
//! - [Ruckus: Procedural CAD For Weirdos](https://docs.racket-lang.org/ruckus/index.html)
//! - [Curv: a language for making art using mathematics](https://github.com/curv3d/curv)
//! - [sdf: Simple SDF mesh generation in Python](https://github.com/fogleman/sdf)
//! - Probably more; PRs welcome!
//!
//! *written by the same author
//!
//! (the MPR paper also cites
//! [many references](https://dl.acm.org/doi/10.1145/3386569.3392429#sec-ref)
//! to related academic work)
//!
//! Compared to these projects, Fidget is unique in having a native JIT **and**
//! using that JIT while performing tape simplification.  Situating it among
//! projects by the same author – which all use roughly the same rendering
//! strategies – it looks something like this:
//!
//! |                 | CPU               | GPU
//! |-----------------|-------------------|------
//! | **Interpreter** | `libfive`, Fidget | MPR
//! | **JIT**         | Fidget            | (please give me APIs to do this)
//!
//! Fidget's native JIT makes it _blazing fast_.
//! For example, here are rough benchmarks rasterizing [this model](https://www.mattkeeter.com/projects/siggraph/depth_norm@2x.png)
//! across three different implementations:
//!
//! Size  | `libfive` | MPR     | Fidget (VM) | Fidget (JIT)
//! ------|-----------|---------|-------------|---------------
//! 1024³ | 66.8 ms   | 22.6 ms | 61.7 ms     | 23.6 ms
//! 1536³ | 127 ms    | 39.3 ms | 112 ms      | 45.4 ms
//! 2048³ | 211 ms    | 60.6 ms | 184 ms      | 77.4 ms
//!
//! `libfive` and Fidget are running on an M1 Max CPU; MPR is running on a GTX
//! 1080 Ti GPU.  We see that Fidget's interpreter is slightly better than
//! `libfive`, and Fidget's JIT is _nearly_ competitive with the GPU-based MPR.
//!
//! Fidget is missing a bunch of features that are found in more mature
//! projects.  For example, it does not implement meshing, and only includes a
//! debug GUI.
//!
//! # Feature flags
#![doc = document_features::document_features!()]

// Re-export everything from fidget::core into the top-level namespace
mod core;
pub use crate::core::*;

mod error;
pub use error::Error;

#[cfg(feature = "render")]
pub mod render;

#[cfg(feature = "rhai")]
pub mod rhai;

#[cfg(feature = "jit")]
pub mod jit;
