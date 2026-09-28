# html2wp Desktop – build pre Windows x64

Tento ZIP obsahuje zdrojový projekt aplikácie, pripnutý html2wp plugin, lock súbory, ikony, licencie a návod. Výsledkom buildu bude NSIS inštalátor `*-setup.exe` pre Windows x64.

## 1. Nainštaluj nástroje

Build spúšťaj priamo vo Windows, napríklad Windows 11 x64, v PowerShelli alebo Command Prompte.

- **Node.js 22 + npm** – rovnaká hlavná verzia, s ktorou sa projekt zostavoval na macOS. Použi inštalátor z [Node.js](https://nodejs.org/en/download).
- **Visual Studio Build Tools 2022** – pri inštalácii vyber **Desktop development with C++**, vrátane MSVC x64/x86 a Windows SDK.
- **Rust cez rustup** – vyber MSVC toolchain pre `x86_64-pc-windows-msvc`. Použi aktuálny stable; projekt bol zostavovaný s Rust 1.90. [Inštalácia Rustu](https://www.rust-lang.org/tools/install).
- **Microsoft Edge WebView2 Runtime** – ak ho ešte nemáš, nainštaluj Evergreen Runtime.

Visual C++ a WebView2 postup aj odkazy na inštalátory sú v [oficiálnom návode Tauri pre Windows](https://v2.tauri.app/start/prerequisites/#windows). Použitý formát inštalátora opisuje [Tauri Windows Installer](https://v2.tauri.app/distribute/windows-installer/).

Po inštalácii otvor nový terminál. Over:

```powershell
node --version
npm.cmd --version
rustc --version
cargo --version
rustup show
```

Pre tento postup má byť aktívny Rust MSVC toolchain. Ak máš inú konfiguráciu, v rozbalenom adresári môžeš nastaviť toolchain iba pre tento projekt:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup override set stable-x86_64-pc-windows-msvc
```

## 2. Rozbaľ ZIP a spusti build

Rozbaľ celý ZIP napríklad do `C:\dev`. Otvor adresár, v ktorom je `package.json` a `build-windows.cmd`:

```powershell
cd C:\dev\html2wp-windows-source-1.0.18
.\build-windows.cmd
```

Skript pripraví Rust target, spustí `npm ci` a vytvorí inštalátor. Bez `TAURI_SIGNING_PRIVATE_KEY` vytvorí lokálny inštalátor bez aktualizačného podpisu; GitHub Actions podpisuje vydávací build. Pri chybe sa zastaví. Prvý build potrebuje internet na stiahnutie závislostí a nástrojov a môže trvať niekoľko minút.

Rovnaký postup ručne:

```powershell
rustup target add x86_64-pc-windows-msvc
npm.cmd ci
npm.cmd run bundle:windows:local
```

`bundle:windows:local` zostaví frontend a spustí Tauri pre `x86_64-pc-windows-msvc` s NSIS a uzamknutými Cargo závislosťami. `bundle:windows` navyše vytvorí podpísaný updater artifact, ak je nastavený vydávací podpisový kľúč. Plugin sa pri štarte aplikácie sťahuje z GitHubu; do inštalátora sa nepribaľuje.

## 3. Kde nájdeš výsledok

Inštalátor:

```text
src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis\*-setup.exe
```

Otvor jeho adresár:

```powershell
explorer .\src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis
```

Na inštaláciu používaj výsledný setup EXE. Docker runtime image nie je v EXE; aplikácia ho podľa pripnutého digestu stiahne z Docker Hubu. Tento postup nepridáva produkčný podpis certifikátom.

## 4. Spustenie a konverzia

Samotný build zdrojov nepotrebuje Docker, Python ani prihlásenie do Codexu. `runtime/runtime-release.json` obsahuje samostatné nemenné Linux image digesty pre ARM64 a x86_64. Windows x64 aplikácia vyberie x86_64. Zdrojový ZIP ani inštalátor neobsahujú runtime image.

Tlačidlo **Prepare environment** stiahne alebo spustí Docker Desktop a pripraví kontajnery. Docker musí používať lokálny Linux x64 engine. Používateľ potvrdí systémové oprávnenia a podmienky Dockeru. Windows môže vyžiadať WSL alebo reštart. Aplikácia pred konverziou overuje, že natívna cesta projektu a cesta Linux Docker daemonu ukazujú na tie isté súbory aj vo vnorenom build kontajneri. Čistá Windows inštalácia vyžaduje ešte manuálny smoke test; pozri [stav automatickej prípravy](docs/automatic-setup.md).

V aplikácii otvor **Settings → Check environment → Prepare environment → Connect with ChatGPT**. Následne vyber model a nastav Free alebo svoju html2wp licenciu.

## Riešenie bežných chýb

- **`link.exe not found` alebo chyba Windows SDK:** doplň C++ workload v Visual Studio Installer. Otvor nový terminál; prípadne použi **x64 Native Tools Command Prompt for VS 2022**.
- **`npm.ps1 cannot be loaded`:** použi `npm.cmd` alebo priložený `build-windows.cmd`.
- **Chýba Rust target:** spusti `rustup target add x86_64-pc-windows-msvc`.
- **Chýbajú súbory pluginu:** rozbaľ celý ZIP. Plugin html2wp si aplikácia stiahne z GitHubu pri prvom spustení (potrebuje internet).
- **Chyba sťahovania závislostí alebo NSIS:** skontroluj internet/proxy a zopakuj build. Chyba je vypísaná v termináli.

## Git a aktualizácie

ZIP je snapshot zdrojov bez histórie `.git`. Obsahuje `.gitmodules`, GitHub Actions a zdroje pripnutých submodulov. Na build zo ZIPu nemusíš inicializovať Git ani sťahovať plugin zvlášť.

Pre ďalší vývoj klonuj [zdrojový repozitár](https://github.com/iOSDevSK/html2wp-app) s `--recurse-submodules` a vyber vetvu `windows`. Workflow `Windows x64` na tejto vetve zostaví a otestuje natívny Windows NSIS inštalátor vrátane Tauri updater podpisu. Samotný podpis updatera nezaručuje podpis Windows Authenticode ani odstránenie SmartScreen upozornenia.

Windows Preview používa samostatný aktualizačný feed `windows-x86_64.json` s podpísanými inštalátormi na vetve `updates` v repozitári binárnych vydaní. Stabilný macOS feed tým zostáva nedotknutý.

Pôvodná licencia pluginu je vo `vendor/html2wp/LICENSE`; licencie závislostí sú v `notices/`. Podrobnosti o ich zbere sú v `docs/desktop-licensing.md`. `SOURCE-MANIFEST.json` v ZIPe obsahuje SHA-256 každého pribaleného súboru.

**Stav overenia:** výstup Windows CI dokazuje kompiláciu a podpis aktualizačného balíka. Funkčný beh Docker Desktop, prihlásenie, konverzia, preview a inštalácia aktualizácie vyžadujú samostatný test na Windows počítači; CI bez Docker Desktop ich neoverí.
