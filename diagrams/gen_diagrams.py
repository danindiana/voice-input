#!/usr/bin/env python3
"""
Generate all voice-input architecture diagrams as PNG + SVG.
Dark background, neon color schemes.
"""
import subprocess, textwrap, os
from pathlib import Path

OUT = Path(__file__).parent

# ── Palette definitions ────────────────────────────────────────────────────
BG    = "#0d0d0d"
BG2   = "#111827"
BG3   = "#0a0a1a"
BG4   = "#0f0f0f"
BG5   = "#080818"

CYAN    = "#00ffff"
GREEN   = "#39ff14"
MAGENTA = "#ff00ff"
YELLOW  = "#ffff00"
ORANGE  = "#ff6600"
PINK    = "#ff1493"
BLUE    = "#00aaff"
LIME    = "#ccff00"
VIOLET  = "#9900ff"
TEAL    = "#00ffcc"
RED     = "#ff2244"
WHITE   = "#e0e0e0"
DIM     = "#555555"
EDGE    = "#333333"

def render(name: str, dot_src: str) -> None:
    dot_path = OUT / f"{name}.dot"
    dot_path.write_text(dot_src)
    for fmt in ("png", "svg"):
        out_path = OUT / f"{name}.{fmt}"
        extra = ["-Gdpi=150"] if fmt == "png" else []
        subprocess.run(
            ["dot", f"-T{fmt}"] + extra + [str(dot_path), "-o", str(out_path)],
            check=True
        )
    print(f"  ✓ {name}")


# ══════════════════════════════════════════════════════════════════════════════
# 1. OVERALL SYSTEM
# ══════════════════════════════════════════════════════════════════════════════

render("sys_01_block", f"""
digraph {{
    bgcolor="{BG}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=11 style=filled shape=box penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5 color="{DIM}"]
    rankdir=LR

    UC03   [label="UC03 USB\nHeadset" fillcolor="#1a1a2e" fontcolor="{CYAN}" color="{CYAN}"]
    script [label="voice-input.sh" fillcolor="#1a1a2e" fontcolor="{GREEN}" color="{GREEN}"]
    py     [label="transcribe.py" fillcolor="#1a1a2e" fontcolor="{MAGENTA}" color="{MAGENTA}"]
    gpu    [label="NVIDIA GPU\n(faster-whisper)" fillcolor="#1a1a2e" fontcolor="{YELLOW}" color="{YELLOW}"]
    out    [label="Output\ntype / clip / print" fillcolor="#1a1a2e" fontcolor="{ORANGE}" color="{ORANGE}"]

    UC03   -> script [label="32kHz PCM" color="{CYAN}"]
    script -> py     [label="WAV file" color="{GREEN}"]
    py     -> gpu    [label="float16 inference" color="{MAGENTA}"]
    gpu    -> py     [label="segments + words" color="{YELLOW}"]
    py     -> script [label="transcript" color="{MAGENTA}"]
    script -> out    [label="text" color="{ORANGE}"]
}}
""")

render("sys_02_dataflow", f"""
digraph {{
    bgcolor="{BG2}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]
    rankdir=TB

    subgraph cluster_capture {{
        bgcolor="#0a0a0a" color="{CYAN}" label="Audio Capture" fontcolor="{CYAN}" penwidth=2
        mic  [label="Mic Input\n32kHz mono" shape=parallelogram fillcolor="#0d1b2a" color="{CYAN}" fontcolor="{CYAN}"]
        raw  [label="/tmp/voice-*.raw\nraw s16le PCM" shape=cylinder fillcolor="#0d1b2a" color="{CYAN}" fontcolor="{TEAL}"]
        wav  [label="/tmp/voice-*.wav\nstandard WAV" shape=cylinder fillcolor="#0d1b2a" color="{CYAN}" fontcolor="{TEAL}"]
    }}

    subgraph cluster_infer {{
        bgcolor="#0a0a0a" color="{MAGENTA}" label="Inference" fontcolor="{MAGENTA}" penwidth=2
        model [label="faster-whisper\nmedium float16" shape=box fillcolor="#1a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
        segs  [label="Segments +\nWord timestamps" shape=note fillcolor="#1a001a" color="{MAGENTA}" fontcolor="{PINK}"]
    }}

    subgraph cluster_out {{
        bgcolor="#0a0a0a" color="{GREEN}" label="Output" fontcolor="{GREEN}" penwidth=2
        text  [label="Plain text" shape=box fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
        anim  [label="Animated\nterminal render" shape=box fillcolor="#001a00" color="{GREEN}" fontcolor="{LIME}"]
    }}

    mic -> raw  [label="parec" color="{CYAN}"]
    raw -> wav  [label="sox" color="{CYAN}"]
    wav -> model [label="transcribe.py" color="{MAGENTA}"]
    model -> segs [color="{MAGENTA}"]
    segs -> text [label="--no-fancy" color="{GREEN}"]
    segs -> anim [label="--fancy" color="{LIME}"]
}}
""")

