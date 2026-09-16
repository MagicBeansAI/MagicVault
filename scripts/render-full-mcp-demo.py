#!/usr/bin/env python3
"""Edit the actual full MCP setup/login recording and its transcript audit.

Requires the local source captures. Only framing, crops, captions, cuts, and
timing changes are generated; every application screen is a real capture.
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
fps, width, height = 12, 1600, 1000
rows = [json.loads(s) for s in (root/'windows/manifest.jsonl').read_text().splitlines()]
font_path = '/System/Library/Fonts/Supplemental/Arial.ttf'
bold_path = '/System/Library/Fonts/Supplemental/Arial Bold.ttf'
fonts = {size: ImageFont.truetype(font_path, size) for size in (19, 21, 26)}
bold = ImageFont.truetype(bold_path, 30)

# The first video was recovered from its original H.264 samples after the
# recorder exited without finalizing its MP4. Its times below use 30 fps.
native_specs = {
    'pair': ('native/recovered.mp4', 14, 18, (1260, 396, 840, 360)),
    'username': ('native/recovered.mp4', 30, 33.2, (1260, 390, 840, 388)),
    'password': ('native/recovered.mp4', 34, 38.2, (1260, 390, 840, 388)),
    'site': ('native/recovered.mp4', 50, 52, (1260, 348, 840, 552)),
    'connect': ('native/recovered.mp4', 67, 68, (1260, 388, 840, 392)),
    'approve': ('native-login/recording.mp4', 26, 29.2, (1260, 294, 840, 776)),
}
native = {}
for label, (video, start, end, crop) in native_specs.items():
    folder = work / ('native-' + label)
    folder.mkdir(exist_ok=True)
    x, y, w, h = crop
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-ss', str(start),
                    '-i', str(root/video), '-t', str(end-start), '-vf',
                    f'fps={fps},crop={w}:{h}:{x}:{y}', '-y', str(folder/'%04d.png')], check=True)
    native[label] = sorted(folder.glob('*.png'))[:round((end-start)*fps)]
    if not native[label]:
        raise SystemExit(f'No native frames for {label}')

# Window source times use windows/manifest.jsonl. Native entries use the
# intervals above. Still entries are genuine window screenshots.
scenes = [
    ('codex', 1, 7, 7, '1  Enable MagicVault MCP in Codex.', 'setup'),
    ('codex', 10, 11, 3, '2  Pair the local Codex client.', 'setup'),
    ('pair', 0, 1, 4, 'You approve pairing in the native dialog.', 'setup'),
    ('codex', 21, 23, 3, '3  Enroll the website account. No values in the command.', 'setup'),
    ('username', 0, 1, 5, 'Enter the username in MagicVault’s hidden-input dialog.', 'setup'),
    ('password', 0, 1, 7, 'Enter and save the password in the native dialog.', 'setup'),
    ('codex', 39, 40, 3, 'Enrollment returns a credential reference and field names.', 'setup'),
    ('codex', 43, 44, 3, '4  Permit this credential on the exact website origin.', 'setup'),
    ('site', 0, 1, 5, 'You review the website and fields before allowing use.', 'setup'),
    ('codex', 69, 70, 3, '5  Connect the dedicated demo browser.', 'setup'),
    ('connect', 0, 1, 4, 'You approve attaching MagicVault to this browser.', 'setup'),
    ('codex', 76, 78, 3, 'Setup is ready: the CLI lists references, not values.', 'setup'),
    ('codex', 87, 90, 5, '6  Ask the real Codex session to log in with the saved account.', 'login'),
    ('codex', 280, 284, 4, 'Allow the MagicVault MCP tools in Codex.', 'login'),
    ('browser', 320, 321, 3, 'The real practice website starts with empty fields.', 'login'),
    ('codex', 326, 328, 6, 'Codex sends references and selectors. MagicVault returns pending.', 'login'),
    ('approve', 0, 1, 8, 'You approve one fill for this website and these fields.', 'login'),
    ('browser', 330, 333, 3, 'MagicVault delivers the values directly to the browser.', 'login'),
    ('codex', 332, 334, 4, 'Codex receives the actual filled receipt.', 'login'),
    ('browser', 344, 349, 5, 'Codex submits the form; the website accepts the login.', 'login'),
    ('still:browser-success.jpg', 0, 0, 5, 'The real website confirms: Logged In Successfully.', 'login'),
    ('still:audit-credentials.jpg', 0, 0, 5, '7  Inspect what the login agent actually received.', 'audit'),
    ('still:audit-counts.jpg', 0, 0, 9, 'Zero credential-value matches in the recorded Codex session.', 'audit'),
]


def source(label, t):
    if label.startswith('still:'):
        return work / label.split(':', 1)[1]
    available = [(row, frame) for row in rows for frame in row['frames']
                 if frame['label'] == label and frame['ok']]
    row, frame = min(available, key=lambda item: abs(item[0]['elapsed'] - t))
    if abs(row['elapsed'] - t) > 2:
        raise ValueError(f'Missing {label} capture at {t}')
    return root/'windows'/frame['file']


def compose(path, caption, chapter, is_native=False):
    canvas = Image.new('RGB', (width, height), '#0d1118')
    draw = ImageDraw.Draw(canvas)
    draw.text((32, 21), 'MagicVault', font=bold, fill='#f1f5fb')
    draw.text((228, 29), '+ Codex', font=fonts[26], fill='#b5c4db')
    draw.text((width-340, 31), 'MCP  /  ' + chapter.upper() + '  /  LIVE', font=fonts[21], fill='#a6e3cf')
    draw.line((32, 73, width-32, 73), fill='#293344', width=1)
    shot = Image.open(path).convert('RGB')
    if is_native:
        # Remove the unrelated background in the native window's round corners.
        mask = Image.new('L', shot.size, 0)
        ImageDraw.Draw(mask).rounded_rectangle((0, 0, shot.width-1, shot.height-1), radius=40, fill=255)
        clean = Image.new('RGB', shot.size, '#0d1118')
        clean.paste(shot, (0, 0), mask)
        shot = clean
    shot = ImageOps.contain(shot, (width-64, 790), Image.Resampling.LANCZOS)
    canvas.paste(shot, ((width-shot.width)//2, 90+(790-shot.height)//2))
    draw.text((32, 902), caption, font=fonts[26], fill='#e5edf9')
    if chapter == 'audit':
        footer = 'Actual transcript audit · Literal public-account values checked · Browser access is a separate boundary'
    elif is_native:
        footer = 'Edited live capture · Public practice account · Native dialog slowed for readability'
    else:
        footer = 'Edited live capture · Public practice account · Starts with MagicVault installed and its service running'
    draw.text((32, 954), footer, font=fonts[19], fill='#96a6bd')
    return canvas


video = out/'magicvault-mcp-full-demo.mp4'
encoder = subprocess.Popen([
    'ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-f', 'rawvideo',
    '-pixel_format', 'rgb24', '-video_size', f'{width}x{height}', '-framerate', str(fps),
    '-i', '-', '-an', '-c:v', 'libx264', '-preset', 'medium', '-crf', '18',
    '-pix_fmt', 'yuv420p', '-movflags', '+faststart', str(video),
], stdin=subprocess.PIPE)
edits, elapsed = [], 0
try:
    for label, start, end, duration, caption, chapter in scenes:
        count, last_path, frame = round(duration*fps), None, None
        for i in range(count):
            fraction = i/max(1, count-1)
            path = native[label][min(len(native[label])-1, int(fraction*len(native[label])))] if label in native else source(label, start+(end-start)*fraction)
            if path != last_path:
                frame = compose(path, caption, chapter, label in native)
                last_path = path
            encoder.stdin.write(frame.tobytes())
        frame.save(work/f'scene-{len(edits)+1:02d}.jpg', quality=93)
        if label == 'still:browser-success.jpg':
            frame.save(out/'magicvault-mcp-full-demo-poster.png')
        edits.append({'source': label, 'source_start': start, 'source_end': end,
                      'output_start': elapsed, 'duration': duration, 'caption': caption, 'chapter': chapter})
        elapsed += duration
        print(f'{elapsed:4.1f}s: {caption}', flush=True)
finally:
    encoder.stdin.close()
if encoder.wait() != 0:
    raise SystemExit('Video encoding failed')
gif = out/'magicvault-mcp-full-demo.gif'
subprocess.run([
    'ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', str(video),
    '-filter_complex', '[0:v]fps=8,scale=1280:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle',
    '-loop', '0', str(gif),
], check=True)
(work/'full-edit-manifest.json').write_text(json.dumps({'fps': fps, 'duration': elapsed,
    'native_sources': native_specs, 'scenes': edits}, indent=2)+'\n')
print(f'Saved {video} and {gif}', flush=True)
