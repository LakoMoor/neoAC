# Graphics: before and after

`before/` is the build at commit `a97f1e0` (with static reflections: that build blacks out with live cube-map faces), `after/` the current one. The same 36 shots each:
3 tracks (drift, nevadadrift, mount_akina_2017) × 4 cars (GT-R, Huracán Performante, Lancer Evo VI, Skyline R34)
× day / sunset / night in rain, High preset (4096 shadow map, 4× MSAA, live cube-map reflections), 1280×720 on an M4 (Metal).
`compare/` has four side-by-side pairs (left before, right after).

| # | Condition | State | Where to look |
|---|-----------|-------|----------------|
| 1 | Reflections | Wet road mirrors the scene (a ray walked through the picture and its depth), the car included; paint and glass mirror the scene too: the eye's ray bounced off the body (normal from the depth) is walked through the picture, the sky where it misses; glass (dark) mirrors more, paint at grazing angles. Look at the rear window and the wing in `compare/drift_ks_nissan_gtr_day.jpg` | `*_night_rain`, `compare/nevadadrift_ks_nissan_gtr_night_rain.jpg` |
| 2 | Soft shadows | Every shadow-map lookup of AC's ported shaders is now five filtered taps (`rustyac-dxbc` `SAMPLE_C`), a wider penumbra on near and far objects; 60 fps kept | `*_day`, `compare/drift_ks_lamborghini_huracan_performante_day.jpg` |
| 3 | Light | Headlights lit from scene depth on walls, trees and ground, soft cone edge. Glints on paint and glass: the sun's mirror image (visible on the windscreen at some angles; zero at night, when there is no sun) and a sky rim at grazing angles | `*_night_rain` |
| 4 | Post | S tone curve, vignette, exposure by time of day (brighter at night); bloom is AC's own post-process | `compare/drift_ks_nissan_skyline_r34_sunset.jpg` |
| 5 | Anti-aliasing | 4× MSAA + FXAA as before. Two frames 0.25 s apart, standing car, foliage region: mean difference 0.63 / 0.38 before and 0.64 / 0.38 after, so nothing new shimmers (what is left is the trees' wind) | `compare/mount_akina_2017_tw_mitsubishi_evo6_tme_day.jpg` |
| 6 | Frame rate | **Not 60 everywhere at 1080p.** See the table below | `fps.md` |

Web demo: the headlights, a drag / pinch / double tap for the camera and a Lights button work in the browser (WebGPU
checked in Chrome by day; night lights and the WebGL2 picture are not checked).

## Extra checks (`checks/`)

- `bloom_off_on_night.png`: post-process off (left) and on (right), night in rain, headlights on. The glow is there around the LED strip and
  lamps but small: AC's bloom only lifts very bright pixels, and a dark night frame has few.
- `shadows_near_far.png`: a close camera (a car's shadow on tarmac, left) and a far one (tree and house shadows on the ground, right):
  soft edges, no acne, no floating shadows.
- `glint_paint_smooth.png`: the sun's glint on a door; the normal comes from neighbours three pixels away so the depth's steps do not show.
- Night, wet: the road mirrors trees, a lorry and the car itself (nevadadrift and drift in rain); the bonnet mirrors the sky and trees. The sun glint is
  zero at night by design (no sun); the lamps' light on the road is the night's glint.

## Frame rate at 1920×1080, High preset, M4 (8 s runs, `rustyac-play --quit-after`; capped at 60)

The wet-road ray walk was the whole cost of rain (60 fps with it off); it is now 8 steps and skips far road.

| Scene (Nissan GT-R unless said) | static cube map (default `video.ini`) | live cube map, 2 faces | live, 6 faces (High preset) |
|---|---|---|---|
| nevadadrift, day | 60.0 | 54 | 46 |
| nevadadrift, night, rain, headlights | 60.0 | 40 | 36 |
| mount_akina_2017, day | 60.0 | 56 | 46 |
| mount_akina_2017, night, rain, headlights | 59.6 | 49 | 43 |
| drift, every weather (earlier runs) | 60.0 | | 60.0 |
| nevadadrift, Huracán, night, rain, headlights | 42–45 (the scene is 53 even with no rain and no lamps) | 36 | 29 |

With the default settings (no `[CUBEMAP]` in `video.ini`) the goal holds on every scene but the Huracán on nevadadrift in rain
with headlights. AC's live cube map (`FACES_PER_FRAME`, six faces in the High preset) costs 10–25 fps on the heavy tracks; paint
and glass now mirror the scene through the screen-space pass, so the static cube map costs no look.