render("sys_03_layers", f"""
digraph {{
    bgcolor="{BG3}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5 color="{DIM}" style=dashed]
    rankdir=BT

    hw  [label="Hardware Layer\nUC03 USB · NVIDIA RTX 3060/3080" fillcolor="#0a0015" color="{VIOLET}" fontcolor="{VIOLET}"]
    os  [label="OS / Kernel Layer\nALSA · PipeWire · CUDA Driver" fillcolor="#00000f" color="{BLUE}" fontcolor="{BLUE}"]
    rt  [label="Runtime Layer\nBash · Python 3.13 · LD_LIBRARY_PATH" fillcolor="#001020" color="{CYAN}" fontcolor="{CYAN}"]
    ml  [label="ML Layer\nfaster-whisper · ctranslate2 · Whisper medium" fillcolor="#0a0020" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    app [label="Application Layer\nvoice-input.sh · transcribe.py" fillcolor="#001500" color="{GREEN}" fontcolor="{GREEN}"]
    ux  [label="UX Layer\nTerminal · xdotool · xclip · Beep feedback" fillcolor="#150000" color="{ORANGE}" fontcolor="{ORANGE}"]

    hw -> os  [color="{VIOLET}"]
    os -> rt  [color="{BLUE}"]
    rt -> ml  [color="{CYAN}"]
    ml -> app [color="{MAGENTA}"]
    app -> ux [color="{GREEN}"]
}}
""")

render("sys_04_user_interaction", f"""
digraph {{
    bgcolor="{BG4}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    start  [label="Run\nvoice-input" shape=oval fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    beep1  [label="Low beep\n(recording starts)" shape=box fillcolor="#0d0d0d" color="{CYAN}" fontcolor="{CYAN}"]
    speak  [label="User speaks" shape=parallelogram fillcolor="#0d0d0d" color="{YELLOW}" fontcolor="{YELLOW}"]
    enter  [label="Press Enter\nor 65s timeout" shape=diamond fillcolor="#0d0d0d" color="{ORANGE}" fontcolor="{ORANGE}"]
    beep2  [label="High beep\n(stopped)" shape=box fillcolor="#0d0d0d" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    wait   [label="Transcribing...\n(GPU inference)" shape=box fillcolor="#0d0d0d" color="{MAGENTA}" fontcolor="{PINK}"]
    result [label="Text output\nprint / clip / type" shape=oval fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]

    start -> beep1 [color="{GREEN}"]
    beep1 -> speak [color="{CYAN}"]
    speak -> enter [color="{YELLOW}"]
    enter -> beep2 [color="{ORANGE}"]
    beep2 -> wait  [color="{MAGENTA}"]
    wait  -> result [color="{LIME}"]
}}
""")

render("sys_05_component_dep", f"""
digraph {{
    bgcolor="{BG5}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled shape=component penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    vi  [label="voice-input.sh" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    tr  [label="transcribe.py" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    fw  [label="faster-whisper" fillcolor="#1a0a00" color="{ORANGE}" fontcolor="{ORANGE}"]
    ct  [label="ctranslate2" fillcolor="#001a1a" color="{CYAN}" fontcolor="{CYAN}"]
    par [label="parec" fillcolor="#0a0a0a" color="{BLUE}" fontcolor="{BLUE}"]
    sox [label="sox" fillcolor="#0a0a0a" color="{BLUE}" fontcolor="{BLUE}"]
    xdo [label="xdotool" fillcolor="#0a0a0a" color="{YELLOW}" fontcolor="{YELLOW}"]
    xcl [label="xclip" fillcolor="#0a0a0a" color="{YELLOW}" fontcolor="{YELLOW}"]
    pap [label="paplay" fillcolor="#0a0a0a" color="{TEAL}" fontcolor="{TEAL}"]
    cu  [label="libcublas.so.12\n(/usr/lib/ollama)" fillcolor="#1a0000" color="{RED}" fontcolor="{RED}"]

    vi -> par [color="{BLUE}" label="capture"]
    vi -> sox [color="{BLUE}" label="convert"]
    vi -> tr  [color="{MAGENTA}" label="invoke"]
    vi -> xdo [color="{YELLOW}" label="type mode"]
    vi -> xcl [color="{YELLOW}" label="clip mode"]
    vi -> pap [color="{TEAL}" label="beeps"]
    tr -> fw  [color="{ORANGE}" label="import"]
    fw -> ct  [color="{CYAN}" label="backend"]
    ct -> cu  [color="{RED}" label="CUDA"]
}}
""")


# ══════════════════════════════════════════════════════════════════════════════
# 2. AUDIO CAPTURE
# ══════════════════════════════════════════════════════════════════════════════

render("audio_01_device_chain", f"""
digraph {{
    bgcolor="{BG}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]
    rankdir=LR

    mic  [label="UC03 USB Mic\nvendor e4b7:0812" shape=box fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    alsa [label="ALSA\ncard 3" shape=box fillcolor="#001520" color="{BLUE}" fontcolor="{BLUE}"]
    pw   [label="PipeWire\naudio server" shape=box fillcolor="#001520" color="{TEAL}" fontcolor="{TEAL}"]
    pa   [label="PulseAudio\ncompat layer" shape=box fillcolor="#001520" color="{VIOLET}" fontcolor="{VIOLET}"]
    src  [label="PW source\nalsa_input.usb-UC03_UC03-00\n.mono-fallback" shape=box fillcolor="#001520" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    parec [label="parec\n--format=s16le\n--rate=32000\n--channels=1" shape=box fillcolor="#001520" color="{GREEN}" fontcolor="{GREEN}"]

    mic -> alsa  [label="USB audio" color="{CYAN}"]
    alsa -> pw   [label="kernel driver" color="{BLUE}"]
    pw   -> pa   [label="compat bridge" color="{TEAL}"]
    pa   -> src  [color="{VIOLET}"]
    src  -> parec [label="PCM stream" color="{GREEN}"]
}}
""")

