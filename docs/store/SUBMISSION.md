# Microsoft Store Submission Guide — Eqho

Paste-ready reference for every Microsoft Partner Center field, the Partner Center identity values you must set in your shell, and how to build, test, and upload Eqho's `.msix` package.

> **Owner rule:** Do **not** include any GitHub repository URLs anywhere in the Microsoft Store listing or Store UI.

The short version of what Eqho needs to run: **a 64-bit Windows 10 (1809+) or Windows 11 desktop, a microphone, and disk space for your chosen offline Whisper speech model (60 MB to 1.5 GB).** Eqho is a native Rust + Slint 1.19 + `whisper.cpp 1.8.4` desktop package with static CRT — no Python, no .NET, no WebView2, no VC++ Redistributable, no CUDA Toolkit, and no account required.

---

## 1. Product Reservation
| Field | Value |
|-------|-------|
| Product type | **MSIX or PWA app** (so the Store signs it — **not** "EXE or MSI app") |
| Name | Reserve **Eqho** (or **Eqho - Offline Voice Dictation** if desired). `EQHO_STORE_DISPLAY_NAME` must match the reserved product name the listing uses, **exactly**. |

---

## 2. Where to Copy & Paste Partner Center Identity Values
In **Partner Center → your product → Product management → Product identity**, copy these values into your PowerShell session before running `packaging\windows\build_msix.ps1 -Store`:

| Environment variable | Partner Center source | Where stamped |
|---|---|---|
| `EQHO_STORE_ID` | Store ID (e.g. `9XXXXXXXXXXX`) — **must NOT be placeholder `9NBLGGH4XXXX`** | Compiled into `Eqho.exe` (`eqho-app/src/license.rs` via `option_env!("EQHO_STORE_ID")`) |
| `EQHO_STORE_IDENTITY_NAME` | Package/Identity/Name (e.g. `12345DanielMevit.Eqho`) | `<Identity Name="...">` (`__IDENTITY_NAME__`) |
| `EQHO_STORE_PUBLISHER` | Package/Identity/Publisher (e.g. `CN=A1B2C3D4-...`) | `<Identity Publisher="...">` (`__PUBLISHER__`) |
| `EQHO_STORE_PUBLISHER_DISPLAY` | Package/Properties/PublisherDisplayName (e.g. `damt.xyz`) | `<PublisherDisplayName>` (`__PUBLISHER_DISPLAY_NAME__`) |
| `EQHO_STORE_DISPLAY_NAME` | Reserved Store product name (defaults to `Eqho` if unset) | `<Properties><DisplayName>` (`__DISPLAY_NAME__`) |

Do **not** commit Partner Center identity strings, Store IDs, or signing certificates into the repository; `packaging/windows/AppxManifest.xml` stays a template with `__...__` placeholders.

---

## 3. Properties Page
| Field | Value |
|-------|-------|
| Category | **Productivity** |
| Subcategory | (leave blank) |
| Secondary category | **Utilities & tools** (optional) |
| Privacy policy URL | `https://danielmevit.github.io/eqho/privacy/` |
| Website | `https://danielmevit.github.io/eqho/` |
| Support contact info | Use a support email address or `https://damt.xyz` — **do not** use a GitHub repository URL |

**Product declarations (checkboxes):**
- Uses the Microsoft commerce platform for purchases — **Yes** (paid base price + 15-day free trial via `Windows.Services.Store.StoreContext::RequestPurchaseAsync`)
- Depends on non-Microsoft drivers or NT services — **No**
- Accesses, collects, or transmits personal information — **No**. *Privacy note:* All speech recognition runs 100% locally on the user's PC via `whisper.cpp`. Audio is never saved to disk or uploaded. The only network activity is (1) user-initiated download of open Whisper GGML speech model weights from Hugging Face over HTTPS, and (2) Windows `StoreContext` licence checks through the Microsoft Store.
- Is a screen reader / accessibility tool — **No**
- Uses a Bluetooth/USB peripheral — **No** (uses standard Windows audio input / WASAPI microphones)

---

## 4. Age Ratings (IARC Questionnaire)
Answer **No** to all questions (no violence, no user-to-user sharing, no data collection, no gambling or controlled substances). Result: **Everyone / 3+** in all regions.

---

