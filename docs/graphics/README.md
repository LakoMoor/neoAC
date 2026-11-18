# Graphics: before and after

`before/` and `after/` each hold 72 shots now (JPEG) (3 tracks x 4 cars x 6 looks: day, sunset and night, dry and in rain). `before/` is the build at commit `a97f1e0` (with static reflections: that build blacks out with live cube-map faces), `after/` the current one. The same 36 shots each:
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

## More checks

- `checks/shadows_four_distances.png`: shadows from 0.5 m to about 4 m of camera distance on three tracks (a car on tarmac, a car under trees, a building and a
  tree on gravel, a canopy over a pit lane): soft edges everywhere, no acne, no floating shadows.
- `checks/wet_road_mirror_poles_car.png`: the wet road mirrors the light poles, trees, the lorry and the car; the lamps' own lens glow is
  mirrored too but it is faint because the lenses glow little in the frame.
- Flicker, 16 screenshots 0.4 s apart of a standing car in front of foliage (akina, day): pixels that flip by more than 40 levels between two frames are 0.01 %
  before and after; the mean frame difference is 0.47 before and 0.55 after (wind in the trees; the screenshots are 2.5 Hz, so this finds a shimmer that
  lasts, not a single-frame one).
- Bloom: AC's own glare (YEBIS) is on and works but is gentle: with `GLARE=0` the pixels in a 15 px ring around the brightest light change by 0.58 levels
  of 255 on average. A bloom of our own in the present pass cost 7–10 fps and showed little, so it was left out.
- Frame rates are sensitive to what else the Mac runs: with another process holding a core, the same scene measured 40–43 fps and 60 fps when idle.
  The table above is the best of repeated runs; AC's six-face live cube map is the one setting that costs a lot on its own.

## Web demo at night (`web/`)

`?hour=18.8&weather=7_heavy_clouds` (or any `hour`) sets the sun angle `(hour - 13) * 16` and the demo's debug picture darkens by it. In Chrome on the M4,
with the Lights button tapped and the camera turned round the car by a one-finger drag (a touch swipe), both pictures, WebGPU
(`night_headlights_webgpu.png`) and WebGL2 (`night_headlights_webgl2.png`), show the headlight beam on the ground and wall ahead of the nose;
`night_lights_off_webgpu.png` is the same hour with the lights off. The demo's pack has no AC shaders, so it always draws the debug picture;
AC's own renderer (Full) is not in the demo.

## The 6 looks (every track and car)

| Look | hour, weather, grip | File suffix |
|---|---|---|
| day, dry | 13, 3_clear | `_day` |
| sunset, dry | 17.4, 5_light_clouds | `_sunset` |
| night, rain | 18.8, 7_heavy_clouds, grip 0.6, lamps on | `_night_rain` |
| day, rain | 13, 6_mid_clouds, grip 0.6 | `_day_rain` |
| sunset, rain | 17.4, 7_heavy_clouds, grip 0.6 | `_sunset_rain` |
| night, dry | 18.8, 7_heavy_clouds, lamps on | `_night_dry` |

(AC's sun does not set further than about 18.3 h; a clear sky at 18.8 h is still a low sun, so night is made with heavy clouds. With the lamps forced on
in that low sun, AC's own light on the sunlit ground blows out; that is AC's engine and the same in the old build.)
