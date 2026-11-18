# neoAC

**Assetto Corsa, native on macOS and Linux.** neoAC runs the Assetto Corsa 1.16 engine as a real Mac and Linux program: written in Rust, drawn with wgpu (Metal on macOS, Vulkan on Linux), with its own sound mixer and a launcher. No Wine, no emulation.

**[Website](https://lakomoor.github.io/neoAC/)** · screenshots, features and status.

> **Built on [rustyAC](https://github.com/thesupersupersigma/rustyAC).** neoAC is not a fork of the game and not a new engine: it is built on rustyAC, the open-source Rust rewrite of Assetto Corsa by thesupersupersigma. The physics, the renderer and most of the code are theirs; licenses and copyright notices are kept (see [LICENSING.md](LICENSING.md)). rustyAC's own README is kept in [docs/RUSTYAC_README.md](docs/RUSTYAC_README.md).
>
> Not affiliated with or endorsed by Kunos Simulazioni or 505 Games. No game files are included: neoAC loads the cars, tracks, sounds and shaders from **your own legal copy** of Assetto Corsa 1.16.

## What neoAC adds on top

- **macOS and Linux build** with a double-click start (`rustyac.command` / `rustyac.sh`).
- **A launcher** (GPUI): cars and tracks with previews, skins, assists from newbie to pro, keys, graphics (`video.ini`), content (install a mod from a zip), weather, time of day and track grip, and a Shaders Patch page.
- **Lights from the car's own data**: headlight pools, lamp positions and colours come from each car's `data/lights.ini` and meshes. Soft turn-on for cars that do not bring a `lights.ksanim`.
- **Rain and wet roads**: rain anchored in the world, grip from the chosen track state.
- **Chase camera**: right button turns it round the car and the turn stays (double right click brings it back), the wheel zooms.
- **Walk out of the car**: **F**, first person (GTA style).
- **Debug panel**: **F10**, frame times, telemetry, tyre contact points, brightness and detail distance.
- **Own sound mixer** for the game's sound banks (no FMOD).
- **Render density and a bicubic upscale with sharpening.**
- **Data tests** across the cars: every `.ini` parses, `lights.ini` is well formed and names meshes the model has, key physics files hold their keys, power and weight agree with the car's spec sheet.

## Status

A preview. Works: driving on macOS with the original renderer, HUD and menus (keyboard and mouse), the launcher, headlights, brake lamps, rain, walking, joining a Kunos `acServer` on the local network. Not yet: Linux is only compile-checked, gamepad, Python apps, turn signals (the base game has none and no car here defines them), exact sound, hosting a server from the game.

## Run it

You need your own **Assetto Corsa 1.16** (the Windows files: `content` and `system`) and [Rust](https://rustup.rs).

```sh
git clone https://github.com/LakoMoor/neoAC
cd neoAC
export AC_ROOT="/path/to/assettocorsa"

# straight into a car
cargo run --release -p rustyac-native -- --car ks_nissan_gtr --track drift

# or package the launcher into the game folder, then double-click rustyac.command
sh packaging/package_unix.sh "$AC_ROOT"
```

Options worth knowing: `--weather <preset>`, `--hour <0-23>`, `--grip <0.4-1>`, `--orbit <yaw>,<pitch>[,<distance>]`, `--debug-panel`, `--shot <png>`. Run `rustyac-play --help` for all of them.

## License

GPL-3.0-or-later for the game, physics, renderer, sound and launcher; parts under MIT OR Apache-2.0 and BSD-3-Clause. Details in [LICENSING.md](LICENSING.md). The screenshots on the website show Assetto Corsa's content and belong to their owners.