## 5. Pricing and Availability
| Field | Value |
|-------|-------|
| Base price | **OWNER DECISION** — choose a paid one-time price tier in Partner Center. Do **not** set Free: the listing promises a 15-day free trial followed by a one-time purchase, and the app includes an in-app **Buy Eqho** button (`StoreContext::RequestPurchaseAsync`). |
| Free trial | **15 days** |
| Markets | **All markets** |
| Visibility | **Public** |
| Discoverability | Available in Store, discoverable |
| Release schedule | **As soon as it passes certification** |
| Device families | **Windows 10/11 Desktop** only |

Reference: <https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/price-and-availability>

---

## 6. System Requirements
| Requirement | Minimum | Recommended |
|---|---|---|
| **OS** | Windows 10 version 1809 (build **17763**) | Windows 11 (64-bit) |
| **Architecture** | **x64 (64-bit)** | x64 |
| **Processor** | Any 64-bit x86-64 CPU | Modern 4-core+ CPU |
| **Memory (RAM)** | **4 GB** | 8 GB |
| **Free disk space** | **200 MB** (app + Base Q5 model) | 2 GB (for larger multilingual models) |
| **Graphics** | Any Windows display (software-rendered Slint UI; CPU fallback built in) | Vulkan-capable NVIDIA, AMD, or Intel GPU |
| **Audio** | Any Windows microphone (WASAPI) | Headset or USB microphone |
| **Internet** | Required once to download a speech model; **100% offline thereafter** | — |
| **Runtime prerequisites** | **None** — static CRT `Eqho.exe` + bundled `whisper.cpp` DLLs | None |

---

## 7. What the Package Declares (`packaging/windows/AppxManifest.xml`)
| Manifest field | Value | Meaning |
|---|---|---|
| Identity Name | `__IDENTITY_NAME__` (stamped at build) | Partner Center package identity |
| Publisher | `__PUBLISHER__` (stamped at build) | Partner Center publisher GUID |
| Version | `<major>.<minor>.<patch>.0` (stamped from `rust/Cargo.toml`) | Four-part MSIX version (`0.9.0.0`) |
| ProcessorArchitecture | `x64` | 64-bit Windows desktop |
| Min OS / Max tested | `10.0.17763.0` / `10.0.26100.0` | Windows 10 1809 through Windows 11 24H2 |
| Entry point | `Windows.FullTrustApplication` (`Eqho.exe`) | Packaged Win32 desktop application |
| Capabilities | `rescap:Capability Name="runFullTrust"`, `DeviceCapability Name="microphone"` | Win32 desktop execution + user-initiated microphone capture |
| Startup task | `desktop:StartupTask` (`TaskId="EqhoStartup"`, `Enabled="false"`, `DisplayName="Eqho"`) | Managed via `Windows.ApplicationModel.StartupTask` when packaged |

---

