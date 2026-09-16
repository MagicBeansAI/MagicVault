#!/usr/bin/env python3
"""Frame actual window captures; captions never replace application content.

The edit manifest selects real files or intervals from windows/manifest.jsonl.
Requires Pillow, FFmpeg and Arial/DejaVu Sans. No network or app automation.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageOps

WIDTH, HEIGHT, FPS = 1600, 1000, 12
CHAPTERS = ['Connect MCP', 'Save for later', 'Just in time', 'Verify']


def font(size, bold=False):
    names = ([Path('/System/Library/Fonts/Supplemental') / ('Arial Bold.ttf' if bold else 'Arial.ttf'),
              Path(os.environ.get('WINDIR', 'C:/Windows')) / 'Fonts' / ('arialbd.ttf' if bold else 'arial.ttf'),
              Path('/usr/share/fonts/truetype/dejavu') / ('DejaVuSans-Bold.ttf' if bold else 'DejaVuSans.ttf')])
    for name in names:
        if name.exists():
            return ImageFont.truetype(str(name), size)
    raise RuntimeError('Install Arial or DejaVu Sans for reproducible captions')


def wrapped(draw, text, face, maximum):
    lines, line = [], ''
    for word in text.split():
        candidate = f'{line} {word}'.strip()
        if draw.textlength(candidate, font=face) > maximum and line:
            lines.append(line)
            line = word
        else:
            line = candidate
    return lines + [line]


def compose(source, scene, elapsed, duration):
    canvas = Image.new('RGB', (WIDTH, HEIGHT), '#0b0e19')
    draw = ImageDraw.Draw(canvas)
    for y in range(HEIGHT):
        strength = max(0, 1 - abs(y - 290) / 700)
        draw.line((0, y, WIDTH, y), fill=(11 + int(8*strength), 14 + int(5*strength), 25 + int(16*strength)))
    draw.rounded_rectangle((36, 27, 80, 71), radius=13, fill='#7857e8')
    draw.text((58, 49), 'M', font=font(26, True), fill='white', anchor='mm')
    draw.text((96, 49), 'MagicVault', font=font(29, True), fill='#f8f7ff', anchor='lm')
    draw.text((263, 50), '+ Codex', font=font(25), fill='#b7b2ce', anchor='lm')
    draw.rounded_rectangle((1312, 31, 1564, 67), radius=18, fill='#203a38')
    draw.ellipse((1328, 45, 1336, 53), fill='#7fe0c5')
    draw.text((1348, 49), 'REAL WEBSITE · MCP', font=font(17, True), fill='#b5f2e1', anchor='lm')
    chapter = scene['chapter']
    if chapter not in CHAPTERS:
        raise ValueError('Unknown chapter')
    x = 40
    for index, label in enumerate(CHAPTERS):
        active = label == chapter
        face = font(17, active)
        text = f'{index+1:02}  {label}'
        span = int(draw.textlength(text, font=face)) + 32
        draw.rounded_rectangle((x, 87, x+span, 121), radius=16,
                               fill='#372957' if active else '#1c2032')
        draw.text((x+16, 104), text, font=face, fill='#e8ddff' if active else '#9ca4bd', anchor='lm')
        x += span + 12
    draw.text((1560, 104), scene.get('label', 'MagicVault 0.9.0 · alpha'),
              font=font(17), fill='#a3abc1', anchor='rm')

    shot = Image.open(source).convert('RGB')
    if scene.get('crop'):
        box = scene['crop']
        if not (0 <= box[0] < box[2] <= shot.width and 0 <= box[1] < box[3] <= shot.height):
            raise ValueError('Crop exceeds real source frame')
        shot = shot.crop(box)
    shot = ImageOps.contain(shot, (1472, 690), Image.Resampling.LANCZOS)
    sx, sy = (WIDTH-shot.width)//2, 143 + (690-shot.height)//2
    bounds = (sx-2, sy-2, sx+shot.width+2, sy+shot.height+2)
    shadow = Image.new('RGBA', canvas.size)
    ImageDraw.Draw(shadow).rounded_rectangle((bounds[0]-8, bounds[1]+10, bounds[2]+8, bounds[3]+20), radius=20, fill=(0, 0, 0, 140))
    canvas = Image.alpha_composite(canvas.convert('RGBA'), shadow.filter(ImageFilter.GaussianBlur(19))).convert('RGB')
    draw = ImageDraw.Draw(canvas)
    draw.rounded_rectangle(bounds, radius=14, fill='#55516c')
    mask = Image.new('L', shot.size)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, shot.width-1, shot.height-1), radius=12, fill=255)
    canvas.paste(shot, (sx, sy), mask)

    # Dedicated caption rail: centred and completely outside captured app content.
    draw.rounded_rectangle((40, 855, 1560, 955), radius=22, fill='#1e2135', outline='#38314f', width=1)
    face = font(30, True)
    lines = wrapped(draw, scene['caption'], face, 1420)
    if len(lines) > 2:
        raise ValueError('Shorten caption to at most two lines')
    for i, line in enumerate(lines):
        draw.text((WIDTH//2, 905 + (i-(len(lines)-1)/2)*36), line, font=face, fill='#faf8ff', anchor='mm')
    footer = scene.get('footer', 'Recorded on macOS · Edited for time · Public / synthetic test data')
    draw.text((WIDTH//2, 976), footer, font=font(16), fill='#a2abc4', anchor='mm')
    draw.rectangle((0, HEIGHT-3, round(WIDTH*min(1, elapsed/duration)), HEIGHT), fill='#9c7bff')
    return canvas


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--capture-root', required=True, type=Path)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--output-dir', type=Path, default=Path('docs/assets'))
    parser.add_argument('--preview', action='store_true')
    args = parser.parse_args()
    root = args.capture_root.resolve()
    spec = json.loads((args.manifest or root/'edit.json').read_text())
    scenes = spec['scenes']
    total = sum(s['duration'] for s in scenes)
    if not scenes or not 0 < total <= 180:
        raise ValueError('Expected a bounded edit of 1–180 seconds')
    rows_file = root/'windows/manifest.jsonl'
    rows = [json.loads(s) for s in rows_file.read_text().splitlines()] if rows_file.exists() else []
    args.output_dir.mkdir(parents=True, exist_ok=True)
    previews = root/'edit'
    previews.mkdir(exist_ok=True)

    def source(scene, fraction):
        if 'file' in scene:
            return root/scene['file']
        wanted = scene['start'] + (scene['end']-scene['start'])*fraction
        candidates = [(r, f) for r in rows for f in r['frames'] if f['ok'] and f['label'] == scene['source']]
        row, frame = min(candidates, key=lambda item: abs(item[0]['time']-wanted))
        if abs(row['time']-wanted) > 2.5:
            raise ValueError(f'Missing real {scene["source"]} capture near {wanted}')
        return root/'windows'/frame['file']

    elapsed, edit = 0, []
    for i, scene in enumerate(scenes):
        still = compose(source(scene, .5), scene, elapsed, total)
        still.save(previews/f'scene-{i+1:02}.png')
        elapsed += scene['duration']
    if args.preview:
        print(f'Wrote {len(scenes)} actual-capture layout previews to {previews}')
        return
    video = args.output_dir/'magicvault-mcp-jit-demo.mp4'
    encoder = subprocess.Popen(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-f', 'rawvideo',
        '-pixel_format', 'rgb24', '-video_size', f'{WIDTH}x{HEIGHT}', '-framerate', str(FPS),
        '-i', '-', '-an', '-c:v', 'libx264', '-preset', 'medium', '-crf', '18', '-pix_fmt', 'yuv420p',
        '-movflags', '+faststart', str(video)], stdin=subprocess.PIPE)
    elapsed = 0
    try:
        for i, scene in enumerate(scenes):
            count, previous, frame = round(scene['duration']*FPS), None, None
            for j in range(count):
                path = source(scene, j/max(1, count-1))
                if path != previous:
                    frame = compose(path, scene, elapsed+j/FPS, total)
                    previous = path
                ImageDraw.Draw(frame).rectangle(
                    (0, HEIGHT-3, round(WIDTH*min(1, (elapsed+(j+1)/FPS)/total)), HEIGHT),
                    fill='#9c7bff')
                encoder.stdin.write(frame.tobytes())
            edit.append({**scene, 'output_start': elapsed})
            elapsed += scene['duration']
            print(f'{elapsed:.1f}s: {scene["caption"]}', flush=True)
    finally:
        encoder.stdin.close()
    if encoder.wait() != 0:
        raise RuntimeError('FFmpeg failed')
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', str(video),
        '-filter_complex', '[0:v]fps=8,scale=1280:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle',
        '-loop', '0', str(args.output_dir/'magicvault-mcp-jit-demo.gif')], check=True)
    poster = spec.get('poster_scene', 0)
    Image.open(previews/f'scene-{poster+1:02}.png').save(args.output_dir/'magicvault-mcp-jit-demo-poster.png')
    (previews/'render-manifest.json').write_text(json.dumps({'fps': FPS, 'duration': total, 'scenes': edit}, indent=2)+'\n')


if __name__ == '__main__':
    main()
