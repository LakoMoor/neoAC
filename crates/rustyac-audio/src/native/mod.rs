// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's sound without FMOD: the banks read from their bytes ([`model`]), their events played by
//! a mixer of our own ([`mixer`]). See `docs/port/sound_native.md`.

#[cfg(all(unix, not(target_family = "wasm")))]
pub mod api;
pub mod mixer;
pub mod model;
