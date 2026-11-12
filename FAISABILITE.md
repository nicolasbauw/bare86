# Étude de faisabilité — émulateur DOS sans émulation x86

*Rédigée le 2026-11-09. Écrite de mémoire, sans recherche web : les détails techniques et les liens sont à vérifier.*

Objectif : un programme dans l'esprit de DOSBox, mais **sans émulation du CPU x86**. Le code DOS s'exécute directement sur le CPU hôte (x86-64 moderne uniquement). Seuls le BIOS, DOS et le matériel PC sont fournis en logiciel.

Cibles matérielles : graphismes **VGA / SVGA** (éventuellement **3dfx Voodoo** dans un futur lointain), son **Sound Blaster 16** (+ OPL3).

---

## Verdict

**Faisable, à une condition : passer par la virtualisation matérielle (KVM sous Linux, WHPX sous Windows), pas par une exécution directe en espace utilisateur.** Le code DOS 16 bits tourne alors nativement sur le CPU hôte, en mode réel, dans une VM minimale. Tout le reste (BIOS, DOS, VGA/SVGA, SB16, PIC, PIT, clavier) est fourni en logiciel par le programme hôte.

L'approche n'est pas théorique : **dosemu2** propose déjà un mode KVM qui fonctionne ainsi. Elle supprime l'émulateur CPU, mais pas le reste : chez DOSBox, le cœur CPU ne représente qu'une part minoritaire du code ; l'essentiel du travail (DOS, vidéo, son, périphériques) reste à faire.

Trois points durs conditionnent le succès :

- **La vitesse** : le code tourne à plusieurs milliards d'instructions par seconde. Beaucoup de jeux deviennent injouables ou plantent (boucles de calibrage). Il faut un mécanisme de bridage fin.
- **Les modes VGA planaires** (EGA, mode X) : chaque accès à la mémoire vidéo doit être intercepté, soit une sortie de VM par accès. C'est le principal risque de performance, à mesurer dès le prototype.
- **La portabilité** : uniquement des hôtes x86-64 avec VT-x / AMD-V accessibles (Linux, Windows ; macOS Intel en option). Pas d'Apple Silicon, ce qui est cohérent avec la cible.

---

## Q1 — Utiliser le CPU hôte sans émulation x86

Oui, mais **seulement via la virtualisation matérielle**. Les CPU modernes savent toujours exécuter du code 8086, mais pas depuis un processus 64 bits ordinaire : en mode long, le mode réel et le mode virtuel 8086 ne sont plus accessibles.

| Option | Principe | Verdict |
| --- | --- | --- |
| Mode virtuel 8086 (vm86) | Le CPU exécute du code 8086 sous contrôle de l'OS, comme NTVDM sous Windows 32 bits | Impossible : n'existe pas en mode long, le noyau Linux 64 bits ne fournit pas `vm86()` |
| Segments 16 bits via la LDT (`modify_ldt`) | Exécuter le code en mode compatibilité avec des segments 16 bits, comme Wine pour Win16 | Non viable : le mode réel calcule les adresses en segment × 16, chaque `mov ds, ax` déclencherait une faute à émuler ; `in`/`out`, `cli`/`sti`, `int` aussi |
| Virtualisation matérielle (KVM, WHPX, Hypervisor.framework) | Une VM minimale démarre en mode réel ; VT-x « unrestricted guest » (Intel depuis 2010) et AMD-V l'exécutent nativement | **Retenue** : code natif ; mode protégé et extendeurs DOS fonctionnent aussi, car l'invité a tous les privilèges |

