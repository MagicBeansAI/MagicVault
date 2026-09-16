#!/usr/bin/env python3
"""Edit actual Codex/Chrome captures. Requires the locally recorded source files.

No tool output, app screen, browser result, or approval is synthesized. The edit
cuts waiting time, resizes real windows, and slows the recorded approval dialog.
"""
import argparse
import json
import pathlib
import subprocess

from PIL import Image, ImageDraw, ImageFont, ImageOps

parser = argparse.ArgumentParser()
parser.add_argument('--capture-root', type=pathlib.Path, required=True)
parser.add_argument('--output-dir', type=pathlib.Path, default=pathlib.Path('docs/assets'))
args = parser.parse_args()
root = args.capture_root.resolve()
out = args.output_dir.resolve()
out.mkdir(parents=True, exist_ok=True)
work = root / 'edit'
work.mkdir(exist_ok=True)
native = work / 'approval'
native.mkdir(exist_ok=True)
fps = 12
width, height = 1600, 1000
rows = [json.loads(s) for s in (root/'take-2/manifest.jsonl').read_text().splitlines()]
font_path = '/System/Library/Fonts/Supplemental/Arial.ttf'
bold_path = '/System/Library/Fonts/Supplemental/Arial Bold.ttf'
fonts = {size: ImageFont.truetype(font_path, size) for size in (19, 21, 26)}
bold = ImageFont.truetype(bold_path, 30)

subprocess.run([
    'ffmpeg', '-hide_banner', '-loglevel', 'error', '-ss', '113.0',
    '-i', str(root/'native-take-2/recording.mp4'), '-t', '1.75',
    '-vf', 'fps=12,crop=900:790:1230:270', '-y', str(native/'%04d.png'),
], check=True)
approval = sorted(native.glob('*.png'))[:21]

# Times refer to take-2/manifest.jsonl, except the native approval segment above.
# The final two cuts change camera between the same completed browser/Codex state.
scenes = [
    ('browser', 0, 2, 2.5, 'A real website. A real Codex session.'),
    ('codex', 104, 109, 5, 'Codex discovers references through MagicVault MCP.'),
    ('codex', 113, 114, 4, 'The fill request contains references and selectors.'),
    ('approval', 0, 1, 6, 'You approve the exact website and fields.'),
    ('browser', 116, 119, 3, 'MagicVault delivers the stored credentials to Chrome.'),
    ('codex', 120, 123, 4, 'Codex receives a filled receipt.'),
    ('browser', 131, 136, 5, 'Codex submits the login form.'),
    ('browser', 240, 244, 5, 'The website confirms a successful login.'),
    ('codex', 160, 163, 3, 'Codex verifies the result.'),
]

def source(label, t):
    row = min(rows, key=lambda r: abs(r['elapsed'] - t))
    frame = next(f for f in row['frames'] if f['label'] == label and f['ok'])
    return root/'take-2'/frame['file']

def compose(path, caption, approval_scene=False):
    im = Image.new('RGB', (width, height), '#0d1118')
    draw = ImageDraw.Draw(im)
    draw.text((32, 21), 'MagicVault', font=bold, fill='#f1f5fb')
    draw.text((228, 29), '+ Codex', font=fonts[26], fill='#b5c4db')
    draw.text((width-305, 31), 'MCP  /  REAL SESSION', font=fonts[21], fill='#a6e3cf')
    draw.line((32, 73, width-32, 73), fill='#293344', width=1)
    shot = Image.open(path).convert('RGB')
    bounds = (width-64, 790)
    shot = ImageOps.contain(shot, bounds, Image.Resampling.LANCZOS)
    im.paste(shot, ((width-shot.width)//2, 90+(790-shot.height)//2))
    draw.text((32, 902), caption, font=fonts[26], fill='#e5edf9')
    foot = 'Edited live capture · Public practice account · Setup completed before recording'
    if approval_scene:
        foot += ' · Approval slowed for readability'
    draw.text((32, 954), foot, font=fonts[19], fill='#96a6bd')
    return im

video = out/'magicvault-mcp-demo.mp4'
encoder = subprocess.Popen([
    'ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-f', 'rawvideo',
    '-pixel_format', 'rgb24', '-video_size', f'{width}x{height}', '-framerate', str(fps),
    '-i', '-', '-an', '-c:v', 'libx264', '-preset', 'medium', '-crf', '18',
    '-pix_fmt', 'yuv420p', '-movflags', '+faststart', str(video),
], stdin=subprocess.PIPE)
edits = []
elapsed = 0
try:
    for label, start, end, duration, caption in scenes:
        last_path, frame = None, None
        count = round(duration*fps)
        for i in range(count):
            fraction = i/max(1, count-1)
            path = approval[min(len(approval)-1, int(fraction*len(approval)))] if label == 'approval' else source(label, start+(end-start)*fraction)
            if path != last_path:
                frame = compose(path, caption, label == 'approval')
                last_path = path
            encoder.stdin.write(frame.tobytes())
        frame.save(work/f'{len(edits)+1:02d}-{label}.jpg', quality=93)
        if caption == 'The website confirms a successful login.':
            frame.save(out/'magicvault-mcp-demo-poster.png')
        edits.append({'source': label, 'source_start': start, 'source_end': end,
                      'output_start': elapsed, 'duration': duration, 'caption': caption})
        elapsed += duration
        print(f'{elapsed:4.1f}s: {caption}', flush=True)
finally:
    encoder.stdin.close()
if encoder.wait() != 0:
    raise SystemExit('Video encoding failed')

gif = out/'magicvault-mcp-demo.gif'
subprocess.run([
    'ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', str(video),
    '-filter_complex', '[0:v]fps=8,scale=1280:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle',
    '-loop', '0', str(gif),
], check=True)
(work/'edit-manifest.json').write_text(json.dumps({'fps': fps, 'duration': elapsed,
    'native_video_crop': [1230, 270, 900, 790],
    'native_video_range_seconds': [113.0, 114.75], 'scenes': edits}, indent=2)+'\n')
print(f'Saved {video} and {gif}', flush=True)
