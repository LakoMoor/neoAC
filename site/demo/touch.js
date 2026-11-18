// SPDX-License-Identifier: GPL-3.0-or-later

// On-screen controls for a phone or a tablet: they press the same keys the page already reads
// (arrows for steering, gas and brake; Space and E for the gears; C for the camera), by sending
// keyboard events to the window. Nothing else in the page changes.
//
// Shown on a touch screen, or with ?touch=1 (hidden with ?touch=0). Tilt steering: the
// "Tilt" button asks for the device's motion (iOS asks once) and turns the phone into a wheel.

const forced = new URLSearchParams(location.search).get('touch');
const touchScreen = matchMedia('(pointer: coarse)').matches || navigator.maxTouchPoints > 0;
const enabled = forced === '1' || (forced !== '0' && touchScreen);

const down = new Set();

function send(code, key, isDown) {
  if (isDown === down.has(code)) return;
  if (isDown) down.add(code);
  else down.delete(code);
  window.dispatchEvent(new KeyboardEvent(isDown ? 'keydown' : 'keyup', { code, key, bubbles: true, cancelable: true }));
}

const STYLE = `
#touch{position:fixed;inset:0;z-index:50;pointer-events:none;display:none;user-select:none;-webkit-user-select:none;touch-action:none;
  font:600 13px/1 system-ui,sans-serif;color:#fff}
#touch.on{display:block}
#touch .pad{position:absolute;pointer-events:auto;display:grid;place-items:center;border-radius:22px;border:2px solid #ffffff55;
  background:#0009;backdrop-filter:blur(6px);-webkit-backdrop-filter:blur(6px);touch-action:none;transition:background .08s,transform .08s}
#touch .pad.held{background:#ff3b30cc;transform:scale(.96)}
#touch .pad b{font-size:26px;font-weight:700}
#touch .pad small{display:block;font-size:11px;opacity:.75;margin-top:4px;font-weight:600;letter-spacing:.04em}
#touch .steer{bottom:max(18px,env(safe-area-inset-bottom));width:min(22vw,120px);height:min(22vw,120px)}
#touch .left{left:max(14px,env(safe-area-inset-left))}
#touch .right{left:calc(max(14px,env(safe-area-inset-left)) + min(22vw,120px) + 12px)}
#touch .gas{right:max(14px,env(safe-area-inset-right));bottom:max(18px,env(safe-area-inset-bottom));width:min(20vw,110px);height:min(34vw,190px)}
#touch .brake{right:calc(max(14px,env(safe-area-inset-right)) + min(20vw,110px) + 12px);bottom:max(18px,env(safe-area-inset-bottom));width:min(20vw,110px);height:min(24vw,130px)}
#touch .tops{position:absolute;top:max(10px,env(safe-area-inset-top));left:max(14px,env(safe-area-inset-left));right:max(14px,env(safe-area-inset-right));display:flex;gap:8px;justify-content:flex-end;pointer-events:none}
#touch .tops .pad{position:relative;height:44px;min-width:62px;padding:0 12px;font-size:13px;border-radius:14px}
#touch .tops .tilt{margin-right:auto}
#touch .tilt.held{background:#35d07fcc}
`;

function pad(parent, cls, label, hint, codes) {
  const el = document.createElement('div');
  el.className = `pad ${cls}`;
  el.innerHTML = hint ? `<div style="text-align:center"><b>${label}</b><small>${hint}</small></div>` : label;
  const ids = new Set();
  const set = (isDown) => {
    el.classList.toggle('held', isDown);
    for (const [code, key] of codes) send(code, key, isDown);
  };
  el.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    ids.add(e.pointerId);
    set(true);
    if (navigator.vibrate) navigator.vibrate(8);
  });
  const up = (e) => {
    ids.delete(e.pointerId);
    if (ids.size === 0) set(false);
  };
  el.addEventListener('pointerup', up);
  el.addEventListener('pointercancel', up);
  el.addEventListener('lostpointercapture', up);
  el.addEventListener('contextmenu', (e) => e.preventDefault());
  parent.appendChild(el);
  return el;
}

function build() {
  const style = document.createElement('style');
  style.textContent = STYLE;
  document.head.appendChild(style);
  const root = document.createElement('div');
  root.id = 'touch';
  document.body.appendChild(root);

  pad(root, 'steer left', '◀', '', [['ArrowLeft', 'ArrowLeft']]);
  pad(root, 'steer right', '▶', '', [['ArrowRight', 'ArrowRight']]);
  pad(root, 'gas', 'GAS', '', [['ArrowUp', 'ArrowUp']]);
  pad(root, 'brake', 'BRAKE', '', [['ArrowDown', 'ArrowDown']]);
  const tops = document.createElement('div');
  tops.className = 'tops';
  root.appendChild(tops);
  const tilt = document.createElement('div');
  tilt.className = 'pad tilt';
  tilt.textContent = 'Tilt';
  tops.appendChild(tilt);
  pad(tops, 'dn', 'Gear −', '', [['KeyQ', 'q']]);
  pad(tops, 'up', 'Gear +', '', [['KeyE', 'e']]);
  pad(tops, 'cam', 'View', '', [['KeyC', 'c']]);
  pad(tops, 'hand', 'Handbrake', '', [['KeyH', 'h']]);
  const exit = document.createElement('div');
  exit.className = 'pad exit';
  exit.textContent = '✕';
  exit.style.minWidth = '44px';
  exit.addEventListener('click', () => {
    if (confirm('Leave the drive?')) {
      window.onbeforeunload = null;
      location.href = './';
    }
  });
  tops.appendChild(exit);

  // tilt steering: the phone is the wheel (gamma when held sideways)
  let tiltOn = false;
  const onOrientation = (e) => {
    // held in landscape: beta is the steering angle, its sign depends on which way the phone is turned
    const angle = (screen.orientation && screen.orientation.angle) || window.orientation || 0;
    const raw = angle === 90 || angle === -270 ? e.beta : angle === 270 || angle === -90 ? -e.beta : e.gamma;
    if (raw == null) return;
    const dead = 6;
    send('ArrowLeft', 'ArrowLeft', raw < -dead);
    send('ArrowRight', 'ArrowRight', raw > dead);
  };
  tilt.addEventListener('click', async () => {
    if (!tiltOn) {
      if (typeof DeviceOrientationEvent !== 'undefined' && typeof DeviceOrientationEvent.requestPermission === 'function') {
        try {
          if ((await DeviceOrientationEvent.requestPermission()) !== 'granted') return;
        } catch {
          return;
        }
      }
      window.addEventListener('deviceorientation', onOrientation);
      tiltOn = true;
    } else {
      window.removeEventListener('deviceorientation', onOrientation);
      send('ArrowLeft', 'ArrowLeft', false);
      send('ArrowRight', 'ArrowRight', false);
      tiltOn = false;
    }
    tilt.classList.toggle('held', tiltOn);
    root.querySelectorAll('.steer').forEach((s) => (s.style.opacity = tiltOn ? '.35' : '1'));
  });
  return root;
}

if (enabled) {
  const start = () => {
    const root = build();
    // shown while a drive runs: the page hides its menu then
    const menu = document.getElementById('menu');
    const check = () => root.classList.toggle('on', forced === '1' || (menu && menu.hidden));
    setInterval(check, 400);
    check();
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
  else start();
  // no pinch-zoom or pull-to-refresh while driving
  document.addEventListener('touchmove', (e) => { if (down.size) e.preventDefault(); }, { passive: false });
}
