#!/usr/bin/env python3
"""Generate MiniMax narration, then fit each clip to its recorded scene.

Authentication belongs to mmx's existing configuration, never this script.
Generation is explicit, cached by request, and stops on the first failure.
Mixing is local and copies the original video stream without re-encoding.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


def run(argv):
    return subprocess.run(argv, check=True, capture_output=True, text=True).stdout


def duration(path):
    return float(run(['ffprobe', '-v', 'error', '-show_entries', 'format=duration',
                      '-of', 'default=noprint_wrappers=1:nokey=1', str(path)]))


def generate(args, plan):
    for segment in plan['segments']:
        if args.segments and segment['id'] not in args.segments:
            continue
        stem = args.work / segment['id']
        request = {key: plan[key] for key in ('model', 'voice', 'language', 'speed')}
        request['text'] = segment['text']
        stamp = stem.with_suffix('.request.json')
        audio = stem.with_suffix('.wav')
        if audio.exists() and stamp.exists() and json.loads(stamp.read_text()) == request:
            if duration(audio) > 0:
                print(f"{segment['id']}: using existing MiniMax clip", flush=True)
                continue
        if audio.exists():
            raise ValueError(f'Existing audio has different or missing request metadata: {audio}')
        text = stem.with_suffix('.txt')
        text.write_text(segment['text'] + '\n')
        stamp.write_text(json.dumps(request, indent=2) + '\n')
        command = [args.mmx, 'speech', 'synthesize', '--model', plan['model'],
                      '--voice', plan['voice'], '--speed', str(plan['speed']),
                      '--language', plan['language'], '--format', 'wav',
                      '--sample-rate', '32000', '--channels', '1', '--subtitles',
                      '--text-file', str(text), '--out', str(audio), '--output', 'json']
        result = subprocess.run(command, capture_output=True, text=True)
        stem.with_suffix('.result.json').write_text(result.stdout)
        stem.with_suffix('.stderr.txt').write_text(result.stderr)
        if result.returncode:
            raise RuntimeError(f"MiniMax exited {result.returncode}; inspect {stem}.result.json and .stderr.txt before retrying")
        print(f"{segment['id']}: generated {duration(audio):.2f}s", flush=True)


def mix(args, plan):
    source_hash = hashlib.sha256(args.video.read_bytes()).hexdigest()
    if source_hash != plan['source_video_sha256']:
        raise ValueError('Video changed since narration was timed; review the plan first')
    total = plan['duration_seconds']
    if abs(duration(args.video) - total) > .05:
        raise ValueError('Video duration does not match narration plan')
    ffmpeg = ['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y']
    inputs, filters, evidence = [], [], []
    previous_end = 0
    for i, segment in enumerate(plan['segments']):
        start, end = segment['start'], segment['end']
        if not (previous_end <= start < end <= total):
            raise ValueError('Narration windows overlap or exceed the video')
        previous_end = end
        source = args.work / (segment['id'] + '.wav')
        request = json.loads(source.with_suffix('.request.json').read_text())
        if request != {**{key: plan[key] for key in ('model', 'voice', 'language', 'speed')},
                       'text': segment['text']}:
            raise ValueError('Narration text or voice changed after generation')
        clean = args.work / (segment['id'] + '-trimmed.wav')
        # Trim only leading/trailing silence, retaining pauses within sentences.
        trim = 'silenceremove=start_periods=1:start_threshold=-50dB:start_silence=0.05'
        run(ffmpeg + ['-i', str(source), '-af', f'{trim},areverse,{trim},areverse',
                      '-c:a', 'pcm_s16le', '-ar', '48000', '-ac', '1', str(clean)])
        seconds = duration(clean)
        available = end - start - .35
        tempo = max(1., seconds / available)
        if tempo > 1.18:
            raise ValueError(f"Shorten segment {segment['id']}: {seconds:.2f}s into {available:.2f}s needs {tempo:.2f}x")
        inputs += ['-i', str(clean)]
        onset = start + .15
        filters.append(f'[{i}:a]atempo={tempo:.8f},adelay={round(onset*1000)}:all=1[a{i}]')
        evidence.append({**segment, 'raw_seconds': duration(source), 'trimmed_seconds': seconds,
                         'tempo': tempo, 'audio_start': onset, 'audio_end': onset + seconds / tempo,
                         'source_audio_sha256': hashlib.sha256(source.read_bytes()).hexdigest()})
    labels = ''.join(f'[a{i}]' for i in range(len(evidence)))
    filters.append(f'{labels}amix=inputs={len(evidence)}:normalize=0,apad,atrim=duration={total}[mix]')
    assembled = args.work / 'assembled.wav'
    run(ffmpeg + inputs + ['-filter_complex', ';'.join(filters), '-map', '[mix]',
                           '-ar', '48000', '-c:a', 'pcm_s16le', str(assembled)])
    measurement = subprocess.run(['ffmpeg', '-hide_banner', '-i', str(assembled), '-af',
                                 'loudnorm=I=-16:TP=-1.5:LRA=11:print_format=json',
                                 '-f', 'null', '-'], check=True, capture_output=True, text=True).stderr
    levels, _ = json.JSONDecoder().raw_decode(measurement[measurement.rfind('{'):])
    normalization = ('loudnorm=I=-16:TP=-1.5:LRA=11:linear=true:'
                     f"measured_I={levels['input_i']}:measured_TP={levels['input_tp']}:"
                     f"measured_LRA={levels['input_lra']}:measured_thresh={levels['input_thresh']}:"
                     f"offset={levels['target_offset']}")
    normalized = args.work / 'narration.wav'
    run(ffmpeg + ['-i', str(assembled), '-af', normalization, '-ar', '48000',
                  '-c:a', 'pcm_s16le', str(normalized)])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    run(ffmpeg + ['-i', str(args.video), '-i', str(normalized), '-map', '0:v:0', '-map', '1:a:0',
                  '-c:v', 'copy', '-c:a', 'aac', '-b:a', '192k', '-ar', '48000',
                  '-t', str(total), '-movflags', '+faststart',
                  '-metadata', 'comment=Synthetic narration generated with MiniMax; actual screen recording.',
                  str(args.output)])
    report = {**plan, 'segments': evidence, 'audio_processing': {
        'loudness_target_lufs': -16, 'true_peak_limit_dbtp': -1.5,
        'maximum_allowed_tempo': 1.18, 'video_stream_copied': True},
        'output_video_sha256': hashlib.sha256(args.output.read_bytes()).hexdigest()}
    args.output.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f'Wrote {args.output} ({duration(args.output):.2f}s)', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('generate', 'mix'))
    parser.add_argument('--work', type=Path, default=Path('output/jit-mcp-demo/narration'))
    parser.add_argument('--mmx', default=shutil.which('mmx') or 'mmx')
    parser.add_argument('--segments', nargs='+')
    parser.add_argument('--video', type=Path, default=Path('docs/assets/magicvault-mcp-jit-demo.mp4'))
    parser.add_argument('--output', type=Path, default=Path('docs/assets/magicvault-mcp-jit-demo-narrated.mp4'))
    args = parser.parse_args()
    plan = json.loads((args.work / 'plan.json').read_text())
    (generate if args.action == 'generate' else mix)(args, plan)


if __name__ == '__main__':
    main()