render("audio_02_pipewire_routing", f"""
digraph {{
    bgcolor="{BG2}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    subgraph cluster_hw {{
        bgcolor="#050515" color="{BLUE}" label="Hardware" fontcolor="{BLUE}" penwidth=2
        uc03in  [label="UC03 In\n32kHz mono" fillcolor="#00001a" color="{BLUE}" fontcolor="{BLUE}"]
        uc03out [label="UC03 Out\nstereo playback" fillcolor="#00001a" color="{TEAL}" fontcolor="{TEAL}"]
        mobo    [label="Motherboard\naudio (ignored)" fillcolor="#00001a" color="{DIM}" fontcolor="{DIM}"]
    }}

    subgraph cluster_pw {{
        bgcolor="#050515" color="{MAGENTA}" label="PipeWire Nodes" fontcolor="{MAGENTA}" penwidth=2
        src  [label="source\nmono-fallback" fillcolor="#0a000a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
        sink [label="sink\nanalog-stereo" fillcolor="#0a000a" color="{PINK}" fontcolor="{PINK}"]
        mon  [label="monitor\n(loopback — disabled)" fillcolor="#0a000a" color="{DIM}" fontcolor="{DIM}"]
    }}

    subgraph cluster_cfg {{
        bgcolor="#050515" color="{GREEN}" label="WirePlumber Config" fontcolor="{GREEN}" penwidth=2
        wp [label="51-default-audio.conf\ndefault source: mono-fallback\ndefault sink: analog-stereo" fillcolor="#001500" color="{GREEN}" fontcolor="{GREEN}"]
    }}

    uc03in  -> src  [color="{MAGENTA}"]
    uc03out -> sink [color="{PINK}"]
    mobo    -> mon  [color="{DIM}" style=dashed]
    wp      -> src  [label="set default" color="{GREEN}" style=dashed]
    wp      -> sink [label="set default" color="{GREEN}" style=dashed]
}}
""")

render("audio_03_sample_rate", f"""
digraph {{
    bgcolor="{BG3}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]
    rankdir=LR

    uc03  [label="UC03 native\n32000 Hz" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    pw32  [label="PipeWire\n32kHz passthrough" fillcolor="#001520" color="{TEAL}" fontcolor="{TEAL}"]
    parec [label="parec\n--rate=32000" fillcolor="#001520" color="{GREEN}" fontcolor="{GREEN}"]
    sox   [label="sox convert\n-r 32000 -e signed\n-b 16 -c 1" fillcolor="#001520" color="{LIME}" fontcolor="{LIME}"]
    wav   [label="WAV\n32kHz mono s16le" fillcolor="#001520" color="{YELLOW}" fontcolor="{YELLOW}"]
    fw    [label="faster-whisper\nnative 32kHz support\n(no internal resample)" fillcolor="#001520" color="{MAGENTA}" fontcolor="{MAGENTA}"]

    uc03  -> pw32  [label="USB audio\n32kHz" color="{CYAN}"]
    pw32  -> parec [label="PCM stream" color="{TEAL}"]
    parec -> sox   [label="raw s16le\n32kHz" color="{GREEN}"]
    sox   -> wav   [label="write" color="{LIME}"]
    wav   -> fw    [label="transcribe" color="{YELLOW}"]

    bad [label="WRONG: --rate=16000\n→ PipeWire resampler\n→ near-silence (RMS ~0.0005)" shape=note fillcolor="#1a0000" color="{RED}" fontcolor="{RED}"]
    bad -> parec [label="avoided" color="{RED}" style=dashed]
}}
""")

render("audio_04_raw_wav_conversion", f"""
digraph {{
    bgcolor="{BG4}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    parec [label="parec\n(background process)" shape=box fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    raw   [label="/tmp/voice-XXXXXX.raw\nraw s16le PCM\n(no header)" shape=cylinder fillcolor="#0d0d1a" color="{BLUE}" fontcolor="{BLUE}"]
    kill  [label="parec exits\n(Enter or timeout)" shape=diamond fillcolor="#0d0d0d" color="{ORANGE}" fontcolor="{ORANGE}"]
    sox   [label="sox convert\n(after clean exit)" shape=box fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    wav   [label="/tmp/voice-XXXXXX.wav\nvalid WAV with header" shape=cylinder fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]

    old1  [label="OLD: parec | sox pipeline\n(pipe to WAV directly)" shape=note fillcolor="#1a0000" color="{RED}" fontcolor="{RED}"]
    old2  [label="Problem: killing pipeline\nleft WAV header incomplete\n→ corrupt file" shape=note fillcolor="#1a0000" color="{RED}" fontcolor="{RED}"]

    parec -> raw  [label="write raw bytes" color="{CYAN}"]
    raw -> kill   [color="{DIM}" style=dashed]
    kill -> sox   [label="then convert" color="{GREEN}"]
    sox -> wav    [color="{LIME}"]
    old1 -> old2  [color="{RED}" style=dashed]
}}
""")

