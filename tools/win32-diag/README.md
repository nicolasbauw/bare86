# Outils de diagnostic des démos Win32 sous HX (2026-11-25)

Écrits pendant la session du 2026-11-25 ; scripts jetables, mais ils ont servi à établir les causes.

- `winprobe.py` : lance chaque élément (zip ou dossier) dans un dossier temporaire avec un clavier scripté, 6 en parallèle, et note les messages de HX
  (`DPMILD32:`, `dkrnl32:`, exceptions), la luminosité de l'écran final et si la limite de temps a été atteinte. Variables : `BARE` (binaire), `PACK` (DLL à copier),
  `KEYS` (clavier). Arguments : `SCRATCH LISTE.txt SORTIE.jsonl`. Piège : sans touche l'élément reste en attente ; une touche peut le faire quitter (et planter à la décharge d'une DLL).
- `win_imports.py` : DLL importées par l'exécutable de chaque élément, par groupe de résultats (lit `results.jsonl` et le diagnostic).
- `hx_rebase.py` : rebase des DLL de HX (relocations appliquées, bases 0x60000000 + 4 Mo × n, signature `PX` de DKRNL32 comprise), pour libérer 0x10000000 avec `DPMILDR=1024`.
- `dpmi_alloc_test.c`, `loadlib_test.c` : petits programmes Win32 (clang `--target=i686-pc-windows-msvc`, sans bibliothèque C) : `INT 31h AX=0504h` à une adresse donnée ; `LoadLibraryExA` et adresse obtenue.
- `dpmi_client32.S` : client DPMI 32 bits en `.COM` (GNU as) : même appel `0504h` depuis un segment 16 bits, comme le chargeur de HX. À lancer avec `--dpmi`.
