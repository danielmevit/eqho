# Eqho test on an Intel-only laptop (Dell Latitude 5400) — plan for a Claude Code session

You are a fresh Claude Code session on the owner's (Daniel's) Intel laptop. Eqho is his local voice-to-text app.
It has two speech engines:
- **faster-whisper** — runs on the processor (CPU) on this laptop (it only uses NVIDIA graphics cards).
- **whisper.cpp** — can use Intel/AMD graphics through **Vulkan**. New in the Windows downloads from v0.8.9.

**Goal:** measure both engines on this laptop (accuracy + speed), check that the Intel graphics are really used, and
check that the test build works. The lead (another Claude session on his main PC) uses your numbers to decide what
"Auto" should pick on PCs like this one.

## Rules
- Work only inside this kit folder (called `KIT` below — the folder this file is in). Use PowerShell.
- You may install **Python 3.13** with `winget install -e --id Python.Python.3.13` if `py -3.13 --version` fails.
  Ask Daniel before installing anything else (drivers included).
- Never push, upload or send anything anywhere. No git needed.
- If a step fails, write down the exact error, try one sensible fix, then move on. Don't spend long on one step.
- Keep Daniel posted with one short line per step.

## What's in KIT
- `eqho-src.zip` — Eqho source at the tested commit (includes `requirements-lock.txt`, `tools/bench_asr.py`, and the
  engine wheel at `packaging/windows/wheels/`).
- `Eqho-portable-*-win-x64.zip` — the test build (what users download). It is the 0.8.9 work-in-progress but still
  reports version **0.8.8** (the version number is bumped at release) — that's expected.
- `data/LibriSpeech/test-clean` and `test-other` — 60 + 60 public recordings with known text (CC BY 4.0).
- `reference/*.csv` — results from the main PC (NVIDIA RTX 3060) on the same clips: `gpu-*` = faster-whisper on
  NVIDIA, `cpu-*` = whisper.cpp (Vulkan on NVIDIA). Use the first 60 rows of each.

## Steps

### 1. System info
Record in RESULTS.md: CPU name, cores/threads, RAM, Windows version, graphics adapter + driver version + date:
```powershell
Get-CimInstance Win32_Processor | Select Name, NumberOfCores, NumberOfLogicalProcessors
[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)
Get-CimInstance Win32_VideoController | Select Name, DriverVersion, DriverDate
(Get-CimInstance Win32_OperatingSystem).Caption + " " + (Get-CimInstance Win32_OperatingSystem).Version
```

### 2. Set up the source
```powershell
Expand-Archive "$KIT\eqho-src.zip" "$KIT\src"
py -3.13 -m venv "$KIT\venv"
& "$KIT\venv\Scripts\python.exe" -m pip install --upgrade pip
& "$KIT\venv\Scripts\python.exe" -m pip install -r "$KIT\src\requirements-lock.txt"
& "$KIT\venv\Scripts\python.exe" -m pip install --no-deps (Get-ChildItem "$KIT\src\packaging\windows\wheels\*.whl").FullName
cd "$KIT\src"; & "$KIT\venv\Scripts\python.exe" run.py --smoke
```
The smoke check must print `"ok": true`. (It downloads a small test model, about 150 MB, the first time.)

### 3. Is the Intel graphics card visible to the engine?
```powershell
cd "$KIT\src"
& "$KIT\venv\Scripts\python.exe" -c "import _pywhispercpp as w; print(w.whisper_print_system_info())"
& "$KIT\venv\Scripts\python.exe" -c "from src.model_host import resolve_backend; print('auto picks:', resolve_backend('auto'))"
```
Record the `ggml_vulkan:` device lines (expect "Intel(R) UHD Graphics 620" or similar) and what auto picks.
If there is **no** ggml_vulkan device: tell Daniel the Intel graphics driver probably needs updating (Windows Update →
Advanced options → Optional updates, or Intel's driver tool), ask him to do it, then repeat this step. Continue with
the rest either way.

### 4. Accuracy + speed: both engines, both sets (4 runs, roughly 30–60 minutes in total)
Each engine downloads its model the first time (about 1.5 GB each) — that is expected.
Create two settings folders, then run the benchmark four times. Save the full console output of each run.
```powershell
cd "$KIT\src"
foreach ($e in "whisper.cpp","faster-whisper") {
  New-Item -ItemType Directory -Force "$KIT\cfg-$e" | Out-Null
  @{ model_size="distil-large-v3"; language="en"; engine_backend=$e; model_dir="$KIT\models" } |
    ConvertTo-Json | Set-Content -Encoding utf8 "$KIT\cfg-$e\settings.json"
}
foreach ($e in "whisper.cpp","faster-whisper") {
  foreach ($s in "clean","other") {
    $env:EQHO_CONFIG_DIR = "$KIT\cfg-$e"
    & "$KIT\venv\Scripts\python.exe" tools\bench_asr.py --data "$KIT\data\LibriSpeech\test-$s" --limit 60 `
        --allow-download --work "$KIT\runs\$e-$s" 2>&1 | Tee-Object "$KIT\runs\$e-$s.log"
  }
}
Remove-Item Env:EQHO_CONFIG_DIR
```
From each run's summary record: WER, RTF / "x realtime", model load time, the backend/device line. For whisper.cpp
also check the log for `ggml_vulkan` lines (did it use the Intel GPU?). While whisper.cpp runs, note the GPU load in
Task Manager → Performance → GPU if you can read it (e.g. `Get-Counter '\GPU Engine(*engtype_3D)\Utilization Percentage'`).

### 5. The test build
```powershell
Expand-Archive (Get-ChildItem "$KIT\Eqho-portable-*-win-x64.zip").FullName "$KIT\app"
Test-Path "$KIT\app\_internal\ggml-vulkan.dll"
$p = Start-Process "$KIT\app\Eqho.exe" -ArgumentList "--smoke" -Wait -PassThru -RedirectStandardOutput "$KIT\app-smoke.txt" -RedirectStandardError "$KIT\app-smoke-err.txt"
$p.ExitCode; Get-Content "$KIT\app-smoke.txt"
```
(The build has no console window; if the smoke output file stays empty, say so — that's a finding, not a failure.)

### 6. Real dictation on the desktop (ONLY with Daniel's OK)
This presses Alt+Q and types into a test window for about a minute; Daniel must not touch the keyboard or mouse.
Ask him first. If he agrees:
```powershell
cd "$KIT\src"
$env:EQHO_CONFIG_DIR = "$KIT\cfg-whisper.cpp"
& "$KIT\venv\Scripts\python.exe" tools\test_e2e.py --data "$KIT\data\LibriSpeech\test-clean" --clips 3 --gate-wer 0.25 --work "$KIT\runs\e2e"
Remove-Item Env:EQHO_CONFIG_DIR
```
If it refuses because a model isn't cached for that config, record the message and skip.

### 7. RESULTS.md (in KIT)
- System info (step 1), Vulkan devices + auto pick (step 3).
- A table: engine × set → WER, x realtime, model load s — and next to it the reference numbers for the **same 60
  clips** from `reference/*.csv` (total WER = sum(sub+del+ins) / sum(ref_words); RTF = sum(transcribe_s) /
  sum(audio_s)).
- Did whisper.cpp use the Intel GPU? Which engine is faster here, and by how much? Is accuracy similar?
- Step 5 and 6 results, every error message, anything odd.
- Your recommendation: on a PC like this, should Auto pick whisper.cpp (Intel GPU) or faster-whisper (CPU)?

When done, tell Daniel: "Finished — RESULTS.md is in the kit folder." He'll bring it back to the main PC.