render("audio_05_timing_buffer", f"""
digraph {{
    bgcolor="{BG5}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    t0   [label="t=0s\nvoice-input runs\nparec starts" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    rec  [label="t=0..65s\nRecording window\n32kHz × 2 bytes × N sec\n= up to 4.0 MB raw" fillcolor="#001520" color="{TEAL}" fontcolor="{TEAL}"]
    ent  [label="Enter pressed\n(any time 0..65s)" shape=diamond fillcolor="#0d0d0d" color="{ORANGE}" fontcolor="{ORANGE}"]
    t65  [label="t=65s\nTimer fires USR1\nparec killed" fillcolor="#0d0d0d" color="{RED}" fontcolor="{RED}"]
    conv [label="sox conversion\n~10-50ms" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    infer [label="GPU inference\n1-5s typical\n(model already loaded)" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]

    t0 -> rec   [color="{CYAN}"]
    rec -> ent  [label="if user presses Enter" color="{ORANGE}"]
    rec -> t65  [label="if no input" color="{RED}"]
    ent -> conv [color="{GREEN}"]
    t65 -> conv [color="{GREEN}"]
    conv -> infer [color="{MAGENTA}"]
}}
""")


# ══════════════════════════════════════════════════════════════════════════════
# 3. PROCESS CONTROL
# ══════════════════════════════════════════════════════════════════════════════

render("proc_01_process_tree", f"""
digraph {{
    bgcolor="{BG}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    shell  [label="zsh / bash\n(user's shell)" fillcolor="#0a000a" color="{VIOLET}" fontcolor="{VIOLET}"]
    main   [label="voice-input.sh\n(MAIN_PID=$$)" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    parec  [label="parec\n(PAREC_PID)\nwrites raw PCM" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    timer  [label="( sleep 65; kill... )\n(TIMER_PID)\nsubshell background" fillcolor="#1a0a00" color="{ORANGE}" fontcolor="{ORANGE}"]
    beep   [label="paplay\n(background &)\nbeep tones" fillcolor="#0a0a0a" color="{TEAL}" fontcolor="{TEAL}"]
    py     [label="python3 transcribe.py\n(inline, waits)" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]

    shell -> main  [color="{VIOLET}"]
    main  -> parec [label="& (background)" color="{GREEN}"]
    main  -> timer [label="& (background)" color="{ORANGE}"]
    main  -> beep  [label="& (fire-forget)" color="{TEAL}"]
    main  -> py    [label="waits for exit" color="{MAGENTA}"]
}}
""")

render("proc_02_signal_flow", f"""
digraph {{
    bgcolor="{BG2}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    enter  [label="User presses Enter\n(read < /dev/tty)" shape=parallelogram fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    timer  [label="65s timer fires\n(background subshell)" shape=box fillcolor="#1a0a00" color="{ORANGE}" fontcolor="{ORANGE}"]

    kill_p [label="kill PAREC_PID" shape=box fillcolor="#0d0d0d" color="{RED}" fontcolor="{RED}"]
    usr1   [label="kill -USR1 MAIN_PID" shape=box fillcolor="#0d0d0d" color="{RED}" fontcolor="{RED}"]
    trap   [label="trap 'true' USR1\n(interrupts read)" shape=box fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]

    read_unblock [label="read unblocked" shape=diamond fillcolor="#0d0d0d" color="{YELLOW}" fontcolor="{YELLOW}"]
    kill_t [label="kill TIMER_PID\nkill PAREC_PID\nwait PAREC_PID" shape=box fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    cont   [label="Continue to sox + transcribe" shape=oval fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]

    enter -> read_unblock [color="{CYAN}" label="direct"]
    timer -> kill_p       [color="{ORANGE}"]
    timer -> usr1         [color="{ORANGE}"]
    usr1  -> trap         [color="{RED}"]
    trap  -> read_unblock [color="{GREEN}" label="via signal"]
    read_unblock -> kill_t [color="{YELLOW}"]
    kill_t -> cont         [color="{LIME}"]
}}
""")

render("proc_03_state_machine", f"""
digraph {{
    bgcolor="{BG3}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=11 style=filled shape=oval penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    IDLE   [label="IDLE" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    REC    [label="RECORDING" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    STOP   [label="STOPPING" fillcolor="#1a0a00" color="{ORANGE}" fontcolor="{ORANGE}"]
    CONV   [label="CONVERTING\n(sox)" fillcolor="#0a001a" color="{VIOLET}" fontcolor="{VIOLET}"]
    INFER  [label="INFERRING\n(GPU)" fillcolor="#1a0020" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    OUT    [label="OUTPUT" fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]
    DONE   [label="DONE / EXIT" fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]

    IDLE  -> REC   [label="voice-input runs\nparec + timer started\nbeep(480Hz)" color="{CYAN}"]
    REC   -> STOP  [label="Enter pressed\nOR 65s USR1" color="{ORANGE}"]
    STOP  -> CONV  [label="parec killed\ntimer killed\nbeep(880Hz)" color="{VIOLET}"]
    CONV  -> INFER [label="WAV written" color="{MAGENTA}"]
    INFER -> OUT   [label="transcript ready" color="{LIME}"]
    OUT   -> DONE  [label="type / clip / print" color="{DIM}"]
}}
""")

render("proc_04_timer_logic", f"""
digraph {{
    bgcolor="{BG4}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    start  [label="parec & (PAREC_PID)\ntimer subshell & (TIMER_PID)\ntrap 'true' USR1" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    read   [label="read -r _ < /dev/tty\n(blocks here)" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    path_e [label="Enter key\n(read returns)" shape=diamond fillcolor="#0d0d0d" color="{YELLOW}" fontcolor="{YELLOW}"]
    path_t [label="65s elapsed:\nkill PAREC_PID\nkill -USR1 MAIN_PID\n(read interrupted)" shape=diamond fillcolor="#0d0d0d" color="{ORANGE}" fontcolor="{ORANGE}"]
    cleanup [label="trap - USR1\nkill TIMER_PID\nkill PAREC_PID\nwait PAREC_PID" fillcolor="#001a00" color="{LIME}" fontcolor="{LIME}"]

    start -> read    [color="{CYAN}"]
    read  -> path_e  [label="Enter" color="{YELLOW}"]
    read  -> path_t  [label="USR1 signal" color="{ORANGE}"]
    path_e -> cleanup [color="{LIME}"]
    path_t -> cleanup [color="{LIME}"]
}}
""")

