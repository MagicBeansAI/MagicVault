#!/usr/bin/env python3
"""Render an explicitly illustrated workflow, never simulated recording evidence.

Requires Pillow. Optional --font-dir contains Arial.ttf, Arial Bold.ttf and
Andale Mono.ttf; defaults to macOS supplemental fonts. No vault/browser access.
"""
import argparse
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

W, H = 1280, 800
BG, CARD, EDGE = '#10151f', '#192231', '#2c3a4e'
WHITE, MUTED, MINT, PURPLE = '#f2f5fa', '#a4b2c6', '#8cf2cd', '#b7a5ff'
STEPS = ['The idea', 'Set up', 'Discover', 'Request', 'Approve', 'Delivered', 'More uses', 'Build with it']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('docs/assets'))
    parser.add_argument('--font-dir', type=Path, default=Path('/System/Library/Fonts/Supplemental'))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    fonts = {}

    def font(size, weight='regular'):
        key = (size, weight)
        if key not in fonts:
            file = {'regular': 'Arial.ttf', 'bold': 'Arial Bold.ttf', 'mono': 'Andale Mono.ttf'}[weight]
            fonts[key] = ImageFont.truetype(str(args.font_dir / file), size)
        return fonts[key]

    def frame(scene, progress=1):
        im = Image.new('RGB', (W, H), BG)
        d = ImageDraw.Draw(im)

        def txt(x, y, message, size=22, color=WHITE, weight='regular'):
            d.text((x, y), message, font=font(size, weight), fill=color)

        def box(x, y, w, h, color=CARD, outline=EDGE, r=18):
            d.rounded_rectangle((x, y, x+w, y+h), radius=r, fill=color, outline=outline, width=2)

        def pill(x, y, label, color=MINT, width=None):
            width = width or int(d.textlength(label, font=font(16, 'bold'))) + 30
            box(x, y, width, 32, color='#223344', outline='#334b5d', r=16)
            txt(x+15, y+6, label, 16, color, 'bold')

        def code(x, y, lines, size=22, reveal=True):
            count = len(lines) if not reveal else max(1, int(len(lines)*progress + 0.8))
            for index, (line, color) in enumerate(lines[:count]):
                txt(x, y+index*36, line, size, color, 'mono')

        def title(kicker, headline, sub):
            txt(52, 108, kicker.upper(), 16, MINT, 'bold')
            txt(52, 143, headline, 43, WHITE, 'bold')
            txt(52, 204, sub, 22, MUTED)

        def browser(filled=False):
            box(728, 273, 500, 380)
            txt(756, 295, 'BROWSER  /  demo.example', 17, MUTED, 'bold')
            d.line((750, 332, 1206, 332), fill=EDGE, width=2)
            txt(762, 366, 'Welcome back', 29, WHITE, 'bold')
            txt(762, 422, 'Password', 19, MUTED)
            box(760, 454, 430, 62, color=BG, outline=MINT if filled else EDGE, r=9)
            if filled:
                txt(781, 463, '••••••••••••', 34, WHITE, 'bold')
            box(760, 544, 430, 53, color='#283b4d', outline='#283b4d', r=9)
            txt(932, 556, 'Sign in', 23, MUTED, 'bold')
            txt(762, 621, 'Your browser tool owns navigation and submission.', 15, MUTED)

        # Persistent labeling prevents this illustrated UI being mistaken for a recording.
        box(52, 29, 38, 38, color='#283442', outline=MINT, r=10)
        d.ellipse((64, 40, 78, 54), outline=MINT, width=3)
        txt(104, 33, 'MagicVault', 28, WHITE, 'bold')
        txt(795, 42, 'ILLUSTRATED WORKFLOW  ·  SYNTHETIC EXAMPLE', 15, MUTED, 'bold')
        d.line((52, 85, 1228, 85), fill=EDGE, width=2)

        if scene == 0:
            title('MagicVault + Codex', 'Let agents use credentials without seeing them.',
                  'Your agent asks for a use. MagicVault delivers to the approved destination.')
            for x, name, detail, color in [(52, 'Codex', 'Requests a reference', PURPLE),
                                          (464, 'MagicVault', 'Holds the credential', MINT),
                                          (876, 'Your app', 'Receives the delivery', WHITE)]:
                box(x, 302, 352, 233)
                txt(x+28, 348, name, 35, color, 'bold')
                txt(x+28, 411, detail, 23, MUTED)
                if name == 'MagicVault':
                    pill(x+28, 465, 'Human-controlled consent')
            for x in [426, 838]:
                txt(x-6, 382, '›', 45, MINT, 'bold')
            txt(52, 591, 'References in tool calls. Status receipts in replies.', 30, WHITE, 'bold')
            txt(52, 642, 'No password pasted into the conversation.', 25, MUTED)
        elif scene == 1:
            title('01 / human setup', 'Enroll once, outside the conversation.',
                  'Install the candidate, run setup, then enter a synthetic password in the native prompt.')
            box(52, 273, 1176, 310)
            txt(80, 299, 'TERMINAL  /  AFTER INSTALLING BOTH LOCAL TARBALLS', 16, MUTED, 'bold')
            code(80, 352, [('$ magicvault --profile agent setup', MINT),
                           ('$ magicvault --profile agent doctor', WHITE),
                           ('$ magicvault --profile agent enroll \\', WHITE),
                           ("    --label 'Demo account' --field password", WHITE)], 24)
            pill(52, 617, 'macOS Apple Silicon')
            pill(278, 617, 'Node 22+')
            pill(404, 617, 'No Rust toolchain for prebuilt packages')
        elif scene == 2:
            title('02 / discover', 'Codex finds the account by reference.',
                  'Connect the MCP server, authorize the test origin, then ask for a secure fill.')
            box(52, 273, 630, 380)
            txt(80, 299, 'CODEX  /  MCP WORKFLOW', 16, PURPLE, 'bold')
            code(80, 351, [('list_credentials()', MINT),
                           ('Demo account', WHITE),
                           ('  credential_ref: cred_…', PURPLE),
                           ('  field_names: [password]', MUTED),
                           ('', WHITE),
                           ('list_browsers() → browser_targets()', MINT)], 22)
            browser()
        elif scene == 3:
            title('03 / request', 'Use a reference in the tool call.',
                  'The browser and document handles bind the request to the intended destination.')
            box(52, 273, 630, 380)
            txt(80, 299, 'CODEX  /  secure_fill', 16, PURPLE, 'bold')
            code(80, 347, [('operation_id: <fresh UUID>', MUTED),
                           ('browser_handle: <browser UUID>', MUTED),
                           ('target_handle: <document UUID>', MUTED),
                           ('fields: [{', WHITE),
                           ('  css: "#password",', WHITE),
                           ('  credential_ref: "cred_…",', MINT),
                           ('  credential_field: "password"', WHITE),
                           ('}]', WHITE)], 22)
            browser()
        elif scene == 4:
            title('04 / consent', 'You decide where the credential goes.',
                  'Per-use approval is the default. Only a human can remember an exact use.')
            box(52, 273, 482, 380)
            txt(80, 301, 'CODEX  /  WAITING', 16, PURPLE, 'bold')
            code(80, 365, [('secure_fill → pending', MINT), ('', WHITE),
                           ('fill_status(operation_id)', WHITE), ('→ pending', MUTED)], 23)
            box(572, 273, 656, 380, color='#233044', outline='#607694')
            txt(604, 300, 'NATIVE APPROVAL  /  SIMPLIFIED ILLUSTRATION', 15, MUTED, 'bold')
            txt(604, 351, 'Allow this password fill?', 29, WHITE, 'bold')
            txt(604, 413, 'Demo account  →  demo.example', 23, WHITE)
            txt(604, 453, 'Field: password     Selector: #password', 21, MUTED)
            box(604, 534, 172, 54, color=CARD)
            txt(654, 548, 'Deny', 22, WHITE, 'bold')
            box(794, 534, 184, 54, color=MINT if progress > .6 else '#3a655e', outline=MINT)
            txt(826, 548, 'Allow once', 22, BG, 'bold')
            txt(999, 553, 'Always allow', 19, MUTED)
        elif scene == 5:
            title('05 / receipt', 'The field is filled. Codex receives a status.',
                  'MagicVault returns a delivery receipt; the browser tool handles the next step.')
            box(52, 273, 630, 380)
            txt(80, 299, 'CODEX  /  fill_status', 16, PURPLE, 'bold')
            code(80, 359, [('state: "filled"', MINT), ('fields: ["filled"]', WHITE),
                           ('error: null', MUTED)], 25)
            pill(80, 541, 'No credential value in the tool reply')
            browser(filled=progress > .3)
        elif scene == 6:
            title('Beyond browser fields', 'One credential boundary. Three kinds of use.',
                  'Human-registered HTTP and process profiles fix the destination before invocation.')
            items = [('Browser', 'secure_fill', ['An authorized field', 'in a connected Chromium tab.']),
                     ('HTTP', 'secure_new_http', ['A fixed API request.', 'Response content is withheld.']),
                     ('Process', 'secure_new_process', ['A fixed new command.', 'stdout / stderr are withheld.'])]
            for i, (label, tool, lines) in enumerate(items):
                x = 52+i*400
                box(x, 285, 376, 344)
                txt(x+26, 317, label, 32, WHITE, 'bold')
                txt(x+26, 382, tool, 20, MINT, 'mono')
                for n, line in enumerate(lines): txt(x+26, 442+n*35, line, 22, MUTED)
                pill(x+26, 550, 'Receipt only')
            txt(52, 660, 'A completed receipt does not prove login or application success.', 21, MUTED)
        else:
            title('Build on MagicVault', 'Use it from your own Node or TypeScript project.',
                  'The local npm candidate now includes a typed client with CommonJS and ESM exports.')
            box(52, 273, 1176, 337)
            txt(80, 299, 'TYPESCRIPT  /  AFTER HUMAN SETUP AND PROFILE REGISTRATION', 16, MUTED, 'bold')
            code(80, 348, [('const vault = new MagicVault({ profile: "agent" });', WHITE),
                           ('const operation_id = createOperationId();', WHITE),
                           ('await vault.secureNewHttp({ profile_id, operation_id });', MINT),
                           ('const receipt = await vault.waitForDelivery(operation_id);', WHITE),
                           ('console.log(receipt.state);', PURPLE)], 24)
            txt(52, 639, 'github.com/MagicBeansAI/MagicVault', 29, MINT, 'bold')
            txt(52, 681, 'Local candidates today. Public npm release and signing are still pending.', 20, MUTED)

        # Boundary text and section navigation remain readable on every scene.
        txt(52, 739, 'Approved recipients and separate browser tools can still observe credentials.', 16, MUTED)
        for i, name in enumerate(STEPS):
            x = 52+i*149
            d.rounded_rectangle((x, 776, x+130, 781), radius=2, fill=MINT if i <= scene else EDGE)
        return im

    frames, durations, posters = [], [], []
    for scene in range(8):
        for step in range(12):
            rendered = frame(scene, (step+1)/12)
            if step == 11:
                posters.append(rendered.copy())
            frames.append(rendered.quantize(colors=128, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
            durations.append(4300 if step == 11 else 80)
    frames[0].save(args.output/'magicvault-demo.gif', save_all=True, append_images=frames[1:],
                   duration=durations, loop=0, optimize=True, disposal=1)
    posters[0].save(args.output/'magicvault-demo-poster.png')
    contact = Image.new('RGB', (1280, 400), BG)
    for i, poster in enumerate(posters):
        contact.paste(poster.resize((320, 200), Image.Resampling.LANCZOS), ((i%4)*320, (i//4)*200))
    qa = Path('output')
    qa.mkdir(exist_ok=True)
    contact.save(qa/'magicvault-demo-contact.png')
    print(f'{args.output}/magicvault-demo.gif: {sum(durations)/1000:.1f}s, {W}x{H}, illustrated workflow')


if __name__ == '__main__':
    main()