Le programme devient un petit hyperviseur (VMM) : il alloue 1 Mo + mémoire étendue, place le CPU virtuel en mode réel, puis boucle sur `KVM_RUN`. Chaque instruction sensible (port d'E/S, accès à une zone non mappée) provoque une sortie de VM que l'hôte traite en logiciel.

**Appels BIOS et DOS** : la table des vecteurs (IVT) pointe vers de petits stubs en ROM virtuelle, du type `out 0xE0, al ; iret`. L'`out` provoque une sortie vers l'hôte, qui lit les registres du CPU virtuel, exécute le service (INT 21h, INT 10h…) et reprend l'exécution. Un programme qui détourne un vecteur (TSR, jeu qui installe son handler clavier) continue de fonctionner, puisque l'IVT reste une vraie IVT en mémoire invitée.

**Prérequis hôte** : CPU x86-64 avec VT-x ou AMD-V activé dans le firmware, accès à `/dev/kvm`. Dans une VM (cloud, WSL2), il faut la virtualisation imbriquée.

---

## Q2 — Documentation BIOS, DOS et matériel

Oui, c'est très bien documenté, y compris une bonne partie du comportement non officiel. La difficulté n'est pas de trouver la spécification, mais de reproduire les comportements implicites sur lesquels comptent les programmes.

| Domaine | Références principales | Remarque |
| --- | --- | --- |
| Interruptions BIOS et DOS | Ralf Brown's Interrupt List (RBIL) | La référence exhaustive : INT 10h, 13h, 15h, 16h, 1Ah, 21h, 2Fh, 33h, 67h… |
| Structures internes de DOS | *Undocumented DOS* (Schulman), sources MS-DOS 1.25 / 2.0 / 4.0 publiées par Microsoft sous licence MIT | List of Lists, SFT, CDS, MCB : certains programmes les lisent directement |
| Implémentations libres | FreeDOS (GPL), DOSBox / DOSBox-X / DOSBox Staging (GPL v2), dosemu2 (GPL) | Reprendre leur code impose leur licence : à décider tôt |
| VGA / SVGA | FreeVGA, spécification VESA VBE 2.0 / 3.0 | Registres séquenceur, CRTC, GC, attributs, DAC ; VBE pour les modes SVGA et le framebuffer linéaire |
| Sound Blaster 16 | Creative « Sound Blaster Series Hardware Programming Guide », datasheets DSP | DSP, mixeur, DMA 8 et 16 bits ; OPL3 pour la musique FM |
| Contrôleurs PC | Datasheets 8259A (PIC), 8253/8254 (PIT), 8237 (DMA), 8042 (clavier), MC146818 (RTC) ; wiki OSDev | Bien couverts |
| 3dfx Voodoo | Spécifications Glide et registres Voodoo publiées ; implémentations MAME, 86Box, DOSBox-X | Faisable plus tard, voir Q3 |

Autre option à garder en tête : exécuter un **vrai noyau DOS (FreeDOS)** dans la VM. Il ne resterait que le BIOS et le matériel à fournir, avec une meilleure compatibilité, au prix d'une intégration moins directe au système de fichiers hôte (il faut alors un redirecteur INT 2Fh, comme dosemu2). Choix retenu : voir la section [Choix du DOS](#choix-du-dos).

---

## Q3 — Support logiciel du matériel

Avec la virtualisation, **ni le mode réel ni les interruptions ne bloquent** : le CPU virtuel gère le mode réel lui-même, et l'hôte injecte les IRQ dans l'invité. Ce sont exactement les points qui rendent l'approche impossible sans virtualisation. Le coût se déplace vers les périphériques, à émuler comme dans DOSBox.

| Élément | Comment le fournir | Coût / difficulté |
| --- | --- | --- |
| Mode réel, A20, mode protégé | Natifs dans la VM ; A20 via le port 92h et le 8042, en jouant sur le mappage mémoire | Faible |
| Interruptions (PIC 8259) | PIC intégré à KVM (`KVM_CREATE_IRQCHIP`) ou PIC émulé en espace utilisateur + `KVM_INTERRUPT` | Faible |
| Timer (PIT 8254), RTC | PIT intégré à KVM (`KVM_CREATE_PIT2`) ou émulé ; les jeux le reprogramment parfois à plusieurs kHz | Faible à moyen |
| Clavier, souris | 8042 + scancodes depuis SDL ; INT 33h en HLE pour la souris | Moyen |
| Texte, CGA, mode 13h | Mémoire vidéo mappée comme RAM normale, lue à chaque image par l'hôte : zéro sortie de VM | Faible |
| VGA planaire (EGA 16 couleurs, mode X) | Zone A0000–AFFFF non mappée : chaque lecture/écriture sort de la VM et passe par une émulation des plans, latches et ALU du VGA | **Élevé** : 1 à 5 µs par accès, potentiellement des dizaines de ms par image |
| SVGA (VESA VBE) | VBE 2.0 en HLE (INT 10h AX=4Fxx), framebuffer linéaire mappé comme RAM + suivi des pages modifiées (`KVM_GET_DIRTY_LOG`) ; fenêtrage banké par remappage de slot mémoire | Moyen, bonnes performances |
| Sound Blaster 16 | DSP (ports 22xh), mixeur, DMA 8237 8 et 16 bits, IRQ 5/7, OPL3 émulé (Nuked-OPL3 ou ymfm), sortie audio SDL | Moyen à élevé : timing audio stable malgré l'exécution native |
| 3dfx Voodoo (futur lointain) | Émulation des registres Voodoo en MMIO (lourd), ou interception de Glide côté DOS traduite vers OpenGL/Vulkan (wrapper type OpenGlide) | Élevé ; le wrapper Glide est l'option réaliste |
| EMS, XMS, UMB | En HLE ; la fenêtre EMS se remappe par slot mémoire KVM, donc sans copie | Moyen |
| DPMI | Les extendeurs (DOS/4GW, DOS32A, PMODE/W) peuvent basculer seuls en mode protégé ; un hôte DPMI 0.9 en HLE ou CWSDPMI couvre DJGPP | Moyen |

Le vrai problème n'est pas le matériel mais **le temps** : sans émulation, on ne compte plus les cycles. Le jeu tourne à la vitesse de l'hôte, et les programmes qui calibrent une boucle sur un tick de 55 ms débordent (erreur 200 de Turbo Pascal, nombreux jeux Sierra). Voir les risques.

---

## Architecture proposée

```
┌──────────────────────────────────────────────────────────────────────┐
│ VM KVM — exécutée nativement par le CPU hôte (VT-x / AMD-V)          │
│  ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────────┐  │
│  │ Programme DOS    │ │ IVT + stubs ROM  │ │ Mémoire invitée      │  │
│  │ réel ou protégé  │ │ out 0xE0,al;iret │ │ RAM, VRAM 13h, LFB   │  │
│  └──────────────────┘ └──────────────────┘ └──────────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
        │ sorties de VM : ports E/S, MMIO, stubs        ▲ IRQ injectées, reprise
        ▼                                               │
┌──────────────────────────────────────────────────────────────────────┐
│ Processus hôte (espace utilisateur)                                  │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │ Boucle vCPU (KVM_RUN) : aiguillage des sorties de VM           │  │
│  └────────────────────────────────────────────────────────────────┘  │
│    │            │             │              │             │         │
│  ┌─▼────────┐ ┌─▼─────────┐ ┌─▼──────────┐ ┌─▼─────────┐ ┌─▼───────┐ │
│  │ BIOS HLE │ │ DOS HLE   │ │ Contrôleurs│ │ VGA/SVGA  │ │ SB16 +  │ │
│  │ INT 10h, │ │ INT 21h,  │ │ PIC, PIT,  │ │ planaire, │ │ OPL3    │ │
│  │ 13h, 16h │ │ PSP, MCB, │ │ RTC, 8042, │ │ DAC,      │ │ DSP,    │ │
│  │ 15h, 1Ah │ │ MZ, XMS   │ │ DMA        │ │ VBE 2.0   │ │ mixeur  │ │
│  └──────────┘ └─────┬─────┘ └─────┬──────┘ └─────┬─────┘ └────┬────┘ │
└─────────────────────┼─────────────┼──────────────┼────────────┼──────┘
                      ▼             ▼              ▼            ▼
             ┌──────────────────┐ ┌────────────────────────────────────┐
             │ Répertoire hôte  │ │ SDL3 : fenêtre, audio, clavier,    │
             │ monté (C:)       │ │ souris                             │
             └──────────────────┘ └────────────────────────────────────┘
```

Le programme DOS tourne dans la VM sans intermédiaire ; il ne repasse par l'hôte que lorsqu'il touche un port d'E/S, une zone mémoire non mappée (VGA planaire) ou un stub d'interruption. Le cœur du projet est donc la boucle vCPU et les modules qu'elle appelle, chacun testable séparément.

---

## Langage : Rust

**Oui, Rust convient très bien, c'est même probablement le meilleur choix aujourd'hui pour ce type de projet.** Les VMM modernes les plus connus sont écrits en Rust (Firecracker d'AWS, Cloud Hypervisor, crosvm de Google), et leurs briques de base sont publiées sous forme de crates réutilisables.

| Besoin | En Rust | Remarque |
| --- | --- | --- |
| API KVM | `kvm-ioctls`, `kvm-bindings` (projet rust-vmm) | Utilisés en production par Firecracker ; couvrent `KVM_RUN`, registres, slots mémoire, IRQ chip, PIT, dirty log, CPUID |
| Mémoire invitée | `vm-memory` (rust-vmm) | Accès sûrs et volatils à une mémoire partagée avec le vCPU |
| Backend Windows (WHPX) | crate `windows` (bindings officiels Microsoft, API `WHv*`) | À encapsuler derrière un trait « backend VM » |
| Backend macOS Intel | Bindings Hypervisor.framework à générer (`bindgen`) | Basse priorité |
| Vidéo, audio, entrées | `sdl3` (bindings SDL3) ; alternative 100 % Rust : `winit` + `pixels`/`wgpu` + `cpal` | SDL3 est le plus simple pour démarrer |
| OPL3 | Nuked-OPL3 (C, LGPL) ou ymfm (C++, BSD) via FFI (`cc`, `cxx`) | Une petite surface d'API, FFI sans difficulté |
| Bridage par compteur d'instructions | `perf_event_open` via `libc` ou crate `perf-event` ; `KVM_RUN` interrompu par signal ou `immediate_exit` | À valider à l'étape 0 |
| Code invité (stubs ROM, petits tests) | NASM appelé depuis `build.rs`, binaire inclus avec `include_bytes!` | Le code 16 bits reste de l'assembleur, ce qui est normal |
| Tests | `cargo test` lançant de petits `.COM` assemblés et vérifiant registres / sortie | Très bon filet de sécurité pour le DOS HLE |

Points d'attention propres à Rust :

- **`unsafe` localisé** : les ioctl KVM, le `mmap` de la mémoire invitée et le FFI imposent un peu d'`unsafe`. Il reste confiné dans la couche VMM ; le reste (DOS HLE, VGA, SB16) est du Rust sûr ordinaire.
- **Mémoire partagée avec le vCPU** : l'invité écrit en mémoire pendant que l'hôte la lit (rendu vidéo, DMA audio). Il ne faut jamais prendre une référence `&[u8]` / `&mut [u8]` classique dessus ; passer par `vm-memory` ou des accès volatils.
- **Concurrence** : vCPU, rendu et audio sont des threads séparés. La boucle vCPU possède l'état des périphériques ; vidéo et audio reçoivent des copies ou des tampons circulaires (`crossbeam`, `ringbuf`), ce qui évite de mettre tout l'état derrière des `Mutex`.
- **Écosystème rétro** : moins de code « émulation PC » réutilisable qu'en C/C++ (où l'on pourrait lire DOSBox ligne à ligne), mais c'est précisément la partie à écrire soi-même ici.

---

## Entrées / sorties hôte : SDL3

**SDL3 couvre vidéo, son, clavier, souris et manettes ; son surcoût est négligeable.** Les goulots du projet sont ailleurs : sorties de VM, VGA planaire, bridage.

| Domaine | Rôle de SDL3 | Volume | Surcoût |
| --- | --- | --- | --- |
| Vidéo | Une texture envoyée au GPU par image ; mise à l'échelle (et shaders CRT éventuels) via Vulkan / D3D / Metal | 320×200 à 70 Hz ≈ 18 Mo/s ; 1024×768 32 bits à 70 Hz ≈ 220 Mo/s | Une copie et un appel GPU par image |
| Son | `SDL_AudioStream`, rééchantillonnage, sortie PipeWire / WASAPI / CoreAudio | 48 kHz stéréo 16 bits = 192 Ko/s | Microscopique ; latence fixée par le tampon (10–20 ms visés) |
| Clavier, souris | Scancodes physiques convertis en scancodes PC set 1 pour le 8042 | Quelques événements/s | < 1 ms au-dessus de l'OS |
| Manette, joystick | `SDL_Gamepad` / `SDL_Joystick` | Négligeable | Négligeable ; la difficulté est l'émulation du port analogique 201h (chronométrage), côté hôte |

**Organisation des threads :**

- **Thread vCPU** : `KVM_RUN` + périphériques émulés ; n'appelle jamais SDL et ne l'attend jamais.
- **Thread principal** : événements SDL et rendu (exigé par SDL pour la fenêtre) ; lit la mémoire vidéo invitée ou un tampon préparé par le vCPU.
- **Thread audio** (géré par SDL) : alimenté par un tampon circulaire sans verrou rempli par SB16 / OPL3.
- **Entrées** : file sans verrou vers le vCPU, puis IRQ 1 (clavier), IRQ 12 ou INT 33h (souris).

Points de fluidité indépendants de SDL : afficher du 70 Hz sur un écran 60 Hz (saccade régulière, VRR ou triple tampon), et synchroniser son et image quand le vCPU est bridé.

Alternatives sans gain de performance : winit + wgpu + cpal + gilrs (100 % Rust, plus d'assemblage) ; Wayland / ALSA / evdev en direct (perte de portabilité).

**Conception :** SDL est isolé derrière des traits `Display`, `AudioSink`, `InputSource`, avec un backend headless pour les tests automatisés (étape 1).

---

## Choix du DOS

**Décision : un DOS réimplémenté côté hôte (HLE) qui se présente comme MS-DOS 5.0 / 6.22, plus un démarrage optionnel d'un vrai DOS (FreeDOS, ou l'image MS-DOS de l'utilisateur).** DOS/4GW ne départage pas les options : sa compatibilité dépend de la couche XMS / A20 / INT 15h, pas du DOS.

| DOS | Licence | Intérêt | Limites |
| --- | --- | --- | --- |
| FreeDOS | GPL | Actif, très compatible, FAT32, se présente comme DOS 7.10 ; dosemu2 en utilise un portage (fdpp) | Petits écarts avec MS-DOS sur le non-documenté |
| DR-DOS / OpenDOS 7.01, Enhanced DR-DOS | Code publié par Caldera en 1997, licence restrictive | Une branche reste maintenue (utilisée par SvarDOS, à vérifier) | Situation juridique floue : à éviter comme base |
| MS-DOS 4.0 | MIT (publié par Microsoft en 2024, comme 1.25 et 2.0 en 2018) | Le vrai MS-DOS, libre | Version datée et boguée, sans DOS=HIGH ni UMB (arrivés en 5.0) ; chaîne de compilation d'époque |
| MS-DOS 6.22 | Propriétaire | La référence de fait | Non redistribuable ; image fournie par l'utilisateur |

**Ce dont DOS/4GW a besoin.** Il bascule lui-même en mode protégé, sert de serveur DPMI au jeu et repasse en mode réel pour chaque appel INT 21h. Il lui faut :

- un INT 21h correct (fichiers, `4Bh` exec, lecture de son exécutable LE) ;
- de la mémoire étendue accessible : XMS, INT 15h `88h` / `E801h`, A20 ; à défaut VCPI ou un hôte DPMI ;
- aucun gestionnaire mémoire qui le gêne.

Dans la VM, l'invité a tous les privilèges : DOS/4GW tourne en mode raw ou XMS comme sur une vraie machine. DOSBox montre qu'un DOS en HLE suffit pour l'immense majorité des jeux DOS/4GW. Un hôte DPMI n'est à ajouter que pour les programmes qui l'exigent.

**Mise en œuvre retenue :**

1. **DOS en HLE (mode principal, étape 3)** : version annoncée 5.00 par défaut (INT 21h `30h`), configurable par programme comme SETVER ; structures internes reproduites (List of Lists, SFT, MCB, PSP) ; `mount` direct de répertoires hôtes ; XMS, EMS et A20 côté hôte ; code entièrement à nous.
2. **Démarrage d'un vrai DOS (option, étape 9)** : FreeDOS livré en image disque séparée (sa GPL ne contamine pas l'émulateur, comme pour QEMU), image MS-DOS 6.22 de l'utilisateur, éventuellement MS-DOS 4.0. Il suffit d'un INT 13h complet.
3. **Sources de référence** : MS-DOS 4.0 (MIT, réutilisable sans contrainte) et FreeDOS (GPL : s'en inspirer pose une question de licence).

Le mode « vrai DOS » sert aussi de filet de sécurité : un programme qui échoue sous notre DOS mais marche sous FreeDOS ou 6.22 désigne un bug du DOS HLE plutôt que du matériel émulé.

---

## Licence

**Décision : GPL-3.0-or-later.** Le code reste public, et toute version modifiée distribuée doit l'être sous la même licence. Le texte officiel est dans [`LICENSE`](LICENSE).

| Point | GPL-2.0-only | GPL-3.0-or-later |
| --- | --- | --- |
| Crates sous Apache-2.0 seule | Incompatible (avis de la FSF) | Compatible |
| Crates « MIT OR Apache-2.0 » (la majorité) | OK en retenant MIT | OK |
| rust-vmm (« Apache-2.0 OR BSD-3-Clause ») | OK en retenant BSD | OK |
| Reprise de code DOSBox, dosemu2, FreeDOS (« v2 or later », à vérifier fichier par fichier) | OK | OK |
| Nuked-OPL3 (LGPL-2.1+), ymfm (BSD), SDL3 (zlib) | OK | OK |
| Brevets, anti-« tivoïsation » | Rien | Clauses explicites |

- La GPL n'empêche pas la revente, mais oblige à fournir le source et les modifications : elle empêche l'appropriation. L'AGPL n'apporte rien pour un programme local.
- Appeler KVM ne fait pas du projet une œuvre dérivée du noyau Linux (GPLv2) : aucun conflit.
- Tant que l'auteur est seul, un changement ou une double licence reste possible. Avec des contributeurs : CLA si l'on veut garder cette option, sinon DCO (`Signed-off-by`).

Mise en œuvre : fichier `LICENSE` (GPLv3 officielle), `license = "GPL-3.0-or-later"` dans `Cargo.toml`, `// SPDX-License-Identifier: GPL-3.0-or-later` en tête de chaque fichier source.

---

## Risques et points durs

Deux risques peuvent invalider le projet et doivent être levés par un prototype avant tout investissement : **le bridage de vitesse** et **le coût du VGA planaire**.

| Risque | Impact | Piste de mitigation | À valider en |
| --- | --- | --- | --- |
| Vitesse excessive : boucles de calibrage qui débordent, jeux trop rapides | Bloquant pour une grande partie du catalogue | Compteur de performance (`perf_event`, instructions invitées uniquement) qui interrompt le vCPU toutes les N instructions, puis pause pour viser un débit cible (ex. 5 MIPS pour un 386) ; à défaut, tranches de temps + patchs connus (TPPATCH) | Étape 0 |
| Coût des sorties de VM en VGA planaire | Jeux mode X et EGA trop lents | Mesurer le coût réel par accès ; coalesced MMIO pour les écritures ; en dernier recours, petit interpréteur x86 qui exécute seulement les instructions qui touchent la VRAM | Étape 0 |
| Timing audio SB16 avec un CPU qui avance par à-coups | Craquements, désynchronisation | DMA cadencé sur l'horloge hôte, tampon audio de 20–40 ms, IRQ de fin de bloc injectée au bon moment | Étape 7 |
| Disponibilité de la virtualisation | Ne marche pas sans `/dev/kvm`, ni dans certaines VM | Message clair au démarrage ; pas de repli en émulation (hors périmètre) | Étape 0 |
| Portabilité Windows / macOS | Trois API d'hyperviseur différentes | Couche d'abstraction « backend VM » dès le départ, KVM en premier | Étape 10 |
| Compatibilité DOS en HLE | Programmes qui lisent les structures internes de DOS | Reproduire List of Lists, SFT, MCB ; ou basculer sur FreeDOS dans la VM | Étape 3 |
| CPU trop moderne vu de l'invité | Rares détections CPU qui échouent | Filtrer CPUID (`KVM_SET_CPUID2`) pour annoncer un CPU plus ancien | Étape 6 |
| Licences | Une dépendance incompatible bloquerait la distribution | Projet en GPL-3.0-or-later (voir [Licence](#licence)) ; vérifier la licence de chaque crate et de tout code repris | Étape 0 |

Le bénéfice réel doit aussi être assumé : pour la plupart des jeux DOS, DOSBox est déjà assez rapide. Le gain se voit surtout sur les jeux exigeants de la fin des années 90 (Quake, jeux SVGA, démos), sur la consommation, et sur la simplicité (pas de cœur CPU à maintenir).

---

## Plan par étapes

L'étape 0 est un **go / no-go** : si le bridage ou le VGA planaire ne tiennent pas leurs critères, on réévalue avant d'aller plus loin. Les étapes suivantes livrent chacune quelque chose qui tourne. Stack : **Rust** + crates rust-vmm + SDL3.

0. **Prototype et mesures (go / no-go)**
    - VM KVM de 1 Mo en mode réel, chargement d'un `.COM`, stubs INT 21h AH=09h et 4Ch : affiche « Hello » et quitte.
    - Mesurer le coût d'une sortie de VM sur port d'E/S et sur MMIO.
    - Tester le bridage par compteur d'instructions invitées.
    - Critère : bridage à ±20 % d'un débit cible, et coût MMIO compatible avec environ 20 000 accès VRAM par image à 70 Hz.
1. **Cœur VMM**
    - Carte mémoire (640 Ko, UMB, VRAM, ROM BIOS, mémoire étendue), boucle vCPU, mécanisme d'appel hôte, abstraction « backend VM ».
    - Journal des interruptions appelées et débogueur minimal (registres, dump mémoire, pas à pas).
    - Traits `Display`, `AudioSink`, `InputSource` : backend SDL3 et backend headless pour les tests.
    - Critère : tests automatisés sur de petits binaires assemblés avec NASM.
2. **BIOS en HLE**
    - BDA, INT 10h texte, 11h, 12h, 15h, 16h, 1Ah, tick INT 08h, 13h minimal.
    - Critère : un programme Turbo C ou Open Watcom en mode texte tourne.
3. **DOS en HLE**
    - PSP, MCB, chargeur MZ avec relocations, environnement, handles de fichiers sur un répertoire hôte monté (comme `mount c` de DOSBox), FCB minimal, shell interne.
    - Critère : outils en ligne de commande classiques (pkunzip, edit, programmes Turbo Pascal) fonctionnent.
4. **Périphériques de base**
    - PIC, PIT, 8042 et scancodes, RTC, haut-parleur PC, souris INT 33h.
    - Critère : un jeu texte ou CGA simple est jouable au clavier.
5. **Vidéo VGA**
    - Fenêtre SDL, rendu texte (police libre), mode 13h, palette DAC, CGA, puis EGA/VGA planaire et mode X.
    - Critère : un jeu mode 13h et un jeu mode X tournent à pleine vitesse.
6. **Mémoire et mode protégé**
    - XMS, EMS, UMB, filtrage CPUID, puis DOS/4GW, DOS32A, PMODE/W, CWSDPMI ; hôte DPMI en HLE si nécessaire.
    - Critère : Doom tourne.
7. **Sound Blaster 16**
    - DMA 8237, DSP SB16 (PCM 8 et 16 bits, auto-init), mixeur, IRQ, OPL3, variable `BLASTER`.
    - Critère : musique FM et effets numériques corrects dans Doom et un jeu Sierra, sans craquements.
8. **SVGA**
    - VBE 2.0, modes 640×480 à 1024×768 en 8/16/24 bits, fenêtres bankées, framebuffer linéaire.
    - Critère : Quake en 640×480 VESA et un jeu SVGA banké tournent.
9. **Confort d'utilisation**
    - Fichier de configuration, réglage de vitesse à chaud, plein écran, captures, CD-ROM (MSCDEX en HLE + images ISO/CUE), joystick, démarrage d'images disque d'un vrai DOS (FreeDOS, MS-DOS de l'utilisateur).
10. **Portabilité**
    - Backend WHPX (Windows), puis Hypervisor.framework (macOS Intel) si la demande existe.
11. **3dfx Voodoo (futur lointain)**
    - Wrapper Glide pour DOS traduit vers OpenGL ou Vulkan côté hôte ; émulation des registres Voodoo seulement si nécessaire.

L'étape 0 est terminée : voir [Avancement](#avancement).

Ordre de grandeur pour une personne : quelques semaines pour l'étape 0, plusieurs mois pour arriver à Doom avec le son, des années pour viser la compatibilité de DOSBox.

---

## Avancement

### Étape 0 : terminée le 2026-11-12 (go)

Mesures sur Ryzen AI 9 HX 370 (AMD-V), noyau Linux avec ses mitigations actives (SRSO, vmscape : IBPB à chaque sortie de VM). On ne les désactive pas, même pour mesurer.

| Point | Résultat | Banc |
| --- | --- | --- |
| Hello `.COM` sous KVM | OK : IVT → stubs `out 0xE0, al ; iret` en F000:n×16, INT 20h, INT 21h 09h/4Ch | `cargo run [fichier.com]` |
| Bridage | ±0,2 % de 1 à 500 MIPS (critère ±20 %) ; sans bridage ≈ 7 600 MIPS | `--throttle` |
| Sortie de VM | `cpuid` 1,3–1,7 µs ; `out` 3,4–4,7 µs ; MMIO 4,5–4,8 µs ; MMIO regroupé 2,8 µs | `--bench` |
| VGA planaire, une sortie par accès | ~650 % d'un cœur au critère (20 000 accès/image à 70 Hz) : **raté** | `--bench` |
| VGA planaire, interpréteur hôte | 73 ns/accès, ~10 % d'un cœur : **tenu** | `--bench` |
| Remappage d'un plan sur A0000 | slot KVM recréé : 70–100 µs ; vue hôte remappée (memfd + `mmap MAP_FIXED`) : 14,5 µs, rentable au-delà de ~600 accès par plan | `--remap` |

**Décision : interpréteur x86 côté hôte pour les rafales VRAM** (décodeur iced-x86). À la première sortie MMIO, l'hôte termine l'accès (`immediate_exit`), exécute les instructions suivantes jusqu'à 256 instructions sans VRAM (ou 100 000 au plus), puis rend la main. Toute instruction non gérée ou fautive repart dans la VM. À l'étape 5 : hybride avec le mappage d'un plan pour les longues passes mono-plan.

L'interpréteur couvre le jeu entier 8086→486 (+ cmov, setcc) en mode réel. Il est vérifié par un **test différentiel contre le processeur** (pas à pas KVM, état aléatoire, comparaison registres, drapeaux définis, mémoire) : ~454 000 instructions, aucune divergence.

```
DIFFTEST_CASES=100000 DIFFTEST_SEED=1 cargo test --release interpreter_matches_cpu -- --nocapture
```

Comportements constatés sur ce processeur : ENTER 32 bits avec pile 16 bits copie l'ESP entier dans EBP ; LOOPE/LOOPNE préfixés F2/F3 ignorent la condition documentée (laissés à la VM).

### Étape 1 : terminée le 2026-11-12

| Élément | Fichier | État |
| --- | --- | --- |
| Abstraction du moteur de virtualisation (`Backend` : run, état CPU, pas à pas) | `src/backend.rs`, `src/kvm.rs` | KVM ; WHPX à l'étape 10 |
| Carte mémoire : 640 Ko, fenêtre VGA non mappée, texte, UMB, ROM BIOS en lecture seule, mémoire étendue (16 Mo par défaut, A20 active) | `src/machine.rs` | fait |
| Boucle vCPU : appels hôte, bus d'E/S, MMIO + interpréteur, bridage (`--mips`) | `src/machine.rs`, `src/io.rs` | fait |
| ROM BIOS assemblée par GNU `as` (`build.rs`) | `asm/bios.S` | stubs F000:n×16, date, modèle AT |
| Journal des interruptions (`--trace-int`), des ports inconnus (`--trace-io`) | `src/machine.rs` | fait |
| Débogueur (`--debug` : registres, mémoire, désassemblage, pas à pas) | `src/debug.rs` | fait |
| Traits `Display`, `AudioSink`, `InputSource`, `TextSink` + backend headless | `src/host.rs` | fait ; **backend SDL3 reporté à l'étape 5** (rien à afficher avant) |
| Tests automatisés sur programmes en `as` (`asm/tests/*.S`) | `src/tests.rs` | 9 tests + interpréteur |

### Interpréteur en mode protégé : terminé le 2026-11-13

- Code 16 et 32 bits (CS.D), piles 16 et 32 bits (SS.B), adressage selon CS.D inversé par 67h.
- Contrôles du descripteur en cache à chaque accès : présence, type (code lisible, données en écriture), limite. Segments expand-down laissés au processeur.
- Laissés au processeur : chargements de registres de segment, transferts lointains, `int`/`iret`/`retf`, CLI/STI si CPL > IOPL, POPF hors ring 0, pagination (CR0.PG), mode V86.
- Test différentiel étendu au mode protégé (GDT, IDT et gestionnaires en mémoire, segments aléatoires dont lecture seule, expand-down, inutilisables ; CPL 0 et 3) : 10 graines × 100 000 cas, moitié en mode protégé, aucune divergence.
- Test de bout en bout `asm/tests/pmode.S` : passage en mode protégé 32 bits, `rep stosb` de 1 000 octets en A0000h, retour en mode réel ; une sortie MMIO, le reste interprété.
- Comportements constatés : ENTER fait #SS si le nouveau sommet de pile sort de la limite ; ROL/SHLD… mémoire avec compte nul contrôlent quand même l'écriture. Banc : après une triple faute, remettre le vCPU en `KVM_MP_STATE_RUNNABLE`, sinon `KVM_RUN` attend indéfiniment.

### Prochaines étapes (vers Doom sous DOS/4GW)

1. Étape 2 : BIOS en HLE (BDA, INT 10h texte, 11h, 12h, 15h dont 88h/E801h, 16h, 1Ah, tick INT 08h, 13h minimal).
2. Étape 3 : DOS en HLE (PSP, MCB, chargeur MZ, fichiers sur répertoire hôte).
3. Étape 4 : PIC, PIT, 8042, injection d'IRQ (et HLT).
4. Étape 5 : VGA (mode 13h, planaire, mode X) + backend SDL3.
5. Étape 6 : XMS, A20, puis DOS/4GW et Doom (pagination dans l'interpréteur si un extendeur l'active).

## Questions ouvertes

- ~~DOS réimplémenté côté hôte ou vrai noyau FreeDOS dans la VM ?~~ Tranché : voir [Choix du DOS](#choix-du-dos).
- ~~Licence du projet ?~~ Tranché : voir [Licence](#licence).

---

## Existant et références

dosemu2 réalise déjà une grande partie de cette idée (mode KVM sous Linux) : à étudier en premier, pour s'en inspirer ou y contribuer.

- [dosemu2](https://github.com/dosemu2/dosemu2) : DOS sous Linux, backends KVM, vm86 et émulation.
- [DOSBox Staging](https://github.com/dosbox-staging/dosbox-staging) et [DOSBox-X](https://github.com/joncampbell123/dosbox-x) : références pour le DOS en HLE, le VGA, la SB16 et la Voodoo.
- [Ralf Brown's Interrupt List](https://www.cs.cmu.edu/~ralf/files.html) : interruptions BIOS et DOS.
- [Sources MS-DOS](https://github.com/microsoft/MS-DOS) : versions 1.25, 2.0 et 4.0 sous licence MIT.
- [SvarDOS](http://svardos.org/) : distribution DOS utilisant le noyau Enhanced DR-DOS (licence à vérifier).
- [FreeDOS](https://www.freedos.org/) : noyau et outils libres.
- [FreeVGA](http://www.osdever.net/FreeVGA/home.htm) : registres VGA.
- [API KVM](https://docs.kernel.org/virt/kvm/api.html) et [rust-vmm](https://github.com/rust-vmm) : construction du VMM.
- [Nuked-OPL3](https://github.com/nukeykt/Nuked-OPL3) et [ymfm](https://github.com/aaronsgiles/ymfm) : émulation OPL3.