render("proc_05_cleanup", f"""
digraph {{
    bgcolor="{BG5}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    trap_exit [label="trap cleanup EXIT\n(registered at startup)" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    fn_clean  [label="cleanup()\n  rm -f TMPRAW\n  rm -f TMPWAV" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]

    normal [label="Normal exit\n(text output done)" shape=oval fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]
    err    [label="set -euo pipefail\n(any error exits)" shape=oval fillcolor="#1a0000" color="{RED}" fontcolor="{RED}"]
    sig    [label="SIGINT / SIGTERM\n(Ctrl+C)" shape=oval fillcolor="#1a0a00" color="{ORANGE}" fontcolor="{ORANGE}"]

    cleanup_run [label="cleanup() runs\n→ temp files deleted" shape=box fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]

    normal -> cleanup_run [color="{LIME}"]
    err    -> cleanup_run [color="{RED}"]
    sig    -> cleanup_run [color="{ORANGE}"]
    trap_exit -> fn_clean [label="registered to" color="{CYAN}" style=dashed]
    fn_clean  -> cleanup_run [label="executes" color="{GREEN}" style=dashed]
}}
""")


# ══════════════════════════════════════════════════════════════════════════════
# 4. TRANSCRIPTION PIPELINE
# ══════════════════════════════════════════════════════════════════════════════

render("infer_01_model_pipeline", f"""
digraph {{
    bgcolor="{BG}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]
    rankdir=LR

    wav   [label="WAV file\n32kHz mono" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    mel   [label="Log-Mel\nSpectrogram\n(80 bins)" fillcolor="#001520" color="{BLUE}" fontcolor="{BLUE}"]
    enc   [label="Whisper\nEncoder\n(transformer)" fillcolor="#0a001a" color="{VIOLET}" fontcolor="{VIOLET}"]
    dec   [label="Whisper\nDecoder\n(autoregressive)" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    tok   [label="Token\nsequence" fillcolor="#0a001a" color="{PINK}" fillcolor="#0a001a"]
    text  [label="Text\nsegments + words\n+ timestamps + prob" fillcolor="#001500" color="{GREEN}" fontcolor="{GREEN}"]

    wav -> mel  [label="librosa/av" color="{CYAN}"]
    mel -> enc  [label="float16\nCUDA" color="{VIOLET}"]
    enc -> dec  [label="key-value cache" color="{MAGENTA}"]
    dec -> tok  [label="beam_size=5" color="{PINK}"]
    tok -> text [label="detokenize" color="{GREEN}"]
}}
""")

render("infer_02_cuda_layers", f"""
digraph {{
    bgcolor="{BG2}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]
    rankdir=BT

    vram [label="NVIDIA VRAM\n~1.5GB (medium model)\nRTX 3060 + RTX 3080" fillcolor="#0a0000" color="{RED}" fontcolor="{RED}"]
    cuda [label="CUDA 12 runtime\nlibcublas.so.12\n(/usr/lib/ollama/)" fillcolor="#1a0000" color="{ORANGE}" fontcolor="{ORANGE}"]
    ct2  [label="ctranslate2\n(fast inference engine)" fillcolor="#1a0010" color="{PINK}" fontcolor="{PINK}"]
    fw   [label="faster-whisper\n(Python wrapper)" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    py   [label="transcribe.py\nWhisperModel('medium', device='cuda',\ncompute_type='float16')" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]

    vram -> cuda [color="{RED}"]
    cuda -> ct2  [color="{ORANGE}"]
    ct2  -> fw   [color="{PINK}"]
    fw   -> py   [color="{MAGENTA}"]
}}
""")

render("infer_03_segment_struct", f"""
digraph {{
    bgcolor="{BG3}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    seg  [label="Segment\n────────────────\n.start  float\n.end    float\n.text   str\n.words  list[Word]" shape=record fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    word [label="Word\n────────────────\n.start       float\n.end         float\n.word        str\n.probability float (0-1)" shape=record fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]

    prob_hi  [label="prob ≥ 0.92\n→ bright white" shape=box fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    prob_mid [label="0.75 ≤ prob < 0.92\n→ normal" shape=box fillcolor="#0d0d0d" color="{WHITE}" fontcolor="{WHITE}"]
    prob_unc [label="0.50 ≤ prob < 0.75\n→ yellow" shape=box fillcolor="#1a1a00" color="{YELLOW}" fontcolor="{YELLOW}"]
    prob_lo  [label="prob < 0.50\n→ red" shape=box fillcolor="#1a0000" color="{RED}" fontcolor="{RED}"]

    seg -> word    [label="1:N" color="{MAGENTA}"]
    word -> prob_hi [label=".probability" color="{GREEN}"]
    word -> prob_mid [color="{WHITE}"]
    word -> prob_unc [color="{YELLOW}"]
    word -> prob_lo  [color="{RED}"]
}}
""")

