# Graphics: before and after

`before/` is the build at commit `a97f1e0`, `after/` the current one. The same 36 shots each:
3 tracks (drift, nevadadrift, mount_akina_2017) × 4 cars (GT-R, Huracán Performante, Lancer Evo VI, Skyline R34)
× day / sunset / night in rain, High preset (4096 shadow map, 4× MSAA, live cube-map reflections), 1280×720 on an M4 (Metal).
`compare/` has four side-by-side pairs (left before, right after).

| # | Condition | State | Where to look |
|---|-----------|-------|----------------|
| 1 | Reflections | Wet road mirrors the scene (a ray walked through the picture and its depth), the car included; paint and glass use the cube map (live with `[CUBEMAP] FACES_PER_FRAME`), which is subtle | `*_night_rain`, `compare/nevadadrift_ks_nissan_gtr_night_rain.jpg` |
| 2 | Soft shadows | Every shadow-map lookup of AC's ported shaders is now five filtered taps (`rustyac-dxbc` `SAMPLE_C`), a wider penumbra on near and far objects; 60 fps kept | `*_day`, `compare/drift_ks_lamborghini_huracan_performante_day.jpg` |
| 3 | Light | Headlights lit from scene depth on walls, trees and ground, soft cone edge. Glints on paint and glass: the sun's mirror image (a normal from the depth, on the body's blocks) and a sky rim at grazing angles | `*_night_rain` |
| 4 | Post | S tone curve, vignette, exposure by time of day (brighter at night); bloom is AC's own post-process | `compare/drift_ks_nissan_skyline_r34_sunset.jpg` |
| 5 | Anti-aliasing | 4× MSAA + FXAA as before. Two frames 0.25 s apart, standing car, foliage region: mean difference 0.63 / 0.38 before and 0.64 / 0.38 after, so nothing new shimmers (what is left is the trees' wind) | `compare/mount_akina_2017_tw_mitsubishi_evo6_tme_day.jpg` |
| 6 | Frame rate | 60.0 fps at 1920×1080 High on M4 (`rustyac-play --quit-after`, printed at exit; capped at 60) | |

Web demo: the headlights, a drag / pinch / double tap for the camera and a Lights button work in the browser (WebGPU
checked in Chrome by day; night lights and the WebGL2 picture are not checked).
