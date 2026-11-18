# SPDX-License-Identifier: GPL-3.0-or-later
"""Assembles the website's demo (site/demo/) from the browser build (dist-web/site/).

    python web/build.py --no-opt                  # the browser build, first
    python web/make_demo.py                       # then this; the pack (site/demo/preview/) is kept

The engine's page becomes play.html; web/demo/landing.html is the page people open (index.html).
The pack is made with tools/web_pack (a car and a track).
"""
import os, shutil

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
src = os.path.join(root, 'dist-web', 'site')
out = os.path.join(root, 'site', 'demo')
keep = {'preview', 'img'}
os.makedirs(out, exist_ok=True)
for name in os.listdir(out):
    if name not in keep:
        path = os.path.join(out, name)
        shutil.rmtree(path) if os.path.isdir(path) else os.remove(path)
for name in os.listdir(src):
    if name in ('preview',):
        continue
    s, d = os.path.join(src, name), os.path.join(out, name)
    shutil.copytree(s, d) if os.path.isdir(s) else shutil.copy2(s, d)
os.replace(os.path.join(out, 'index.html'), os.path.join(out, 'play.html'))
shutil.copy2(os.path.join(root, 'web', 'demo', 'landing.html'), os.path.join(out, 'index.html'))
print('site/demo ready:', len(os.listdir(out)), 'entries')