render("infer_04_faster_whisper", f"""
digraph {{
    bgcolor="{BG4}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    load  [label="WhisperModel('medium',\n  device='cuda',\n  compute_type='float16')\n→ loads to VRAM once" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    call  [label="model.transcribe(\n  wav_path,\n  beam_size=5,\n  language='en',\n  word_timestamps=True\n)" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    gen   [label="generator yields\nSegment objects\n(streaming — not buffered)" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    loop  [label="for segment in segments:\n  for word in segment.words:" fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]
    out   [label="animate_word()\nor print()" fillcolor="#1a001a" color="{PINK}" fontcolor="{PINK}"]

    load -> call [label="call transcribe()" color="{CYAN}"]
    call -> gen  [label="returns generator" color="{GREEN}"]
    gen  -> loop [label="iterate" color="{LIME}"]
    loop -> out  [color="{PINK}"]
}}
""")

render("infer_05_model_loading", f"""
digraph {{
    bgcolor="{BG5}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    inv   [label="transcribe.py invoked" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    cache [label="~/.cache/huggingface/\nhub/models--Systran--\nfaster-whisper-medium\n(~1.5 GB)" shape=cylinder fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    chk   [label="Already cached?" shape=diamond fillcolor="#0d0d0d" color="{YELLOW}" fontcolor="{YELLOW}"]
    dl    [label="Download from\nHuggingFace Hub\n(first run only)" fillcolor="#1a0a00" color="{ORANGE}" fontcolor="{ORANGE}"]
    vram  [label="Load into VRAM\nfloat16\n~1.5GB" fillcolor="#1a0000" color="{RED}" fontcolor="{RED}"]
    ready [label="Model ready\n→ transcribe()" fillcolor="#001500" color="{GREEN}" fontcolor="{GREEN}"]

    inv -> chk    [color="{CYAN}"]
    chk -> dl     [label="no" color="{ORANGE}"]
    chk -> vram   [label="yes" color="{GREEN}"]
    dl  -> cache  [label="save" color="{ORANGE}"]
    cache -> vram [color="{RED}"]
    vram -> ready [color="{GREEN}"]
}}
""")


# ══════════════════════════════════════════════════════════════════════════════
# 5. OUTPUT MODES
# ══════════════════════════════════════════════════════════════════════════════

render("out_01_dispatch", f"""
digraph {{
    bgcolor="{BG}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    args  [label="CLI args parsed\n--print / --clip / (default)" shape=parallelogram fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    fancy [label="FANCY flag\n--no-fancy?" shape=diamond fillcolor="#0d0d0d" color="{VIOLET}" fontcolor="{VIOLET}"]

    f_print [label="fancy_transcribe()\nstream animation\n→ exit" shape=box fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    p_text  [label="plain_transcribe()\nTEXT=capture stdout" shape=box fillcolor="#001500" color="{GREEN}" fontcolor="{GREEN}"]

    mode  [label="MODE switch\ntype / clip / print" shape=diamond fillcolor="#0d0d0d" color="{YELLOW}" fontcolor="{YELLOW}"]
    type_ [label="xdotool type\n--clearmodifiers\n--delay 20" shape=box fillcolor="#001520" color="{BLUE}" fontcolor="{BLUE}"]
    clip  [label="echo -n | xclip\n-selection clipboard" shape=box fillcolor="#001520" color="{TEAL}" fontcolor="{TEAL}"]
    print [label="echo TEXT\n→ stdout" shape=box fillcolor="#001520" color="{LIME}" fontcolor="{LIME}"]

    args  -> fancy  [color="{CYAN}"]
    fancy -> f_print [label="fancy + print" color="{MAGENTA}"]
    fancy -> p_text  [label="no-fancy OR clip/type" color="{GREEN}"]
    p_text -> mode   [color="{YELLOW}"]
    mode  -> type_   [label="type" color="{BLUE}"]
    mode  -> clip    [label="clip" color="{TEAL}"]
    mode  -> print   [label="print" color="{LIME}"]
}}
""")

render("out_02_fancy_vs_plain", f"""
digraph {{
    bgcolor="{BG2}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    subgraph cluster_fancy {{
        bgcolor="#050515" color="{MAGENTA}" label="fancy mode (default)" fontcolor="{MAGENTA}" penwidth=2
        fw_f  [label="word_timestamps=True" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
        anim  [label="animate_word()\nscramble → resolve\n+ confidence color\n+ dim timestamp" fillcolor="#0a001a" color="{PINK}" fontcolor="{PINK}"]
        stream [label="streams live to tty\n(no capture into $())" fillcolor="#0a001a" color="{VIOLET}" fontcolor="{VIOLET}"]
    }}

    subgraph cluster_plain {{
        bgcolor="#051505" color="{GREEN}" label="--no-fancy mode" fontcolor="{GREEN}" penwidth=2
        fw_p  [label="word_timestamps=False\n(faster)" fillcolor="#001500" color="{GREEN}" fontcolor="{GREEN}"]
        join  [label="' '.join(seg.text)" fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]
        cap   [label="captured into TEXT\nfor dispatch" fillcolor="#001500" color="{TEAL}" fontcolor="{TEAL}"]
    }}

    fw_f -> anim   [color="{MAGENTA}"]
    anim -> stream [color="{PINK}"]
    fw_p -> join   [color="{GREEN}"]
    join -> cap    [color="{LIME}"]
}}
""")

