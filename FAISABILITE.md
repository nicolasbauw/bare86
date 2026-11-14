# Bare86 — étude de faisabilité d'un émulateur DOS sans émulation x86

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

### Étape 2 : BIOS en HLE, terminée le 2026-11-13

État dans la BDA, comme un vrai BIOS (`src/bios.rs`) ; registres et drapeaux rendus par IRET dans `src/hle.rs`.

| Service | Couvert |
| --- | --- |
| BDA | équipement, 640 Ko, tampon clavier, état vidéo (mode, colonnes, lignes, pages, curseurs), tics, drapeau de minuit, octets EGA/VGA |
| INT 10h | modes texte 0–3 et 7 (graphiques : BDA seulement, affichage à l'étape 5), curseur, pages, défilement 06h/07h, 08h/09h/0Ah, télétype 0Eh, 0Fh, 13h, 1130h, 12h BL=10h, 1Ah ; palette acceptée sans effet ; VBE absent (AL ≠ 4Fh) |
| INT 11h, 12h | équipement, mémoire conventionnelle |
| INT 13h | minimal : aucun lecteur |
| INT 15h | 2401h–2403h (A20 toujours ouverte), 86h, 87h, 88h, C0h (table en F000:E6F5), E801h, E820h ; sinon AH=86h, CF=1 |
| INT 16h | 00h–03h, 05h, 10h–12h ; tampon de la BDA alimenté par `InputSource` (scancodes jeu 1, ou caractères du terminal) |
| INT 1Ah | tics, RTC (heure locale hôte, BCD) |

Provisoire jusqu'à l'étape 4 : tics recalculés d'après l'horloge hôte à chaque appel hôte (un programme qui scrute 0040:006C sans aucun appel ne les voit pas avancer) ; touches rangées dans le tampon par l'hôte, sans IRQ 1. INT 21h 02h/09h passent par le télétype du BIOS. En ligne de commande, le clavier du terminal est lu sans écho et `--screen` affiche l'écran texte à la fin. Tests : `asm/tests/bios_{text,info,kbd,tick}.S`, 21 tests au total.

Critère tenu : `c/tests/textdemo.c`, compilé en `.COM` (modèle tiny) par Open Watcom v2, place du texte en couleur par INT 10h, écrit par `printf` et lit une touche par `getch` (test `watcom_text_program`). Open Watcom est construit depuis ses sources dans `~/Dev/open-watcom-v2` (`./build.sh rel` ; seule l'aide du navigateur Windows échoue, faute de DOSBox) ; `c/build.sh` recompile les tests, et les `.COM` sont versionnés pour que `cargo test` se passe du compilateur. `graph.lib` ne se lie pas en modèle tiny (relocations de segment) : elle attendra le chargeur MZ.

Le runtime Watcom a demandé, en avance sur l'étape 3, une petite partie de l'INT 21h : 01h, 07h, 08h, 0Bh (console par le clavier du BIOS, touches étendues en deux appels), 40h sur les handles 1 et 2, 4400h, 4Ah (sans MCB : tout jusqu'à A000h), 6300h (table DBCS vide en F000:E700), 6601h (page de codes 437). L'état du DOS vit dans `dos::Dos`.

### Étape 3 : DOS en HLE, en cours (2026-11-13)

`src/dos/` : `memory.rs` (MCB), `process.rs` (PSP, environnement, chargeurs), `handles.rs` (JFT et SFT), `fs.rs` (lecteurs, noms 8.3, motifs), `fcb.rs`, `shell.rs`, `mod.rs` (INT 21h).

| Domaine | Couvert |
| --- | --- |
| Mémoire | chaîne de MCB en mémoire invitée à partir de 0100h (premier, meilleur, dernier bloc), 48h, 49h, 4Ah, 58h ; pas d'UMB avant l'étape 6 |
| Processus | PSP complet (JFT, vecteurs 22h–24h, FCB 1 et 2, ligne de commande), environnement hérité + chemin du programme, chargeurs .COM et MZ (relocations, chargement en haut), 4B00h (exec, état du parent rétabli à la fin de l'enfant), 4B03h (overlay), 00h/4Ch/31h, INT 20h/27h, 4Dh, 50h/51h/62h ; shell racine avec PSP et environnement maître |
| Fichiers | répertoire hôte monté (`--mount`, par défaut celui du programme) ; noms 8.3 générés pour les noms longs (`LONGFI~1.TXT`) ; handles par la JFT du PSP vers une SFT hôte (héritage, 45h/46h, 67h) ; 3Ch–43h, 56h, 57h, 5Ah, 5Bh, 6Ch ; périphériques CON, NUL, AUX, PRN ; 4Eh/4Fh (DTA), 39h–3Bh, 47h, 60h ; 44h (00h, 01h, 06h–0Ah, 0Eh) |
| FCB | 0Fh–17h, 21h–24h, 27h, 28h, 29h |
| Divers | console 01h–0Ch, date et heure, 25h/35h, 30h (5.00), 33h, 34h, 37h, 38h/65h (États-Unis, page de codes 437), 52h (List of Lists avec en-tête NUL), 59h, 63h, 66h ; LFN (71h) refusé comme MS-DOS 5 |
| Shell | invite, DIR, CD, X:, TYPE, CLS, ECHO, SET, VER, MD, RD, DEL, EXIT ; programmes cherchés dans le répertoire courant puis C:\ (.COM puis .EXE) ; pas de .BAT |

Tests : `c/tests/files.c` (exécutable MZ Watcom small : argv, environnement, fichiers, recherche, répertoires, renommage, malloc, `spawnl` d'un enfant), `asm/tests/fcb.S`, session de shell scriptée ; 28 tests. Corrigé en route : le tampon clavier du BIOS perdait les touches au-delà de 15 quand l'entrée arrivait d'un bloc (texte collé, tube).

Critère, avec des binaires d'époque rangés hors du dépôt dans `~/Dev/dos-progs` (PKZIP 2.04g de HPCalc, MD5 de `PKZIP.EXE` conforme à PCjs ; EDIT 0.9a du dépôt FreeDOS 1.3 ; Turbo Pascal 5.5 de l'ancien musée Embarcadero) :

| Programme | Résultat |
| --- | --- |
| PKZIP 2.04g | archive créée, validée par `unzip -t` sur l'hôte |
| PKUNZIP 2.04g | test et extraction ; fichiers identiques aux originaux |
| FreeDOS EDIT 0.9a | saisie, menus, enregistrement (Ctrl+S), sortie par le menu ; pilotée par `--keys` |
| TPC 5.5 | compile un programme Crt + Dos |
| Programme Turbo Pascal | tourne depuis l'étape 4 (Crt, fichiers, `Delay`) ; bloqué avant : l'initialisation de Crt calibre `Delay` en scrutant 0040:006C sans aucun appel |

Le pilotage d'EDIT a demandé un clavier scripté réaliste : `--keys` (texte et touches nommées, puis fin de session), touches livrées une à une (la suivante attend que la précédente soit lue ; les boucles d'événements comme DFlat inversaient sinon raccourcis et texte), AH=02h sans lecture de l'entrée (l'état des modificateurs reste celui de la frappe), délai de grâce de 300 ms après la fin du script pour un programme qui scrute le clavier. La trace `--trace-int` montre aussi les registres et CF au retour.

### Étape 4 : périphériques de base, terminée le 2026-11-13

| Élément | Fichier | État |
| --- | --- | --- |
| Injection d'interruptions | `src/backend.rs`, `src/kvm.rs` | KVM_INTERRUPT (ioctl direct), fenêtre d'interruption ; passe par l'IDT en mode protégé |
| PIC 8259 maître + esclave | `src/pic.rs` | ICW1–4, OCW1–3, EOI, cascade, priorités ; vecteurs 08h et 70h |
| PIT 8254 + port 61h | `src/pit.rs` | modes 0–5, verrouillage, relecture, accès lo/hi ; IRQ 0 au rythme programmé ; bascule de rafraîchissement ; canal 2 (haut-parleur : fréquence prête pour le son de l'étape 5) |
| Battement | `src/timer.rs` | minuterie POSIX périodique vers le thread du vCPU (EINTR), période = IRQ 0 bornée entre 50 µs et 10 ms |
| HLT | `src/machine.rs` | attente jusqu'à la prochaine IRQ 0 (10 ms au plus) |
| ROM | `asm/bios.S` | INT 08h (F000:FEA5 : tics, minuit, INT 1Ch, EOI), INT 09h (F000:E987 : port 60h, INT 15h/4Fh, scancode à l'hôte par le port E1h), IRQ par défaut, INT 1Ch en IRET, tremplins `sti; hlt` de reprise |
| 8042 | `src/kbc.rs` | tampon, IRQ 1, octet de commande, tests, port de sortie (A20, pour l'étape 6), commandes clavier (LED, identification, reset) |
| Clavier | `src/bios.rs`, `src/machine.rs` | scancodes jeu 1 (préfixe E0h), BDA tenue par l'INT 09h ; une frappe à la fois (2 ms, appui suivant après lecture de la précédente sauf INT 09h détournée) |
| Attentes de touche | `src/machine.rs`, `src/dos/` | plus aucune attente bloquante côté hôte : INT 16h et DOS rendent `Flow::Wait`, registres d'entrée rétablis, reprise après HLT ; saisie de ligne par morceaux |
| RTC CMOS | `src/rtc.rs` | heure locale BCD, UIP, état A–D, mémoire et équipement, somme de contrôle |
| Souris | `src/mouse.rs` | INT 33h : reset, position, boutons, plages, mickeys, curseur texte ; gestionnaire utilisateur (0Ch) enregistré, pas encore appelé |
| VGA minimal | `src/vga.rs` | état 3DAh/3BAh (retours de trame à 70 Hz et de ligne), CRTC et CGA conservés ; INT 10h 12h BL=30h–36h |
| Terminal | `src/term.rs`, `src/host.rs` | `--term` : écran texte en direct (couleurs CGA 24 bits, CP437 complet, curseur), flèches, Origine/Fin, Pg, Inser/Suppr, F1–F12 et Alt+lettre décodés ; `--keys` gagne `{wait:ms}` |

Critère tenu : ZZT 3.2 (Epic MegaGames, gratuit depuis 1997, Museum of ZZT, MD5 conforme), jouable au clavier : configuration, menu, partie dans la ville de ZZT, déplacements ; vérifié par `--keys` et dans un pseudo-terminal avec `--term` et de vraies séquences de flèches. Le programme Turbo Pascal de l'étape 3 tourne aussi (dernier point de son critère). Tests : `asm/tests/{irq,int9,mouse}.S` et tests unitaires (PIC, PIT, 8042, RTC, souris, VGA) ; 40 tests.

Constaté en route : ZZT attend le retour de trame VGA sur le port 3DAh (sans lui, boucle infinie) ; DFlat (EDIT) lit l'état des modificateurs juste après la touche.

### Étape 5 : décision sur la mémoire vidéo (2026-11-13)

But : réduire l'émulation d'instructions à un secours rare, sans renoncer au mode planaire.

Constat : toute sortie MMIO passe déjà par l'émulateur x86 de KVM (`arch/x86/kvm/emulate.c`), qui décode l'instruction fautive. Une conception « tout MMIO » repose donc aussi sur de l'émulation d'instructions, cachée dans le noyau. Notre interpréteur ne fait que prolonger ce travail sur les instructions qui touchent la VRAM : le programme reste exécuté nativement ; seuls les accès au périphérique sont émulés, comme dans tout hyperviseur.

**Décision : mapper la VRAM directement dès que la VGA se comporte comme de la RAM, l'interpréteur ne servant qu'au reste.**

| Configuration | Cas typiques | Traitement |
| --- | --- | --- |
| Texte, CGA, mode 13h (chain4) | la majorité des jeux | tampon linéaire mappé, zéro sortie (QEMU fait de même pour chain4) |
| Non chaîné, écriture simple (mode 0, sans rotation, remplacement, masque de bits FFh, set/reset inactif) vers un seul plan P, lecture sur P | mode X ; Doom (copie de l'image plan par plan) | plan P mappé (vue hôte memfd remappée, 14,5 µs) |
| Latches (mode 1), opérations logiques, masque de bits, set/reset, modes 2 et 3, comparaison de couleur | EGA 16 couleurs, blits en mode X | interpréteur hôte (73 ns/accès) |

- **Adaptatif** : un programme qui change de plan à chaque pixel paierait plus en remappages (14,5 µs ≈ 200 accès interprétés) qu'en interprétation. On ne mappe donc une configuration qu'après un seuil d'accès sans changement (≈ 600, cf. étape 0).
- **Ordre de grandeur pour Doom** (64 000 octets par image) : environ 4,7 ms par image interprétée, contre environ 60 µs avec le mappage (4 sorties `out` + 4 remappages).
- **Bonus** : le mappage EPT/NPT vaut aussi en mode protégé ; l'extension de l'interpréteur à la pagination (DOS/4GW) devient probablement inutile.
- **Mesure** : compteurs d'accès mappés et interprétés, pour suivre ce qui reste émulé.

Pistes écartées :

- **Pages en écriture seule** pour séparer lecture et écriture : interdit par EPT (configuration invalide).
- **MMIO regroupé** : une sortie vers le noyau par accès (2,8 µs) ; inapplicable aux lectures.
- **Écrire dans une mémoire fantôme, puis appliquer la logique VGA en bloc à chaque changement de registre.** Juste en théorie : sans lecture, donc avec des latches constants, l'effet d'une suite d'écritures sur un octet ne dépend que de la dernière. Mais le suivi des pages modifiées de KVM ne donne que des pages de 4 Ko, et un octet réécrit avec sa valeur précédente reste invisible. Gardée en réserve pour le mode d'écriture 3 (une écriture de 0 n'y a aucun effet).

Plan de l'étape 5 :

1. Cœur VGA : registres (séquenceur, contrôleur graphique, attributs, DAC, divers, CRTC), 256 Ko en 4 plans dans une mémoire remappable, tous les modes de lecture et d'écriture.
2. Gestionnaire de la fenêtre A0000 : mode linéaire, plan unique ou interprété, avec seuil adaptatif et compteurs.
3. Rendu : texte, CGA, EGA, 13h, X ; palette, clignotement, curseur.
4. Backend SDL3 : fenêtre, clavier, souris, haut-parleur ; vCPU sur son propre thread.
5. Critère : un jeu en mode 13h et un jeu en mode X à pleine vitesse, avec la part mappée mesurée.

Avancement :

- **Points 1 et 2 faits (commit 42e7748).** `src/vga.rs` : registres complets, memfd de 5 × 64 Ko (4 plans + tampon linéaire chain4, réparti ou rassemblé au passage chain4 ↔ non chaîné, plan a & 3 au décalage a & !3), tous les modes de lecture et d'écriture, DAC (palette par défaut approchée), INT 10h programme les registres des modes 0Dh, 0Eh, 10h, 12h, 13h et texte. `src/machine.rs` (`update_window`) : linéaire mappé immédiatement ; plan d'écriture mappé après 600 accès émulés, puis simple remappage de la vue à chaque changement de plan ; démappage pour les configurations à logique de plans ; refroidissement de 0,5 s au-delà de 2 000 changements de configuration par seconde ; compteurs `vram_emulated`, `vram_maps`, `vram_unmaps`.
- Règle affinée en route : Doom ne règle jamais le plan de lecture quand il copie son image. On mappe donc le plan *écrit*, sauf si une lecture a été vue pendant la phase émulée d'une configuration dont le plan de lecture diffère (une lecture native rendrait le mauvais plan). Limite connue : en accès natif, les lectures ne chargent pas les latches.
- Test `asm/tests/vram.S` : remplissage en mode 13h natif ; en mode X, premier plan émulé jusqu'au seuil, second plan natif ; copie par les latches émulée. Au total, 16 200 accès émulés sur 96 200.
- **Bogue de l'étape 0 corrigé** : une instruction à deux accès MMIO (MOVS de la VRAM vers la VRAM) perdait son écriture, car `complete_pending` ignorait la seconde sortie MMIO. Elle est maintenant servie.

- **Point 3 fait (commit 8dd4fe2 et suivant).** `src/render.rs` : 256 couleurs (chain4 : adressage en doubles mots ; non chaîné : plan x & 3), planaires 16 couleurs (palette des attributs, puis DAC ; l'INT 10h charge la palette EGA de 64 couleurs, comme le BIOS VGA), CGA 4 et 2 couleurs (RAM B8000 entrelacée) ; géométrie lue dans le CRTC. Mode texte avec police (attributs, clignotement, curseur). Capture `--shot F.png` (encodeur PNG sans dépendance). Pas encore gérés : décalage horizontal, écran partagé. La palette 256 couleurs par défaut n'est qu'une approximation de celle du BIOS.
- **Police en attendant la décision** : chargée à l'exécution depuis un fichier PSF1 ou PSF2 (gzip accepté, `src/font.rs`). Par défaut, `/usr/share/kbd/consolefonts/default8x16.psfu.gz` (paquet kbd) ; sinon `--font F.psf`. Rien n'est embarqué ni distribué ; les glyphes CP437 sont retrouvés par la table Unicode. Vérifié sur une capture de ZZT, rendu exact.
- **Point 4 fait en partie (commit suivant).** `src/sdl.rs` : fenêtre SDL3 (`--window`), vCPU sur son propre thread (machine et signaux créés dessus), images partagées à 70 Hz, affichage au rapport 4:3. Clavier par position physique (scancodes HID → jeu 1, E0h pour les touches étendues : le DOS voit un clavier US). Souris relative et boutons. Vérifié avec le pilote vidéo « offscreen » de SDL. **Restent** : le son du haut-parleur (la fréquence est déjà calculée par `Pit::speaker`), et le shell interne dans la fenêtre (pour l'instant, `--window` exige un programme).

- **Point 5, mode X : Wolfenstein 3D 1.4 shareware.** Copie archivée du FTP de 3D Realms, 856 401 octets, la taille de la fiche de l'archive ; stockée hors dépôt dans `~/Dev/dos-progs/wolf3d`.
  - **Installation** : l'installeur DOS d'origine tourne et décompresse le jeu.
  - **Jeu** : écran d'accueil (souris détectée, pas d'EMS/XMS avant l'étape 6), titre, générique, puis démo en 3D rendue correctement (captures `--shot`).
  - **Corrections nécessaires** :
    - **Registre d'état** : la boucle d'attente d'id Software repère le *blanc vertical* (bit 0 tenu sans le bit 3). Le registre modélise maintenant 449 lignes, dont 400 affichées, avec une synchronisation de 2 lignes dans le blanc vertical.
    - **Clavier scripté** : les touches sont tenues 40 ms. Le jeu lit l'état du clavier à chaque image, et un appui de 2 ms passait inaperçu.
  - **Mesures sur 60 s** :
    - Pendant la démo, la VRAM est surtout émulée (≈ 350 000 accès/s, soit ≈ 2,5 % d'un cœur). Wolfenstein dessine les murs colonne par colonne en changeant de plan : c'est le cas « un plan par pixel » que le refroidissement renvoie volontairement à l'interpréteur.
    - Le gros des sorties de VM vient des scrutations du port 3DAh et de l'INT 33h AX=0003h : ≈ 40 000 lectures de la souris par seconde, chacune un appel hôte.
  - **Optimisation notée** : servir les lectures de la souris (03h, 0Bh) en ROM, à partir d'une zone en mémoire invitée tenue à jour par l'hôte.
- **Reste dans le menu de Wolfenstein** : la navigation scriptée ne parvient pas encore de façon fiable jusqu'à « New Game » (position par défaut du curseur à éclaircir, dans la fenêtre plutôt qu'au script).
- **Outils ajoutés** : `--stats` (compteurs en fin d'exécution) et `--limit S` (arrêt au bout de S secondes, avec registres et code à CS:IP), utiles pour diagnostiquer une boucle sans appel.
- **Son et shell dans la fenêtre (commits 428fd4e et suivant).**
  - Haut-parleur : le vCPU publie la fréquence (`Pit::speaker`) dans une variable atomique ; le thread de la fenêtre génère l'onde carrée (48 kHz, environ 40 ms d'avance) dans un flux audio SDL. Le son numérisé par modulation de largeur d'impulsion n'est pas rendu.
  - Le shell interne s'affiche maintenant en fenêtre : sa lecture de ligne rafraîchit l'affichage avant chaque attente.
  - Vérifié sans écran, avec les pilotes SDL `offscreen` et `dummy`.
- **Optimisation de la souris faite.** L'INT 33h AX=0003h est servi en ROM (F000:E870), d'après un bloc d'état en ROM (F000:E890) que l'hôte réécrit à chaque changement ; l'invité ne peut que le lire.
  - Sur 50 s de Wolfenstein, les appels hôte passent de 3 995 423 à 341.
  - Les sorties de VM restantes sont presque toutes des scrutations du port 3DAh (≈ 226 000/s) : l'attente active du jeu, qui coûte aussi un cœur sur une vraie machine.
  - Piste notée sans être poursuivie : convertir cette attente en sommeil hôte. Mais pendant l'affichage, le bit 0 change à chaque ligne (25 µs), donc l'heuristique se déclencherait rarement.
- **Point 5, mode 13h : SkyRoads (Bluemoon, gratuit).** Téléchargé depuis le site de Bluemoon, en HTTP simple car son certificat TLS est refusé ; aucune somme de contrôle n'est publiée, donc l'intégrité n'est pas vérifiable au-delà de la structure de l'archive. Stocké dans `~/Dev/dos-progs/skyroads`. Titre et partie rendus correctement, **avec zéro accès VRAM émulé** (`vram_emulated: 0`, `mmio_exits: 0`, un seul mappage de la fenêtre) : en mode chain4, toute la mémoire vidéo est accédée nativement, ce qui est l'objectif de la décision de l'étape 5.
- **Critère de l'étape 5 tenu sur le fond** : un jeu en mode 13h (SkyRoads, en accès natifs) et un jeu en mode X (Wolfenstein 3D, démo 3D) tournent et s'affichent correctement. **Reste à confirmer** : la vitesse ressentie et le son, par un essai interactif dans la fenêtre par le propriétaire du projet (image, son, clavier, vitesse ressentie). Commande : `target/release/bare86 --window --mount ~/Dev/dos-progs/wolf3d/c/WOLF3D ~/Dev/dos-progs/wolf3d/c/WOLF3D/WOLF3D.EXE`, ou `--window` seul pour le shell.

- **Essai interactif de Wolfenstein 3D par le propriétaire du projet : bon.** Seul défaut relevé : la souris n'était pas capturée. Corrigé : un clic dans la fenêtre la capture (mode relatif SDL, curseur masqué), le bouton central, Ctrl+F10 ou la perte du focus la libèrent ; le titre de la fenêtre l'indique. Son du haut-parleur confirmé sous Wolfenstein.
- **Police : Terminus 8×16 (choix du propriétaire), embarquée.** `tools/terminus_cp437.py` extrait les 256 glyphes CP437 de `ter-u16n.bdf` (Terminus 4.49.1, source officielle) vers `fonts/terminus-cp437-8x16.bin` (4 Kio), licence dans `fonts/OFL-Terminus.txt`. Aucun glyphe manquant. `--font F.psf` reste possible.
- **Dune (Cryo, 1992, copie du propriétaire)** : `DUNEPRG.EXE ENG VGA` (sans EMS, étape 6) tourne : logo Virgin, intro spatiale, puis le jeu (salle du trône, curseur souris), en mode 13h natif (`vram_emulated: 0`). Stocké dans `~/Dev/dos-progs/dune`.
- **Mise à l'échelle nette** : l'étirement direct au plus proche voisin (200 → 720 lignes, facteur 3,6) doublait certaines lignes, visible sur le texte de Dune. Désormais : agrandissement entier sans lissage vers une texture intermédiaire, puis réduction lissée à la taille de la fenêtre. Vérifié sur une capture de la vraie fenêtre (logo Virgin).

### Étape 6 : mémoire étendue et extendeurs, terminée le 2026-11-13

**Doom 1.9 shareware tourne** (DOS/4GW 1.95, démo, jeu). Archive `doom19s.zip` de l'archive idgames, installée dans l'émulateur par son `DEICE.EXE` puis l'auto-extracteur ; `DOOM1.WAD` a le MD5 connu de la 1.9 shareware (`f0cefca49926d00903cf57551d901abe`). Stocké hors dépôt dans `~/Dev/dos-progs/doom/c/DOOMS`.

- **DOS/4GW tournait déjà en mode raw** (aucune pagination, donc pas besoin de l'étendre à l'interpréteur) : un programme Watcom 32 bits alloue, écrit et relit 4 Mo (somme exacte).
- **Vitesse : `DOOM -timedemo demo1`**, 5026 tics de jeu.

  | Version | Tics réels | Images/s | Cause |
  |---|---|---|---|
  | Départ | 8920 | 19,7 | Doom change de plan à chaque colonne (≈ 1 400 fois par image) : le refroidissement renvoyait toute la VRAM à l'interpréteur (561 000 instructions interprétées par image) |
  | Sans refroidissement | 4263 | 41 | Chaque changement de plan = `mmap` + invalidation EPT + refautes (≈ 13 µs, 89 s de temps système sur 122) |
  | Scène VGA | 1412 | **124** | Voir ci-dessous |

  Réf. : un 486DX2-66 fait ≈ 20 i/s, un Pentium 100 ≈ 40.
- **Scène VGA** (`src/vga.rs`) : la vue de la fenêtre ne change plus de cible pour un plan. Elle reste sur une région « scène » du memfd, qui tient le contenu du plan choisi ; changer de plan = deux copies de 64 Ko (scène → ancien plan, nouveau plan → scène), sans appel système ni faute de page dans KVM. Les accès du modèle passent par `locate`. Le refroidissement est supprimé : un changement de plan coûte moins qu'une sortie MMIO. Wolfenstein en profite aussi (accès émulés ≈ 350 000/s → quelques milliers).
- **XMS 3.0** (`src/xms.rs`) : INT 2Fh 4300h/4310h, point d'entrée en ROM F000:E8A0 (saut court + 3 NOP, `out E0h, al`, `retf`) ; HMA, A20 (toujours ouverte), EMB au-dessus de la HMA (allocation, libération, copie, verrouillage, redimensionnement, versions 32 bits 88h/89h/8Eh/8Fh). Pas d'UMB. INT 15h AH=88h rend 0, comme sous HIMEM.SYS. DOS/4GW, DOS/32A et PMODE/W passent par XMS.
- **EMS LIM 4.0** (`src/ems.rs`) : page frame en E000h, pages logiques côté hôte (`--ems MO`, 4 par défaut), mappage par copie de 16 Ko. Détection : le vecteur INT 67h pointe en FE8Bh:0012h, segment qui commence par un en-tête de périphérique « EMMXXXX0 » en ROM. Limite : une page logique mappée dans deux pages physiques n'est pas partagée. Dune tourne avec `ENG VGA EMS 386`, ses arguments d'origine ; Wolfenstein voit l'EMS et la XMS.
- **CPUID filtré** (`src/kvm.rs`) : sans table, KVM rendait des zéros (famille 0), et DOS/32A et PMODE/W concluaient à un 286. L'invité voit maintenant un Pentium MMX (famille 5, modèle 4, fabricant de l'hôte, FPU/VME/DE/PSE/TSC/MSR/CX8/MMX). DOS/32A 9.1.2 et PMODE/W (Open Watcom) font tourner le programme de test.
- **Divers** : port 92h (A20 rapide, reflète le port de sortie du 8042), INT 33h AX=0015h–0017h (état du pilote souris, demandé par DOS/4GW).
- Tests : `asm/tests/{xms,ems,cpu386}.S`, tests unitaires XMS, EMS, scène VGA ; 54 tests.

- **Porte A20 réelle** : la HMA (100000h–10FFFFh) a son propre slot KVM, qui pointe sur la mémoire de 1 Mo (porte ouverte) ou sur les 64 premiers Ko (fermée : repliement de FFFF:0010 sur 0000:0000). Le port de sortie du 8042 fait foi ; port 92h, XMS 03h–07h (globale et compteur local) et INT 15h 2400h–2402h passent par lui. Fermée au démarrage, comme après le POST d'un AT : les .EXE compressés par EXEPACK chargés sous 64 Ko ne voient plus la mémoire au-delà de 1 Mo. Test `asm/tests/a20.S`.
- **UMB** : XMS 10h–12h, en C800h–DFFFh (96 Ko ; C000h–C7FFh réservé à une ROM VGA, E000h à la page frame EMS). Le DOS ne les relie pas encore à sa chaîne de MCB (INT 21h 5803h) : ils servent aux programmes qui les demandent à XMS.
- **DJGPP et CWSDPMI : Quake 1.06 shareware** (archive idgames `quake106.zip`, installée par son `DEICE.EXE` ; `PAK0.PAK` au MD5 connu `5906e5998fc3d896ddaf5e6a62e03abb` ; `~/Dev/dos-progs/quake/c/QUAKE_SW`). CWSDPMI s'installe (avec pagination), Quake alloue 14 Mo, charge son PAK, passe en mode 13h et joue ses démos. `QUAKE -nosound -nocdaudio +timedemo demo1` : **969 images en 3,4 s, 288 i/s**, sans accès VRAM émulé. Sa boucle de temps lit le compteur 0 du PIT (≈ 75 000 lectures/s) : correcte, mais c'est l'essentiel des sorties de VM.
- `--stats` donne aussi les ports d'E/S les plus sollicités.

**Critère de l'étape 6 tenu** (Doom tourne, ainsi que Quake, Dune avec EMS, DOS/4GW, DOS/32A, PMODE/W, CWSDPMI). Reste hors critère : relier les UMB à la chaîne de MCB du DOS (`DOS=UMB`, `LOADHIGH`), un hôte DPMI en HLE seulement si un programme l'exige.

### Étape 7 : Sound Blaster 16, terminée le 2026-11-13

- **DMA 8237** (`src/dma.rs`) : canaux 0–3 et 4–7, pages, auto-init, sens décroissant, fin de comptage ; transferts tirés par le périphérique au fil du rendu.
- **DSP SB16 4.05** (`src/sb16.rs`) en 220h, IRQ 5, DMA 1 et 5 : réinitialisation, version, sortie directe, DMA 8 et 16 bits (cycle unique, auto-init, haute vitesse), taux (40h, 41h), pause et reprise, silence (80h), IRQ forcées (F2h, F3h), mixeur (état des IRQ en 82h, volumes maître et voix). Rééchantillonnage linéaire vers 48 kHz.
- **Mixeur** (`src/audio.rs`) sur le thread du vCPU : trames dues d'après l'horloge hôte, haut-parleur PC compris ; sortie vers une file que la fenêtre verse dans le flux SDL (coussin de 40 ms reconstitué si le flux se vide), ou fichier WAV (`--wav F.wav`, sans fenêtre). Variable `BLASTER=A220 I5 D1 H5 T6` dans l'environnement.
- **Doom** (`snd_sfxdevice 3` dans `DEFAULT.CFG`) : les effets numérisés de la démo sont rendus (WAV analysé : silence puis tirs et bruitages dès le début de la démo, crêtes ≈ 12 000, sans écrêtage).
- **FM OPL3** (`src/opl.rs`) : Nuked-OPL3 1.8 (LGPL-2.1+, commit `765ec96`, copie non modifiée dans `third_party/nuked-opl3`, compilée par `build.rs` avec le crate `cc` ; accord du propriétaire du projet). Ports 388h–38Bh, 220h–223h, 228h–229h ; minuteries et registre d'état (détection AdLib, OPL3 reconnue par les bits 1–2 à 0). Chaque écriture est horodatée et appliquée à son échantillon lors du rendu, au lieu du début du bloc suivant (jusqu'à 10 ms de décalage). Gain FM ×2 : Nuked sort ≈ 6 dB sous le numérique d'une vraie carte.
- **Mixeur SB16** : volumes par pas de 2 dB (31 = 0 dB), au maximum au démarrage.
- **Doom** (`snd_musicdevice 3`) : musique FM dès l'écran titre, puis musique et bruitages pendant la démo (rms ≈ 1 700 puis 3 500–5 500, un échantillon écrêté sur 20 s). **Wolfenstein** : musique AdLib (rms ≈ 800).
- **Écoute de Doom dans la fenêtre par le propriétaire du projet : parfaite.**
- **Jeu Sierra : démo officielle de King's Quest VI** (SCI1.1, 1992 ; archive.org `KingsQuestViHeirTodayGoneTomorrowDemo`, `kq6demo.ZIP`, MD5 `43395de532bd65122969ae379b4db37b`, déjà configurée `ADL.DRV` + `AUDBLAST.DRV` ; `~/Dev/dos-progs/kq6demo/c`, lancer `GAME.EXE`). Logo Sierra, écran titre, puis la démo, avec musique FM et son numérisé.
  - **Détection d'IRQ d'`AUDBLAST.DRV`** : il démasque les IRQ 3, 5, 7, 10, lance un transfert DMA d'un octet (14h) et l'attend par une boucle calibrée pour un 386, qui ne dure qu'≈ 10 µs à la vitesse native (mesuré). Livrée au passage suivant du mixeur, l'IRQ arrivait trop tard (« Unable to initialize your audio hardware ») ; livrée tout de suite, elle précédait la remise à zéro du drapeau attendu. Désormais, un transfert en cycle unique de ≤ 64 unités est achevé après 64 instructions en pas à pas.
  - **Fuite de TF** : pendant ce pas à pas, un `pushf` ou un `int` de l'invité empile TF=1, que le pilote rétablit plus tard (piège déjà noté). L'INT 1 qui arrive alors sur le stub de la ROM (aucun débogueur dans l'invité) efface TF au retour.
  - Niveaux : musique FM forte (rms ≈ 15 000), 0,05 % d'échantillons écrêtés dans les passages les plus forts.
- **Écoute de la démo de King's Quest VI par le propriétaire du projet : pas de craquements, musique correcte. Critère de l'étape 7 tenu.**
- **FM sur un thread audio** (`src/audio.rs`, `src/opl.rs`) : le vCPU garde ce qui répond à l'invité (adresse, minuteries et état de l'OPL ; DSP et DMA, qui lisent la mémoire invitée ; haut-parleur) et envoie toutes les 5 ms un bloc déjà mixé avec les écritures OPL horodatées ; le thread audio y ajoute la FM (Nuked-OPL3) et livre à la sortie. Un seul canal : toute écriture antérieure à la fin d'un bloc arrive avant lui. Le mixeur ne rend qu'au-delà d'1 ms due, testée par une simple comparaison d'instants.
- **Mesure** (Doom `-timedemo demo1`, tics réels, médiane de 5 passes alternées, même machine) :

  | Version | Tics | i/s |
  |---|---|---|
  | Avant le son (`1e2a44f`), `-nosound` | 1429 | 123 |
  | FM sur le thread du vCPU (`b4b124f`), avec son | ≈ 1583 (3 passes) | 111 |
  | FM sur le thread audio, `-nosound` | 1457 | 121 |
  | FM sur le thread audio, avec son | 1521 | 116 |

  Gain du déplacement : ≈ 4 % (1583 → 1521). L'écart restant avec `-nosound` (≈ 4 %) vient surtout du travail de l'invité lui-même : DMX mélange les effets en logiciel dans ses IRQ, comme sur une vraie machine. Celui avec la version d'avant le son (≈ 2 %) est le coût fixe des nouveaux périphériques à chaque sortie de VM.

### Étape 8 : SVGA (VBE 2.0), terminée le 2026-11-13

- **Framebuffer de 4 Mo** dans le memfd de la VRAM, après les régions VGA (`src/vga.rs`). **Fenêtre par banques** : la vue de 64 Ko en A0000 pointe sur la banque choisie (`Access::Bank`, toujours natif). **Framebuffer linéaire** : un slot KVM fixe à `E0000000h` (`src/kvm.rs`, `map_lfb`), mappé une fois pour toutes ; accès natifs, en mode protégé et avec pagination.
- **INT 10h 4Fxxh** (`src/bios.rs`) : 00h (bloc VBE 2.0 de 512 octets si « VBE2 »), 01h, 02h (bit 14 : LFB, bit 15 : sans effacement), 03h, 05h, 06h, 07h, 08h (DAC fixe à 6 bits), 09h ; 04h et 0Ah absents. Modes 100h, 101h, 103h, 105h (8 bits), 110h–118h (15, 16, 24 bits). Fonction de fenêtre en ROM (F000:E8D0, appel lointain vers 4F05h), chaînes et liste des modes en ROM. Rendu 8 bits par la palette, 15/16/24 bits en couleurs directes, début d'affichage et ligne logique.
- **`--vbe1`** : carte VESA 1.2, banques seules, sans LFB (cartes de 1994-1995 ; repli de compatibilité).
- **PAT de l'invité en write-back** : sous AMD (NPT), la PAT de l'invité s'applique ; CWSDPMI mappe le LFB en non caché (PCD), et Quake en 640×480 tombait à 17,7 i/s. Avec une PAT tout en write-back (le Pentium MMX annoncé n'en a pas, Intel l'ignore) : 627 i/s.
- **Critère tenu** :
  - **Quake 1.06 en VESA** (LFB ; Quake exige VBE 2.0) : `+vid_mode 12` (640×480) et `14` (1024×768). Timedemo : 627 i/s en 640×480, 349 en 1024×768 (288 en 320×200 VGA).
  - **Duke Nukem 3D 1.3D shareware** (archive.org `3dduke13`, MD5 `04e4ca70…`, `~/Dev/dos-progs/duke3d/c/DUKE3D`) en 640×480 : par banques avec `--vbe1` (≈ 2 500 changements de banque par seconde), par le LFB et 4F07h en VBE 2.0. Image correcte dans les deux cas.
- **Compatibilité gagnée en route** (installeur de Duke) : `COMMAND.COM` virtuel — INT 21h 4300h le dit présent, l'EXEC de « COMMAND.COM /C commande » exécute une commande interne ou lance le programme à sa place (`system()` des bibliothèques C) ; commande interne `REN` ; le shell lance un programme désigné par son chemin. `--trace-int` affiche le nom de fichier (DS:DX) des fonctions DOS qui en prennent un, et la ligne de commande des EXEC.
- Tests : `asm/tests/vbe.S` (bloc d'information, mode 101h, banques par 4F05h et par la fonction de fenêtre), `vbe_direct_color_rendering` ; 62 tests.
- Restent hors critère : INT 10h 1Bh et EFh (demandés par l'installeur de Duke, non gérés, sans conséquence) ; `.BAT` et COMMAND.COM interactif.

### Étape 9 : confort d'utilisation, en cours (2026-11-13)

- **Projet renommé Bare86** (crate, binaire `target/release/bare86`, répertoire `~/Dev/bare86`, titre de la fenêtre, nom OEM VBE). `--help` en anglais ; les autres messages seront traduits plus tard.
- **Fenêtre** : Alt+Entrée (plein écran), Ctrl+F5 (capture `bare86_NNN.png` dans le répertoire courant), Ctrl+F11 / Ctrl+Maj+F11 (vitesse, plus lente / plus rapide : 3, 6, 12, 25, 50, 100, 200, 400 MIPS ou sans bridage, affichée dans le titre ; Ctrl+F12 en synonyme de « plus vite », pris par KDE). Essai du propriétaire du projet : Alt+Entrée, Ctrl+F5, Ctrl+F11 fonctionnent, Ctrl+F10 ou bouton central (souris). Les touches relâchées d'un raccourci ne vont pas à l'invité. `--fullscreen`. Au clavier scripté : `{speed:N}`, `{speed:max}`.
- **Bridage corrigé** : le signal du compteur d'instructions pouvait tomber hors de KVM_RUN (pendant le service d'une sortie de VM) ; il n'interrompait alors rien, le compteur n'était plus réarmé et le bridage s'éteignait. Doom, qui passe l'essentiel de son temps en sorties, n'était donc jamais bridé. Le gestionnaire lève maintenant un drapeau consulté à chaque tour de boucle. Doom `-timedemo` : 1445 tics sans bridage, 2948 à 50 MIPS, 12230 à 12 MIPS ; banc de l'étape 0 inchangé (±0,2 %).
- **Fichier de configuration** (`src/config.rs`) : `option = valeur`, comme `--option valeur` (`true` : option seule) ; `program`, `args` ; chemins relatifs au fichier. `--config F`, sinon `./bare86.conf`, sinon `~/.config/bare86/bare86.conf`. La ligne de commande l'emporte.
- **Joystick** (`src/gameport.rs`) : port 201h (quatre monostables de 24 µs à 1,1 ms selon la position, boutons), INT 15h AH=84h ; premier joystick SDL, ouvert avant le vCPU (les jeux le détectent au lancement). Keen 4 détecte « Joystick #1 » avec une manette Logitech Precision. Coût : Keen mesure la manche en continu (≈ 120 000 lectures de 201h par seconde, chacune une sortie de VM) ; piste : raccourcir les durées.
- **Démarrage d'un vrai DOS** (choix du propriétaire du projet : avant le CD-ROM). `src/disk.rs` : images de disquette (géométrie d'après la taille : 160 Ko à 2,88 Mo) et de disque dur (géométrie d'après la fin CHS de la première partition, sinon 16 têtes × 63 secteurs) ; INT 13h 00h–05h, 08h, 15h, 16h, 41h, 42h, 43h, 48h ; écritures directes dans le fichier (lecture seule si le fichier l'est) ; table des paramètres de disquette en ROM à F000:EFC7 (vecteur 1Eh) ; BDA : disquettes dans le mot d'équipement, nombre de disques durs en 475h.
  - Options `--fda`, `--fdb`, `--hda`, `--boot` (A: si une disquette est présente, sinon C:), aussi en `bare86.conf`. `--fda` répété : jeu de disquettes, Ctrl+F4 ou `{disk}` passe à la suivante (INT 13h AH=16h signale le changement une fois).
  - `Pc::boot_disk` : DOS de l'hôte effacé (`Dos::call` court-circuité), mémoire basse mise à zéro, secteur d'amorce en 0000:7C00, DL = lecteur. XMS, EMS, souris (INT 33h), VBE, son, joystick restent servis par l'hôte.
  - **Redémarrage** : INT 19h, saut en F000:FFF0 (devenu `jmp F000:0190`, le stub d'INT 19h), 8042 (impulsion FEh, bit 0 du port de sortie), port 92h bit 0, triple faute. `Pc::reboot` : registres système de la mise sous tension (`Backend::reset_cpu`), BIOS réinstallé, XMS, EMS, PIC, PIT remis à neuf, A20 fermée, amorçage sur le même lecteur. Sous le DOS de l'hôte, un redémarrage termine la session.
  - Clavier scripté : `{until:TEXTE}` attend que l'écran texte affiche TEXTE (indispensable pour enchaîner l'installateur ; les délais fixes étaient trop fragiles). `--limit` affiche aussi l'écran texte.
  - **Installation (2026-11-13)** : FreeDOS 1.3 Floppy Edition officielle (`~/Dev/dos-progs/freedos`, SHA-256 vérifié) démarre sur A:, l'installateur partitionne C: (image de 256 Mo), redémarre, formate, transfère le système et extrait les archives SLICER des 5 disquettes ; FreeDOS démarre ensuite depuis C: (`~/Dev/dos-progs/freedos/fd13-hd.img`), invite `C:\>`, VER, MEM. Script d'installation : attentes sur « Select from Menu », « proceed [Y,N] » (lettre + Entrée : vchoice attend la confirmation), « reboot now », « format your drive », « install FreeDOS 1.3. », « Press a key », puis « FREEDOS.024 in drive », 048, 072, 096.
  - **XMS et EMS de l'hôte retirées sous un vrai DOS** : le noyau FreeDOS ne renvoie pas l'INT 2Fh au BIOS (sur un vrai PC, le BIOS ne la sert pas), donc 4310h n'atteignait jamais la XMS de l'hôte ; et JEMMEX refusait de se charger (« An EMM is already installed »). Au démarrage sur image, le vecteur 67h pointe sur un stub ordinaire, INT 2Fh/67h ne sont plus servis par l'hôte et INT 15h AH=88h rend toute la mémoire étendue. JEMMEX se charge alors (mode V86 sous KVM) : HMA, UMB 181 Ko, EMS 8 Mo, 629 Ko conventionnels libres.
  - **Critère TENU (2026-11-13)** : Doom 1.9 shareware copié par `mcopy -s -i image@@32256` (mtools) dans l'image FreeDOS (`~/Dev/dos-progs/freedos/fd13-hd-doom.img`) ; `doom -timedemo demo1` sous FreeDOS + JEMMEX + DOS/4GW (VCPI) : 5026 tics en 1699 (≈ 104 i/s, contre ≈ 124 sous le DOS de l'hôte). Écart : sous JEMMEX le processeur est en V86 puis paginé, ce qui écarte l'interpréteur (`interpreted: 0`, 609 000 sorties MMIO).
  - **Défauts corrigés ensuite (2026-11-13)** :
    - *Ports IDE* : le pilote de CD-ROM de FreeDOS sondait les quatre canaux, qui rendaient FFh (bus flottant, BSY levé) : attente du délai maximal sur chacun. Canaux absents à 00h (`io::NoIde`) : 6 lectures par canal au lieu de ≈ 265 000, démarrage 13,9 → 9,5 s (dont 5 s de menu).
    - *Processeur à l'invite* : FreeCOM scrute INT 16h AH=01h en boucle. Après 20 ms d'appels rapprochés (< 200 µs) sans touche ni autre service entre eux, chaque appel cède 1 ms (`Pc::idle_poll`) : 43 % → 5 %. Le noyau FreeDOS scrute aussi le clavier (Ctrl-C) à chaque appel DOS : tout autre service (disque, texte) remet la série à zéro, sans quoi l'extraction était ralentie d'1 ms par appel.
    - *Installation* : 9 min → 20 s (sondage IDE à chaque démarrage de l'installateur).
    - *Souris* : le noyau FreeDOS remet le vecteur 33h sur un IRET ; un vrai DOS veut un pilote et du matériel. Interface PS/2 du BIOS : INT 15h AH=C2h (00h–07h), bit 2 du mot d'équipement, IRQ 12 ouverte à l'activation, INT 74h en ROM (F000:E940) qui demande un paquet à l'hôte (F000:E948) et appelle la routine du pilote (déposée en F000:E938) avec état, X, Y, 0. CuteMouse : « Installed at PS/2 port » ; Doom : « Mouse: detected » ; mouvement vérifié par INT 33h AX=3 (`{mouse:dx,dy,boutons}` au clavier scripté).
    - *Interpréteur en mode paginé* : traduction par la table de pages de l'invité (4 Ko, 4 Mo avec PSE ; présence, écriture, U/S, CR0.WP ; bits A/D posés), contrôlée dans `lin()` avant tout effet (faute de page : rendue au processeur), décodage octet par octet à travers les pages. Doom sous FreeDOS + JEMMEX : 1690 → ≈ 1525 tics (≈ 115 i/s), au niveau du DOS de l'hôte (1520). Le mode V86 reste au processeur (instructions sensibles à IOPL à traiter).
    - INT 10h AX=1104h/1114h (police ROM 8×16) sans effet ; APM (INT 15h AH=53h) et BIOS PCI (INT 1Ah AH=B1h) absents.
    - *Interpréteur en mode V86* (jeux en mode réel sous JEMMEX) : adressage du mode réel, CPL 3 (droits des pages) ; INT, et CLI/STI/PUSHF/POPF/IRET (sensibles à IOPL), rendus au processeur ; IN/OUT et HLT l'étaient déjà. Sous FreeDOS + JEMMEX, 110 s depuis le lancement : Wolf3D (démo 3D) 431 → 804 images, sorties MMIO 5,3 M → 167 000 ; Keen 4 sorties MMIO 913 000 → 243 000 (images inchangées : calé sur la synchro). Rendu vérifié par captures ; difftest inchangé.
    - Vu en passant : Wolf3D scrute le port du joystick (201h, ≈ 3 M lectures) sur son écran « Press a key » ; Keen 4 ne voit ni EMS ni XMS sous FreeDOS (« Expanded 0 Kbytes »), comme déjà sous le DOS de l'hôte.
  - **Reste** : polices chargées par les programmes (rendu texte d'après le plan 2, comme le vrai matériel : DISPLAY/MODE CON CP, jeux en mode texte) et polices 8×8/8×14 (AX=1102h, 1101h) ; FDISK écrit la fin de partition en cylindre 520 au lieu de 519 (le noyau corrige d'après le LBA : sans conséquence).
- **Reste ensuite**, dans l'ordre (choix du propriétaire du projet : le réseau avant le CD-ROM) :
  1. **Réseau — FAIT (2026-11-14)**, critère tenu : sous FreeDOS, packet driver Crynwr `NE2000 0x60 3 0x300`, puis mTCP : DHCP (10.0.2.15 obtenu), PING de la passerelle (4/4), HTGET d'un fichier de 1 Mo servi par l'hôte (SHA-1 identique, ≈ 3,7 s soit ≈ 280 Ko/s, écriture sur disque comprise) et de http://example.com/ (DNS et TCP vers Internet).
     - `--net` : carte NE2000 ISA (`src/ne2000.rs`, DP8390 calqué sur la NE2000 de QEMU : PROM MAC 52:54:00:12:34:56 + signature « WW », anneau de réception 32 Ko, DMA distant octet/mot/double mot, filtre diffusion/multicast/promiscuité, trames en attente quand l'anneau est plein, IRQ sur front montant) en 300h, IRQ 3 ; `--net-fwd tcp:HÔTE:INVITÉ` (ou `udp:`) redirige un port de 127.0.0.1 vers l'invité.
     - NAT : libslirp (celle de QEMU, BSD-3, paquet système, liée par `pkg-config`) dans un thread dédié (`src/net/`), derrière un shim C (`slirp_shim.c` : configuration 10.0.2.0/24, minuteries, poll) ; réveil par un tube, trames vers l'invité par un canal relevé à chaque tour de la boucle du vCPU. Piège : `SlirpConfig.version` = 6, sinon libslirp appelle le rappel obsolète `register_poll_fd` (nul : plantage à l'ouverture du premier socket UDP).
     - Correctif général : les instructions de chaîne sur les ports (`rep insw`/`outsw`) arrivent en une seule sortie KVM de plusieurs accès ; la taille unitaire, que kvm-ioctls ne rend pas, est relue dans `kvm_run` et le bus d'E/S découpe (`Exit::IoIn/IoOut { size }`).
     - Tests : `ne2000::tests` (PROM, émission, réception dans l'anneau, anneau plein), `net::tests::slirp_answers_dhcp` (DISCOVER → OFFER, sans réseau réel).
     - Essai : image `~/Dev/dos-progs/net/fd13-net.img` (FreeDOS + `C:\NET` : NE2000.COM, mTCP, `NETUP.BAT` = pilote + DHCP) ; `bare86 --hda COPIE.img --boot --net --keys "{until:C:\>}C:\NET\NETUP{enter}{until:Settings written}C:\NET\PING 10.0.2.2{enter}"`. Paquets FreeDOS d'origine (`crynwr.zip`, `mtcp.zip`, dépôt 1.3/net d'ibiblio) dans `~/Dev/dos-progs/net/pkgs`.
     - **Débit (2026-11-14)** : HTGET de 4 Mo vers `NUL`, mesuré par `tools/netbench.sh` (trace `BARE86_NETLOG`, première → dernière trame de données), épinglé sur les cœurs rapides :

       | Étape | Débit | Sorties de VM par trame |
       |---|---|---|
       | Trames relevées à chaque tour de boucle | 1,6 Mo/s | ≈ 60 |
       | + réveil du vCPU par le thread réseau | 3,4 Mo/s | ≈ 60 |
       | + rafales de ports dans l'interpréteur | 4,8 Mo/s | ≈ 20 |
       | + INS/OUTS et PIC servis en rafale | **9,5 Mo/s** | ≈ 8 |

       (Une vraie NE2000 ISA plafonne vers 1 Mo/s.) Aucune retransmission ; la limite restante est la fenêtre TCP de mTCP (≈ 9–16 Ko) sur l'aller-retour.
       - *Réveil* : à l'arrivée de trames, le thread réseau écrit dans un eventfd (attente en HLT par `ppoll`, précise à la ns, au lieu d'un sommeil) et envoie au thread du vCPU le signal du battement (sortie de `KVM_RUN`) ; un seul réveil tant que le vCPU n'a pas relevé les trames.
       - *Rafales de ports* : le pilote Crynwr intercale `push ax / in al,61h / pop ax` (temporisation ISA) entre ses accès aux registres : ≈ 24 lectures de 61h par trame, une sortie chacune (≈ 4 µs). Une sortie sur un port de la carte lance désormais une rafale de l'interpréteur (comme la VGA), qui sert IN/OUT/INS/OUTS sur les ports déclarés (`IoBus::burst_on` : carte et déclencheurs ; 61h, PIC servis sans déclencher, pour que les boucles d'attente des jeux sur 61h ne passent pas dans l'interpréteur). Droits respectés : CPL ≤ IOPL, sinon (et toujours en V86, cas de JEMMEX) bitmap d'E/S du TSS (`Cpu::tr_base/tr_limit`) ; refus → le processeur exécute l'instruction. Test `port_io_in_bursts` ; difftest inchangé.
       - *Processeur hybride de l'hôte* (Ryzen AI 9 HX 370 : CPU 0–3 et 12–15 Zen 5 à 5,2 GHz, les 16 autres Zen 5c à 3,3 GHz) : une session ordonnancée sur un cœur Zen 5c tourne ≈ 1,8× moins vite en entier (5,3 contre 9,5 Mo/s ici). Pour toute mesure : `taskset -c 0-3`, sinon des passes « lentes » apparaissent au hasard (sans doute une part du bruit des mesures de Doom).
     - **Reste** : IPX des jeux (Doom, Duke3D) par un pont (pilote ODI/IPX sur le packet driver, ou IPX émulé côté hôte façon DOSBox) ; TAP en option.
  2. **CD-ROM** : MSCDEX en HLE, images ISO/CUE, pistes audio. Sous un vrai DOS, un MSCDEX de l'hôte ne serait pas joignable (le noyau FreeDOS ne renvoie pas l'INT 2Fh au BIOS) : y ajouter un lecteur ATAPI émulé, servi par les pilotes du DOS (UDVD2, OAKCDROM) et SHSUCDX/MSCDEX, utile aussi à Windows.
     - **Palier 1 — lecteur ATAPI : FAIT (2026-11-14)**. `--cdrom IMAGE.iso` (clé `cdrom` du fichier de configuration) : lecteur ATAPI en maître du canal IDE secondaire (`src/ide.rs` : 170h–177h, 376h, IRQ 15, PIO seulement, commandes achevées sur-le-champ, signature EB14h, IDENTIFY PACKET DEVICE, PACKET avec morceaux bornés par la limite d'octets et une impulsion d'IRQ par bloc, SRST, esclave absent lu à 0) ; commandes MMC dans `src/cdrom.rs` (TEST UNIT READY, REQUEST SENSE, INQUIRY, MODE SENSE 6/10 pages 01h/0Eh/2Ah, MODE SELECT absorbé, READ CAPACITY, READ 10/12, READ CD utile ou brut (2352 reconstruit), READ TOC formats 0 et 1 en LBA/MSF, READ SUB-CHANNEL, GET CONFIGURATION, GET EVENT STATUS, MECHANISM STATUS ; UNIT ATTENTION après changement de disque, NOT READY sans disque). Critère tenu : sous FreeDOS, sans aucune configuration, CDROM.BAT charge UDVD2 + SHSUCDX au démarrage, « CD-ROM configured as D: », DIR (nom de volume, sous-répertoire) et COPY d'un fichier de 5 Mo identique à l'original ; ≈ 2 sorties de VM par secteur. Tests `cdrom::tests`, `ide::tests`.
     - **Palier 2 — MSCDEX en HLE : FAIT (2026-11-14)**. Sous le DOS intégré, `--cdrom` monte aussi l'image en D: :
       - `src/iso9660.rs` : descripteur primaire, répertoires (noms 8.3 sans « ;1 », fichiers cachés omis), dates, étiquette ;
       - `src/dos/vfs.rs` : couche d'accès aux fichiers des lecteurs, arborescence hôte ou image en lecture seule ; les chemins restent des `PathBuf` (racine virtuelle unique « \0cd:D:n », que l'hôte ne peut pas ouvrir ; unique car le registre est global au processus : deux machines de tests parallèles s'écrasaient leurs images), `Meta` et `VFile` remplacent `fs::Metadata` et `fs::File` dans tout le DOS ; écritures refusées (accès refusé) ;
       - `src/dos/mscdex.rs` : INT 2Fh AX=1500h (présence), 1501h (liste des pilotes), 1502h–1504h (fichiers du descripteur), 1505h (VTOC), 1508h (lecture absolue), 150Bh, 150Ch (version 2.23), 150Dh, 150Eh, 1510h (requêtes : IOCTL d'entrée 0, 1, 4, 6–12, 15 ; IOCTL de sortie, ouverture, fermeture, recherche, READ LONG cuit, arrêt et reprise audio) ; en-tête « MSCD001 » en 0060:00C0, sans code derrière (les programmes qui appellent le pilote directement au lieu de 1510h ne sont pas servis) ;
       - étiquette de volume par FINDFIRST (attribut 08h) et dans `DIR` du shell.
       Test `mscdex_on_iso` (`asm/tests/mscdex.S`, ISO fabriquée par xorriso, sauté sans xorriso).
     - **Palier 3 — CUE/BIN et pistes audio : FAIT (2026-11-14)**.
       - `Disc::open` accepte une feuille CUE : plusieurs FILE, pistes MODE1/2048, MODE1/2352 (données à +16), MODE2/2336, MODE2/2352 (+24), AUDIO ; INDEX 01 = début de piste (INDEX 00 et prégap présent dans le fichier restent à la piste précédente), PREGAP hors fichier (silence) ; fichier retrouvé sans tenir compte de la casse.
       - `Player` (lecture audio) partagé par le lecteur ATAPI et MSCDEX, rendu dans le mixeur (rééchantillonnage linéaire 44,1 → 48 kHz, ajouté à la SB16). ATAPI : PLAY AUDIO (10), (12), MSF (début FFFFFFh = position courante), PAUSE/RESUME, STOP PLAY, éjection = arrêt, READ SUB-CHANNEL avec état (11h lecture, 12h pause, 13h terminé une fois, 15h) et positions absolue/relative. MSCDEX : PLAY (132, HSG ou Red Book), STOP (133 : pause, puis arrêt), RESUME (136), bit « occupé » pendant la lecture, IOCTL 10/11 (disque et pistes audio), 12 (sous-canal Q, piste en BCD), 15 (état audio). Fin de lecture au-delà du disque : bornée plutôt que refusée (indulgence pour les jeux). Quand le mixeur saute un retard, la position du disque avance quand même (`Player::skip`).
       - Tests `cue_tracks_and_audio` (deux fichiers, casse différente, PREGAP, INDEX 00, TOC, refus de lire de la musique sur une piste de données et inversement, rendu, pause, fin), `mscdex_audio_tracks` (`asm/tests/cdaudio.S` : la position avance pendant la lecture). Essais : FreeDOS lit une CUE/BIN en D: par ses pilotes ; une sinusoïde de 1 kHz jouée par MSCDEX ressort à ≈ 1 kHz dans le WAV.
     - Reste (CD-ROM) : changement de disque à chaud (`Drive::insert` prêt ; raccourci, MSCDEX IOCTL 9) ; volume des canaux audio (MODE SELECT page 0Eh, IOCTL de sortie 3) ignoré ; appels directs au pilote MSCD001 (sans 1510h) non servis ; images CHD/MDS non lues ; un vrai jeu sur CD à essayer (aucun sous la main).
  3. **Windows 95/98** (comme rust-dos ; copie de Windows fournie par le propriétaire du projet). Démarrage depuis un vrai DOS (acquis), puis WIN.COM et le VMM32 en mode protégé : c'est Windows qui pilote alors le matériel, avec ses propres pilotes. À prévoir :
     - **contrôleur IDE/ATA** (ports 1F0h–1F7h, 3F6h, IRQ 14) sur l'image disque : sans lui, Windows reste en « mode de compatibilité MS-DOS » pour les disques (INT 13h en V86, lent mais fonctionnel, premier palier possible) ;
     - **lecteur ATAPI** pour l'installation depuis le CD (ou copie préalable des CAB sur le disque) ;
     - **8042 complet** : canal auxiliaire (souris PS/2 par les ports 60h/64h et l'IRQ 12, plus seulement par INT 15h C2h), car Windows lit le contrôleur directement ;
     - **affichage** : VGA standard 640×480×16 d'abord (pilote de Windows), puis un pilote VBE (VBEMP, licence à vérifier) pour les modes SVGA ; le rendu planaire passe par l'interpréteur ;
     - **son** : SB16 déjà conforme (pilote de Windows, configuration manuelle 220h/5/1/5) ;
     - pièges connus sur processeur moderne : « Windows protection error » (boucles de calibrage trop rapides, correctif FIX95CPU ou bridage à `--mips`), limite de mémoire de 98 (≤ 512 Mo, sans souci avec nos 16–64 Mo), TLB et pagination du VMM sous KVM à surveiller ;
     - critère : bureau de Windows 95 puis 98, souris, un jeu DirectX/WinG simple ; ensuite mode SVGA et son.
     - **questions à trancher avant de commencer** (2026-11-14, propriétaire du projet) :
       - *se procurer Windows légalement* : voir si l'installation officielle (95/98) peut encore s'obtenir par une voie légale (licence et support d'origine d'occasion, abonnement MSDN/Visual Studio donnant accès aux anciennes versions, etc.), plutôt qu'une image de provenance douteuse ;
       - *Windows « intégré » à la rust-dos ou Windows historique ?* rust-dos fournit son propre Windows : KERNEL32, USER32, GDI32, DirectX et les autres DLL système réécrits dans l'émulateur, comme son DOS (aucune installation, aucune image, aucun fichier Windows : on tape le nom du jeu à l'invite DOS). Comparer les deux voies (couverture des jeux, effort, fidélité, légalité, cohérence avec la virtualisation KVM de Bare86) avant de choisir.

### Point de reprise (fin de session du 2026-11-13)

État : dernier commit « Interpréteur en mode V86 », 68 tests. Étape 9, volet « vrai DOS » terminé : FreeDOS 1.3 installé par son installateur officiel (20 s), démarré depuis C:, Doom sous FreeDOS + JEMMEX ≈ 115 i/s, Wolf3D et Keen 4 accélérés en V86. Images : `~/Dev/dos-progs/freedos/fd13-hd.img` (FreeDOS seul), `fd13-hd-doom.img` (+ `C:\DOOMS`) ; toujours travailler sur une copie (Bare86 écrit dans l'image). Copie de fichiers : `mcopy -s -i IMAGE@@32256 SOURCE ::/` (mtools installé).

Essais commodes (clavier scripté) : `bare86 --hda COPIE.img --boot --keys "{until:C:\>}cd \DOOMS{enter}doom -timedemo demo1{enter}{until:timed}" --wav /dev/null` ; `--limit S` affiche registres, compteurs et écran ; `--shot F.png` capture à la fin. En zsh, passer les listes d'options par un tableau (`F=(…)` puis `"${F[@]}"`).

À faire, dans l'ordre convenu :
1. ~~Réseau~~ : FAIT le 2026-11-14 (`--net`, NE2000 + libslirp ; DHCP, ping, HTGET sous FreeDOS + mTCP ; 9,5 Mo/s après réveil du vCPU et rafales de ports). Reste IPX pour les jeux, plus tard. Mesures : épingler sur les cœurs Zen 5 (`taskset -c 0-3`).
2. ~~CD-ROM~~ : FAIT le 2026-11-14 (lecteur ATAPI pour les vrais DOS, MSCDEX en HLE pour le DOS intégré, ISO et CUE/BIN, pistes audio mixées). Restes mineurs listés à l'étape 9 ; à essayer avec un vrai jeu sur CD.
3. **Windows 95/98**, après l'interface de paramétrage (4) (comme rust-dos) : IDE/ATA, ATAPI, 8042 auxiliaire, VGA puis VBE ; détail dans « Reste ensuite » (étape 9). Trancher d'abord : obtention légale de l'installation officielle, et Windows historique ou Windows intégré façon rust-dos.
4. **Interface de paramétrage** (idée du propriétaire du projet, 2026-11-14 ; **avant Windows**, décision du propriétaire du projet ; jamais imposée : `bare86` sans argument lance toujours directement le DOS intégré, accès rapide et expérimentation inchangés) : menu en mode texte façon DOS, dessiné dans la fenêtre SDL3 par notre rendu (police Terminus), utilisable au clavier et à la manette, en plein écran, portable tel quel sous Windows ; aucune boîte à outils graphique. Deux temps :
   - *menu en jeu* : **FAIT (2026-11-14)**. Ctrl+F1 dans la fenêtre (`src/menu.rs`, indépendant de SDL et testé ; intégration dans `src/sdl.rs`) : disquette de A: et CD choisis dans un explorateur de fichiers (images filtrées par type, image en cours présélectionnée, vérifiées avant envoi), éjection du CD, vitesse (←→), plein écran, capture, page des raccourcis, quitter. Boîte en mode texte façon DOS (police intégrée, palette CGA) par-dessus l'image assombrie, agrandie d'un facteur entier ; l'invité continue de tourner, sans clavier (les relâchements de touches déjà enfoncées lui parviennent : pas de touche coincée) ; souris libérée. Côté machine : `InputEvent::InsertFloppy` / `InsertCd`, `Disks::insert_floppy`, `Pc::change_cd` (disque partagé, musique arrêtée, lecteur ATAPI → UNIT ATTENTION, D: du DOS intégré remonté → MSCDEX IOCTL 9) ; demandés à l'invite du shell intégré, ils sont appliqués avant la commande. Clavier scripté : `{fda:IMAGE}`, `{cd:IMAGE}`, `{cd:}`. Tests `menu::tests`, `cd_change_reaches_dos` (et son contrôle négatif).
     - Limite constatée : sous FreeDOS, UDVD2 + SHSUCDX ne voient pas le changement à chaud (aucune commande n'atteint le lecteur après l'échange, même 4 s plus tard ; une impulsion d'IRQ 15 n'y change rien) ; même problème connu sous QEMU. À réessayer avec d'autres pilotes (OAKCDROM, MSCDEX d'un MS-DOS). Après éjection sous le DOS intégré, D: paraît vide au lieu de « lecteur non prêt ».
   - *lanceur*, sur demande seulement (`--launcher`) : profils par jeu, réglage des disques, du CD, du réseau, de la mémoire et de la vitesse, enregistrés dans le format existant de `bare86.conf` (pas de nouveau format).
5. **Choix du type de cœur** (idée du propriétaire du projet, 2026-11-14 ; priorité basse, `taskset` en attendant) : `--cores perf|eco|auto` (et clé de configuration), qui épingle le seul thread du vCPU (`sched_setaffinity`) sur les cœurs rapides ou économes, `auto` par défaut (laisser faire le système). Type de cœur détecté de façon générique par `cpuinfo_max_freq` (AMD Zen 5/5c, Intel P/E) ; sous Windows, `GetSystemCpuSetInformation`. Intérêt : moins la vitesse (même un cœur Zen 5c dépasse de loin un processeur de l'époque, et les jeux sont calés sur la minuterie ou bridés) que la *stabilité* (pas de migration en cours de session, qui fausserait les boucles de calibrage), l'autonomie sur portable et des mesures reproductibles.
6. Restes mineurs : polices chargées par les programmes et 8×8/8×14 (rendu texte d'après le plan 2) ; Keen 4 sans EMS/XMS et rendu de 160 px de large ; Wolf3D scrute 201h sur « Press a key » ; essais en fenêtre par le propriétaire du projet (souris sous FreeDOS, Ctrl+F4).

### L'interpréteur est-il encore nécessaire ? (2026-11-13)

Question du propriétaire du projet : KVM fournit un vCPU, l'interpréteur x86 sert-il encore ? Il ne sert plus qu'à un cas : les accès à la fenêtre VGA qu'aucun mappage ne peut exécuter, parce que la carte les transforme (write modes 1–3, latches, set/reset, masque de bits, plusieurs plans à la fois, lecture d'un autre plan). Sans lui, chacun coûte une sortie de VM (≈ 4,5 µs) ; avec lui, la première sortie est suivie d'une rafale côté hôte (≈ 73 ns par accès) jusqu'à ce que le code ne touche plus la VRAM.

Outils ajoutés : `--no-interp` (une sortie de VM par accès émulé, KVM terminant lui-même l'instruction ; test `vga_window_without_interpreter` : mêmes résultats) ; `page_flips` dans `--stats` (écritures du registre 0Ch du CRTC, une par changement de page) ; banc `asm/bench/vrambench.S`, chronométré dans l'invité par RDTSC calibré sur les tics du BIOS (`cargo test --release vram_bench -- --ignored --nocapture`).

**Banc** (µs par répétition, deux passes ; les valeurs absolues varient du simple au double d'une passe à l'autre, les rapports sont stables) :

| Motif | Avec | Sans | Rapport |
|---|---|---|---|
| 1. EGA 0Dh, copie écran → écran par latches, 8000 o (défilement de Keen) | 205–328 | 51 430–89 738 | ×250–275 |
| 2. EGA 0Dh, remplissage 4 plans par set/reset, 8000 o | 167–273 | 41 898–58 821 | ×215–250 |
| 3. Mode X, 320 colonnes d'un plan (Doom, Wolfenstein) | 2 275–3 689 | 2 374–3 422 | ≈ 1 (mappé) |
| 4. Mode X, remplissage 4 plans, 16 000 o | 219–375 | 41 742–58 437 | ×155–190 |
| 5. Mode 13h, 64 000 o | 13–22 | 11–30 | natif |

**Correction trouvée en route** : le motif 3 n'était jamais mappé (18,2 ms par image au lieu de 1,6). Le seuil de 600 accès émulés avant mappage repartait de zéro à chaque changement de configuration, donc à chaque colonne. Les accès s'additionnent maintenant tant que l'on reste en mode « un plan » ; le compte ne repart à zéro qu'en le quittant, ce qui garde au seuil son rôle face aux alternances coûteuses (mappage ↔ émulation = suppression et recréation d'un slot KVM).

**Jeux** :

| Jeu | Avec | Sans |
|---|---|---|
| Doom, `-timedemo demo1 -nosound` (tics, 3 passes) | 1450–1456 | 1553–1559 (+7 %) |
| Wolfenstein, 60 s d'attente + démo | 36 863 sorties MMIO, 167 changements de page | 1 562 419 sorties (≈ 12 % du temps), 120 changements de page (−28 %) |
| Commander Keen 4, 60 s de démo | 266 268 sorties MMIO, 1 324 changements de page | 1 958 196 sorties (≈ 16 % du temps), 1 265 (−4 %) |

**Conclusion** : l'interpréteur reste nécessaire, mais seulement comme accélérateur de la logique VGA (×150 à ×275 sur les motifs émulés). Les jeux VGA 256 couleurs modernes (Doom, Quake) s'en passeraient presque (+7 % pour Doom, rien pour Quake) ; les jeux EGA et mode X qui recopient par latches ou écrivent plusieurs plans à la fois (Keen, Wolfenstein, la plupart des jeux 16 couleurs) perdraient de 12 à 16 % du temps en sorties sur ces démos légères, et bien plus sur un jeu chargé. Ce n'est pas un émulateur de processeur : il rend la main au vCPU après 256 instructions sans accès à la VRAM, ou sur tout ce qu'il ne gère pas.

Relevé en route (Commander Keen 4 v1.4 shareware, archive.org `Keen4e-sw`, MD5 `278e73fe…`, `~/Dev/dos-progs/keen4/c`) : l'image du mode EGA de Keen n'est rendue que sur 160 points de large (géométrie lue dans le CRTC, à revoir), et Keen ne voit pas l'EMS (« Expanded 0 Kbytes » : détection probable par ouverture du périphérique `EMMXXXX0` par INT 21h).

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
- [rust-dos](https://github.com/dividebysandwich/rust-dos) (GPL-2.0-or-later, compatible) : émulateur DOS / Win9x en Rust, avec émulation du CPU. Ses crates autonomes (`crates/` : dma 8237, soundblaster SB 1.x→SB16/AWE32, opl / nuked-opl3 en Rust, mpu401, gus, tandy, lpt-dac, MT-32/SC-55) sont candidates à une dépendance git figée à l'étape 7, après vérification de leur accès mémoire pour le DMA et de leur cadencement. Son cœur (`src/` : int21, int10, mcb, exec MZ, fat, dpmi, kbc, VGA/SVGA) dépend de son CPU : référence de comportement aux étapes 2 à 6 (reprise ponctuelle en citant la source), ainsi que `games.rs`, `game_fixes.rs` et `autospeed.rs`. Ne pas reprendre ses polices `IBM_VGA_*.bin` (copies de ROM probables).
- [Nuked-OPL3](https://github.com/nukeykt/Nuked-OPL3) et [ymfm](https://github.com/aaronsgiles/ymfm) : émulation OPL3.
