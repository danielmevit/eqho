# Eqho test on the Intel laptop — run 2 (check the fixes + the new small default model)

You are a Claude Code session on Daniel's Intel laptop (Dell Latitude 5400). Eqho is his open-source, offline
voice-to-text app (github.com/danielmevit/eqho). Run 1 (RESULTS.md, if still in this folder) found that on this
laptop Eqho typed nothing after a dictation (it waited only 5 s for the text), that Auto picked the slower engine, and
that the default model needs 14–26 s per sentence here.

Since then (Eqho 0.8.9 released, 0.8.10 on the `dev` branch): Eqho waits for the text instead of dropping it, Auto
uses the processor on built-in graphics, and on PCs without a suitable graphics card new installs get a small fast
model (Distil Small EN). **Goal of run 2:** confirm all that on this real laptop, and measure the small models here.
Everything uses recorded public audio files (LibriSpeech, CC BY 4.0). Nothing is sent anywhere.

## Rules
- Work only inside this kit folder (`KIT` = the folder this file is in). Use PowerShell.
- Reuse what run 1 set up: `KIT\venv`, `KIT\data`, `KIT\models`. Install nothing new without asking Daniel.
- Write settings files WITHOUT a byte-order mark:
  `[IO.File]::WriteAllText($path, $json, [Text.UTF8Encoding]::new($false))`.
- Keep the laptop on AC power and awake (as in run 1).
- If a step fails, write down the exact error, try one sensible fix, then move on.
- One short line to Daniel per step.
- **Privacy (results are published):** never record the computer name, the Windows user name, IP or network
  details, serial numbers / service tag, or product keys. Write paths as `KIT\...`, not `C:\Users\...`. Hardware
  model, Windows version and driver version are fine.

## Steps