render("out_03_xdotool", f"""
digraph {{
    bgcolor="{BG3}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    text  [label="TEXT variable\n(plain transcript)" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    sleep [label="sleep 0.1\n(focus settle time)" fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]
    xdo   [label="xdotool type\n  --clearmodifiers\n  --delay 20\n  'TEXT'" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    win   [label="Active X11 window\n(whatever has focus)" fillcolor="#001520" color="{BLUE}" fontcolor="{BLUE}"]
    typed [label="Text appears\nas if typed by keyboard" shape=parallelogram fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]

    note  [label="use case: type into\nClaude Code prompt\nwithout copy-paste" shape=note fillcolor="#0a0a0a" color="{DIM}" fontcolor="{DIM}"]

    text -> sleep [color="{CYAN}"]
    sleep -> xdo  [color="{GREEN}"]
    xdo -> win    [label="X11 fake key events" color="{BLUE}"]
    win -> typed  [color="{LIME}"]
    note -> xdo   [style=dashed color="{DIM}"]
}}
""")

render("out_04_clipboard", f"""
digraph {{
    bgcolor="{BG4}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    text  [label="TEXT variable" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    xcl   [label="echo -n TEXT\n| xclip -selection clipboard" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    cb    [label="X11 CLIPBOARD\nbuffer" shape=cylinder fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]
    msg   [label="'Copied to clipboard.'\n→ stderr" fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]

    paste [label="Ctrl+Shift+V\n(terminal paste)\nor Ctrl+V (other apps)" shape=parallelogram fillcolor="#001520" color="{TEAL}" fontcolor="{TEAL}"]
    dest  [label="Target app\n(Claude Code, editor, etc.)" shape=parallelogram fillcolor="#001520" color="{BLUE}" fontcolor="{BLUE}"]

    text -> xcl   [color="{CYAN}"]
    xcl  -> cb    [label="write" color="{GREEN}"]
    xcl  -> msg   [color="{DIM}"]
    cb   -> paste [label="user pastes" color="{TEAL}"]
    paste -> dest [color="{BLUE}"]
}}
""")

render("out_05_end_to_end", f"""
digraph {{
    bgcolor="{BG5}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=9 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=8 penwidth=1.5]
    rankdir=LR

    UC03  [label="UC03\nMic" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    parec [label="parec\n32kHz" fillcolor="#001520" color="{BLUE}" fontcolor="{BLUE}"]
    raw   [label=".raw\nPCM" shape=cylinder fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]
    sox   [label="sox\nconvert" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    wav   [label=".wav" shape=cylinder fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]
    fw    [label="faster-\nwhisper\nGPU" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]

    fancy_out [label="animated\nterminal" fillcolor="#0a001a" color="{PINK}" fontcolor="{PINK}"]
    plain_out [label="TEXT\nstring" fillcolor="#001500" color="{GREEN}" fontcolor="{GREEN}"]

    type_ [label="xdotool\ntype" fillcolor="#001520" color="{YELLOW}" fontcolor="{YELLOW}"]
    clip  [label="xclip\nclipboard" fillcolor="#001520" color="{TEAL}" fontcolor="{TEAL}"]
    print [label="stdout\nprint" fillcolor="#001520" color="{LIME}" fontcolor="{LIME}"]

    UC03 -> parec -> raw -> sox -> wav -> fw
    fw -> fancy_out [label="--fancy" color="{PINK}"]
    fw -> plain_out [label="--no-fancy" color="{GREEN}"]
    plain_out -> type_  [label="(default)" color="{YELLOW}"]
    plain_out -> clip   [label="--clip" color="{TEAL}"]
    plain_out -> print  [label="--print\n--no-fancy" color="{LIME}"]
    fancy_out -> print  [label="--print" color="{MAGENTA}"]
}}
""")


# ══════════════════════════════════════════════════════════════════════════════
# 6. ANIMATION RENDERER
# ══════════════════════════════════════════════════════════════════════════════

render("anim_01_word_state_machine", f"""
digraph {{
    bgcolor="{BG}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled shape=oval penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    WAIT    [label="WAITING\nfor model" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    PH      [label="PLACEHOLDER\n___ printed" fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]
    SCRAM   [label="SCRAMBLING\nframes = int((1-prob)×10)" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    SNAP    [label="SNAPPED\nreal word printed" fillcolor="#001500" color="{GREEN}" fontcolor="{GREEN}"]
    ADV     [label="ADVANCE\ncursor left behind\nto next word" fillcolor="#001520" color="{TEAL}" fontcolor="{TEAL}"]

    WAIT  -> PH    [label="word yielded\nfrom generator" color="{CYAN}"]
    PH    -> SCRAM [label="animate_word()" color="{MAGENTA}"]
    SCRAM -> SCRAM [label="next frame\n(0.035s sleep)" color="{MAGENTA}"]
    SCRAM -> SNAP  [label="frames exhausted" color="{GREEN}"]
    SNAP  -> ADV   [label="cursor stays\npast word" color="{TEAL}"]
    ADV   -> WAIT  [label="next word" color="{CYAN}"]
}}
""")