## 8. Store Listing Copy
`docs/store/LISTING.md` is the **single source of truth** for the Store listing text (description, short description, features, What's new, keywords, copyright, privacy summary, screenshots plan, and certification notes).
- **Owner rule:** zero GitHub repository URLs anywhere in the listing.

---

## 9. Submission Options — `runFullTrust` Justification (REQUIRED)
Because Eqho declares `rescap:Capability Name="runFullTrust"`, Partner Center requires a written justification on the **Submission Options** page. Paste this:

> Eqho is a full-trust Win32 desktop application (written in Rust using the Slint UI toolkit, packaged as MSIX) for offline voice-to-text dictation. It declares `runFullTrust` because packaged Win32 executables require it to run outside the UWP AppContainer sandbox and provide system-wide dictation: Eqho lives in the Windows system tray, registers a user-configurable global keyboard shortcut (`WH_KEYBOARD_LL`) so users can start and stop dictation from any desktop app, renders a non-activating floating listening pill (`WS_EX_NOACTIVATE`), inserts transcribed text into the user's focused window via private clipboard paste or Unicode `SendInput`, optionally ducks system audio via WASAPI during dictation, and spawns an isolated child process (`Eqho.exe --model-host`) running `whisper.cpp` locally on Vulkan GPU or CPU so model switches cleanly release memory. Eqho declares the `microphone` capability solely to capture voice audio while the user is actively dictating — audio is never saved to disk or transmitted over the network. Eqho installs no drivers or NT services, collects no personal data or telemetry, and makes network requests only when the user explicitly downloads an offline Whisper GGML speech model (over HTTPS from Hugging Face) or when Windows checks the Store licence via `StoreContext`.

---

## 10. Notes for Certification (Reviewer Instructions)
Paste this into **Notes for certification**:

> Eqho is a local offline voice-to-text dictation app that runs in the Windows system tray.
>
> 1. On first launch, a short 6-step setup wizard opens: choose a dictation language, verify microphone input, choose a local speech model to download (e.g., Base Q5 is ~60 MB and downloads quickly over HTTPS), optionally choose whether to mute other audio while speaking, and test dictation directly in Step 6.
> 2. After setup, Eqho lives in the system tray (left-click the Eqho icon to open the Dashboard; right-click for Start/Stop Dictation, Open Dashboard, and Quit).
> 3. To dictate into any application (such as Notepad), focus a text field, press **Alt+Q**, speak, and press **Alt+Q** again — transcribed text appears at the cursor. Hovering over the floating pill reveals a quick control bar (silent cancel, timer, 99-language menu, Batch/Live toggle, pin, and Done).
> 4. No account or sign-in is required. All transcription runs locally on the device (Vulkan GPU or multi-core CPU).
> 5. Eqho includes a 15-day free trial managed through the Microsoft Store (`StoreContext`). During the trial, the bottom bar shows days remaining and a "Buy Eqho" button; after trial expiry, dictation pauses quietly and a banner offers "Buy Eqho".

---

## 11. Verifying Trial UI States Before Submission
Build with `--features store` and launch isolated demo sessions (each uses a throwaway temp config dir, never touches `%APPDATA%\Eqho`, never calls Store APIs, and never writes `license.json`):

```powershell
.\Eqho.exe --demo-dashboard --demo-license=trial
.\Eqho.exe --demo-dashboard --demo-license=trial-last-day
.\Eqho.exe --demo-dashboard --demo-license=trial-ended
.\Eqho.exe --demo-dashboard --demo-license=full
.\Eqho.exe --demo-dashboard --demo-license=unknown
```

---

## 12. Build & Upload Steps (Windows)
1. Ensure Windows 10/11 SDK (`makeappx.exe`, `makepri.exe`, `signtool.exe`), Rust 1.95+, and the clean-PC `whisper.cpp 1.8.4` DLL folder (`13` static-CRT `/MT` DLLs) are present.
2. Verify committed MSIX logo assets match `generate_assets.py`:
   ```bash
   python3 packaging/windows/generate_assets.py --check
   ```
3. Set all Partner Center identity environment variables in PowerShell:
   ```powershell
   $env:EQHO_STORE_ID                = "<Partner Center Store ID, e.g. 9XXXXXXXXXXX>"
   $env:EQHO_STORE_IDENTITY_NAME     = "<Partner Center Package/Identity/Name>"
   $env:EQHO_STORE_PUBLISHER         = "<Partner Center Package/Identity/Publisher, e.g. CN=...>"
   $env:EQHO_STORE_PUBLISHER_DISPLAY = "<Partner Center PublisherDisplayName>"
   $env:EQHO_STORE_DISPLAY_NAME      = "Eqho"
   ```
4. Build the unsigned Partner Center `.msix` (**do not** pass `-SkipBuild` so `EQHO_STORE_ID` is compiled into `Eqho.exe`):
   ```powershell
   powershell -ExecutionPolicy Bypass -File packaging\windows\build_msix.ps1 `
     -Store `
     -DllDir "D:\Vibe Coding\_vm\eqho\rust-09\dist\whisper-1.8.4-clean-pc-dlls"
   ```
   *(If cross-compiling `Eqho.exe` from WSL with `cargo xwin`, export `EQHO_STORE_ID="<RealStoreId>"` in the WSL build command first, then pass `-ExePath <built-store-Eqho.exe> -SkipBuild` to `build_msix.ps1 -Store`.)*
5. Upload `dist\store\Eqho-0.9.0-store.msix` (unsigned — Microsoft signs Store packages during certification) on the **Packages** tab in Partner Center.
6. Optional local sideload & Windows App Certification Kit (WACK) check before upload (keep test `.pfx` outside the repo):
   ```powershell
   powershell -ExecutionPolicy Bypass -File packaging\windows\build_msix.ps1 `
     -SignForSideload C:\temp\eqho-test.pfx `
     -DllDir "D:\Vibe Coding\_vm\eqho\rust-09\dist\whisper-1.8.4-clean-pc-dlls"
   Add-AppxPackage dist\store\Eqho-0.9.0-sideload.msix
   ```