### 1. New source (dev branch, from the public repo)
```powershell
cd $KIT
Invoke-WebRequest "https://github.com/danielmevit/eqho/archive/refs/heads/dev.zip" -OutFile "$KIT\eqho-dev.zip"
Expand-Archive "$KIT\eqho-dev.zip" "$KIT\src2" -Force
$SRC = (Get-ChildItem "$KIT\src2" -Directory | Select-Object -First 1).FullName
& "$KIT\venv\Scripts\python.exe" -m pip install -r "$SRC\requirements-lock.txt"
& "$KIT\venv\Scripts\python.exe" -m pip install --no-deps --force-reinstall (Get-ChildItem "$SRC\packaging\windows\wheels\*.whl").FullName
cd $SRC; & "$KIT\venv\Scripts\python.exe" run.py --smoke
```
Record the commit (GitHub stores it as the zip comment):
`& "$KIT\venv\Scripts\python.exe" -c "import zipfile,sys; print(zipfile.ZipFile(sys.argv[1]).comment.decode())" "$KIT\eqho-dev.zip"`
(Windows PowerShell 5.1's ZipArchive has no Comment property).
Smoke must print `"ok": true`.

### 2. What does Eqho pick on this laptop?
```powershell
cd $SRC
& "$KIT\venv\Scripts\python.exe" -c "import logging; logging.basicConfig(level=logging.INFO); from src.model_host import resolve_backend; from src.modelpick import hardware_class, recommend_model; print('engine:', resolve_backend('auto'), '| hardware:', hardware_class(), '| model en:', recommend_model('en'), '| model de:', recommend_model('de'))"
```
Expected: `engine: faster-whisper | hardware: cpu | model en: distil-small.en | model de: small`.

### 3. Real dictation test (needs Daniel's OK — about 6 minutes, hands off the keyboard and mouse)
The test opens its own small test window, plays recorded clips into Eqho in place of the microphone, presses Eqho's
test hotkey (Ctrl+Shift+F9) to start and stop, and checks the text Eqho types into that window. Ask Daniel first;
if he says no, skip this step. Two runs: the OLD default model (the slow case that failed in run 1) and the NEW one.
```powershell
foreach ($m in "distil-large-v3","distil-small.en") {
  New-Item -ItemType Directory -Force "$KIT\cfg-e2e-$m" | Out-Null
  $json = @{ model_size=$m; language="en"; engine_backend="auto"; model_dir="$KIT\models" } | ConvertTo-Json
  [IO.File]::WriteAllText("$KIT\cfg-e2e-$m\settings.json", $json, [Text.UTF8Encoding]::new($false))
  $env:EQHO_CONFIG_DIR = "$KIT\cfg-e2e-$m"
  cd $SRC
  & "$KIT\venv\Scripts\python.exe" tools\test_e2e.py --data "$KIT\data\LibriSpeech\test-clean" --clips 3 --model $m `
      --gate-wer 0.25 --work "$KIT\runs2\e2e-$m" 2>&1 | Tee-Object "$KIT\runs2\e2e-$m.log"
}
Remove-Item Env:EQHO_CONFIG_DIR
```
(Eqho downloads Distil Small EN itself the first time, about 330 MB — the test waits up to 5 minutes for it.)
Expected: 3/3 typed in both runs; stop → text roughly 15–20 s with distil-large-v3, a few seconds with
distil-small.en. In each run's `eqho-config\eqho.log` look for: "Engine auto", "Live preview off", "Complete transcript
… transcribed in …s", "Injected N chars", and — in the distil-large-v3 run — one "slow" tip line suggesting Distil
Small EN.

### 4. Small models on the processor (about 30–45 minutes; downloads a few hundred MB per model the first time)
```powershell
New-Item -ItemType Directory -Force "$KIT\cfg-cpu" | Out-Null
$json = @{ model_size="distil-small.en"; language="en"; engine_backend="faster-whisper"; model_dir="$KIT\models" } | ConvertTo-Json
[IO.File]::WriteAllText("$KIT\cfg-cpu\settings.json", $json, [Text.UTF8Encoding]::new($false))
$env:EQHO_CONFIG_DIR = "$KIT\cfg-cpu"
cd $SRC
foreach ($m in "distil-small.en","small","base") {
  foreach ($s in "clean","other") {
    & "$KIT\venv\Scripts\python.exe" tools\bench_asr.py --data "$KIT\data\LibriSpeech\test-$s" --limit 60 `
        --model $m --allow-download --work "$KIT\runs2\cpu-$m-$s" 2>&1 | Tee-Object "$KIT\runs2\cpu-$m-$s.log"
  }
}
Remove-Item Env:EQHO_CONFIG_DIR
```

### 5. Screens on this laptop's display (optional, needs Daniel's OK — about 1 minute, a window appears)
```powershell
cd $SRC; & "$KIT\venv\Scripts\python.exe" tools\test_ui.py --work "$KIT\runs2\ui"
```
It must end with "ALL CHECKS PASSED" (it also fails if any text is cut off). Look at `ui-models.png` and the
`ui-setup-*.png` pages and say if anything looks wrong at this screen size.

### 6. RESULTS-2.md (in KIT)
- Commit tested, smoke result, step 2 output.
- Step 3: per run and clip — typed yes/no, stop → text seconds, WER; the eqho.log lines above.
- Step 4: one table, model × set → WER, × realtime, median seconds per clip, average for short clips (< 4 s); add run
  1's distil-large-v3 faster-whisper numbers (2.48 % / 4.52 %, 0.48×, 14–19 s per clip) as the last row, and the main
  PC's numbers for comparison if you like: distil-small.en 3.82 % / 6.18 % at ~2.5 s, small 3.68 % / 5.99 % at ~3 s,
  base 4.95 % / 8.48 % at ~1 s.
- Step 5 (if run): result + anything odd on screen.
- Your view: is Distil Small EN a good default for a laptop like this (good accuracy, a short sentence typed within a
  few seconds)?

When done, tell Daniel: "Finished — RESULTS-2.md is in the kit folder." If Daniel wants, commit RESULTS-2.md and the
`runs2` folder (logs, CSVs and screenshots only — no audio, no models) to a new branch `bench/intel-uhd620-run2` on
github.com/danielmevit/eqho and push it.