render("anim_02_scramble_loop", f"""
digraph {{
    bgcolor="{BG2}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    calc  [label="frames = max(1, int((1 - prob) × 10))\n\nexample:\nprob=0.95 → frames=0 → max → 1\nprob=0.50 → frames=5\nprob=0.10 → frames=9" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    back  [label="back = width + len(ts_raw) + 1\n(columns to step back)" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    loop  [label="for _ in range(frames):" fillcolor="#001a00" color="{GREEN}" fontcolor="{GREEN}"]
    rand  [label="scrambled = random chars\n(len == width)" fillcolor="#001a00" color="{LIME}" fontcolor="{LIME}"]
    write [label="write: dim scrambled + ts + space\n+ \\033[backD  ← cursor left back cols" fillcolor="#001a00" color="{TEAL}" fontcolor="{TEAL}"]
    sleep [label="sleep 0.035s" fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]
    final [label="write: color + real_word + ts + space\n(no cursor left — advance normally)" fillcolor="#001500" color="{YELLOW}" fontcolor="{YELLOW}"]

    calc  -> back  [color="{CYAN}"]
    back  -> loop  [color="{MAGENTA}"]
    loop  -> rand  [color="{GREEN}"]
    rand  -> write [color="{LIME}"]
    write -> sleep [color="{TEAL}"]
    sleep -> loop  [label="next frame" color="{DIM}"]
    loop  -> final [label="done" color="{YELLOW}"]
}}
""")

render("anim_03_cursor_positioning", f"""
digraph {{
    bgcolor="{BG3}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    ph    [label="placeholder written:\n___¹²·³ˢ \n(width + ts + space = back cols)" fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]
    cur_a [label="cursor at position A\n(after placeholder)" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    frame [label="write scramble:\nxyz¹²·³ˢ \n+ \\033[backD" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    cur_b [label="cursor steps LEFT back cols\n→ back at position A-back\n= start of placeholder" fillcolor="#0a001a" color="{PINK}" fontcolor="{PINK}"]
    final [label="write real word:\nword¹²·³ˢ \n(no cursor-left)" fillcolor="#001500" color="{GREEN}" fontcolor="{GREEN}"]
    cur_c [label="cursor at A\n(after word+ts+space)\n= correct position for\nnext word" fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]

    bad   [label="OLD: \\033[s / \\033[u\ncursor save/restore\nFAILS silently in tmux,\nVTE terminals" shape=note fillcolor="#1a0000" color="{RED}" fontcolor="{RED}"]

    ph    -> cur_a [color="{CYAN}"]
    cur_a -> frame [color="{MAGENTA}"]
    frame -> cur_b [color="{PINK}"]
    cur_b -> frame [label="next frame" color="{MAGENTA}"]
    cur_b -> final [label="all frames done" color="{GREEN}"]
    final -> cur_c [color="{LIME}"]
    bad   -> frame [label="replaced by" color="{RED}" style=dashed]
}}
""")

render("anim_04_confidence_colors", f"""
digraph {{
    bgcolor="{BG4}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=11 style=filled penwidth=2]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    fn   [label="word_color(prob)" shape=box fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    p92  [label="prob ≥ 0.92\n\\033[97m\nbright white" shape=box fillcolor="#1a1a1a" color="#e0e0e0" fontcolor="#e0e0e0"]
    p75  [label="prob ≥ 0.75\n\\033[0m\nnormal" shape=box fillcolor="#0d0d0d" color="{WHITE}" fontcolor="{WHITE}"]
    p50  [label="prob ≥ 0.50\n\\033[33m\nyellow" shape=box fillcolor="#1a1a00" color="{YELLOW}" fontcolor="{YELLOW}"]
    plo  [label="prob < 0.50\n\\033[31m\nred" shape=box fillcolor="#1a0000" color="{RED}" fontcolor="{RED}"]

    fn -> p92 [label="high confidence" color="#e0e0e0"]
    fn -> p75 [label="medium-high" color="{WHITE}"]
    fn -> p50 [label="uncertain" color="{YELLOW}"]
    fn -> plo [label="low confidence" color="{RED}"]

    frames [label="scramble frames:\nhigh prob → fewer (min 1)\nlow prob  → more (max 9)" shape=note fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    frames -> fn [style=dashed color="{MAGENTA}" label="also affects"]
}}
""")

render("anim_05_terminal_render", f"""
digraph {{
    bgcolor="{BG5}" fontname="monospace" fontcolor="{WHITE}"
    node [fontname="monospace" fontsize=10 style=filled penwidth=2 shape=box]
    edge [fontname="monospace" fontsize=9 penwidth=1.5]

    seg   [label="Segment arrives\nfrom generator\n(streaming — not waiting for all)" fillcolor="#001520" color="{CYAN}" fontcolor="{CYAN}"]
    wrap  [label="col + len(w) + 12 > 100?\n→ \\n + reset col=0" shape=diamond fillcolor="#0d0d0d" color="{ORANGE}" fontcolor="{ORANGE}"]
    ph    [label="write placeholder:\n\\033[2m____\\033[0m¹²·³ˢ " fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]
    anim  [label="animate_word(w, prob, start)\n→ scramble frames\n→ cursor-left\n→ snap to real word" fillcolor="#0a001a" color="{MAGENTA}" fontcolor="{MAGENTA}"]
    adv   [label="col += len(w) + len(ts_raw) + 1" fillcolor="#001500" color="{LIME}" fontcolor="{LIME}"]
    nl    [label="sys.stdout.write('\\n')\nat end of all segments" fillcolor="#0d0d0d" color="{DIM}" fontcolor="{DIM}"]

    seg  -> wrap [color="{CYAN}"]
    wrap -> ph   [label="no wrap" color="{DIM}"]
    wrap -> ph   [label="wrap: \\n first" color="{ORANGE}"]
    ph   -> anim [color="{MAGENTA}"]
    anim -> adv  [color="{LIME}"]
    adv  -> seg  [label="next word" color="{CYAN}"]
    adv  -> nl   [label="all done" color="{DIM}"]
}}
""")

print(f"\nAll diagrams written to {OUT}/")
print(f"PNG + SVG generated for each.")
