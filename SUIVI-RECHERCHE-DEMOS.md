# Démo aux cochons : pistes (fichier non suivi, ne pas commiter)

## Ce qu'on cherche (souvenirs de l'utilisateur, ~30 ans)
- Démo DOS, pas française, petite (pas une 64k), VGA, pixel art.
- Cochons stylisés ; « don't smoke » écrit sur une image.
- Musique « chip tune » + samples (dont un sample « don't smoke »).
- Pas de « pig » dans le titre (à peu près sûr), fin 90 – début 2000.

## Où on a cherché
Sources : CD PC Team 27 → 63 (sept. 1997 – déc. 2000), section demoscène extraite
dans `/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team/dos-demos/pcteam_cdNN/`
(32 CD ; sans section demoscène : 41, 44, 45, 51, 61). Script : `CD PC Team/fetch_cds.py`.

Fait (2026-11-19), sans résultat :
- Texte (nfo/diz/txt, zips imbriqués) : pig, pork, swine, oink, cochon, porc, schwein, smok…
  → rien de net (« pig » n'apparaît que comme pseudo ou nom de musique).
- Noms d'instruments / octets des modules et exécutables : « don't smoke », oink, piggy…
  → rien de net (fausses pistes : off_by_d.zip, LABEL-SK.ZIP, i_want_t.zip).
- OCR (tesseract, ×3) sur les 4 363 images des archives : seul « smokeSome » (Plastic,
  CD 53), démo Windows, pas la bonne.
- Tri des images VGA par proportion de rose : surtout des visages, aucun cochon.
- Candidats déjà récupérés et écartés par l'utilisateur : `~/Dev/dos-progs/scene/pigs/`
  (flypigz, pig, piglet, pigs, smoke2, smokesome).

## Piste restante : balayage visuel (OUTILS PRÊTS le 2026-11-21, pas encore lancé)
Lancer chaque démo dans bare86 sans fenêtre, capturer des images à intervalles réguliers,
puis regarder des planches (soi-même, et/ou par un modèle de vision local).
- Si on retrouve la démo, l'image « don't smoke » est sans doute dans un fichier de
  données propriétaire ou compressé (invisible aux recherches de texte/OCR sur archives) :
  seul le balayage visuel peut la montrer.

### Ce qui est prêt (dépôt bare86, commits 319246b et 195c3f7)
- `bare86 --shot F.png --shot-every S` : captures `F-SSSS.png` toutes les S secondes.
- `tools/survey.py` (lire son en-tête) : pour chaque archive (.zip, .rar, .lzh…) ou
  dossier contenant un .exe/.com sous `--src` : décompression dans un dossier de travail
  propre (données non fiables), choix de l'exécutable DOS principal (nom proche de
  l'archive, sinon le plus gros, hors setup/install…), lancement sans fenêtre
  (`--no-realtime --gus --mips 200 --limit N`), captures, mesure du son, planche par
  élément (`sheets/ID.png`), résultats (`results.jsonl`), planches de revue de 12 éléments
  (`review/page-NNN.png` + `review/index.md`). Reprise automatique (éléments faits sautés).
  Programmes Windows (PE) notés `win32` et non lancés (sauf `--win32`). Si la sortie parle
  de DPMI, second essai avec `--dpmi` (hôte DPMI de HX).
- Éléments « figés » (captures presque identiques : écran de configuration qui attend
  une touche) : repérés (`static`), et relancés seuls par
  `--retry-static '{wait:3000}{enter}{wait:2000}{enter}{wait:2000}{enter}{wait:900000}'`
  (Entrée choisit en général l'option proposée ; essayer aussi d'autres touches : 1, y…).
  La revue garde le dernier résultat de chaque élément.
- `tools/survey_ask.py` : pose une question sur chaque planche à un modèle de vision local
  (API compatible OpenAI de Lemonade, http://localhost:13305/api/v1), réponses dans
  `asks.jsonl`, `--grep MOT` pour filtrer. **Testé avec `Gemma-4-12B-it-GGUF`, installé**
  (6,8 Go dans `/var/lib/lemonade`, disque interne ; téléchargé le 2026-11-21 par une
  requête de test, voir les pièges) : descriptions justes (il lit le texte des écrans),
  ≈ 6 s par planche, soit ≈ 2 h 30 – 3 h pour tout le balayage. Gemma 4 raisonne avant
  de répondre : l'outil coupe ce raisonnement (`enable_thinking: false`) et garde de la
  place pour la réponse. Question structurée recommandée (première ligne `PIG: yes/no`)
  pour filtrer sans attraper « there are no pigs ». Machine : 27 Go de RAM.
- Essai fait : `CD PC Team/scan-essai/` (4 archives du CD 40, abduct98) : la mécanique
  fonctionne ; 3 sur 4 s'arrêtaient sur un écran de configuration (choix du mode vidéo,
  de la carte son) : d'où `--retry-static`.

### Comment lancer (proposé)
```sh
PC="/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team"
cd ~/Dev/bare86 && cargo build --release
# 1. calibrer sur un CD (≈ 20 archives), regarder review/page-*.png
python3 -I tools/survey.py --src "$PC/dos-demos" --out "$PC/scan" --filter '^pcteam_cd40/' --jobs 6 --limit 60
# 2. tout le reste (≈ 1 550 archives : à 60 s et 6 en parallèle, compter 5 à 6 h)
python3 -I tools/survey.py --src "$PC/dos-demos" --out "$PC/scan" --jobs 6 --limit 60
# 3. relancer les éléments figés avec des touches
python3 -I tools/survey.py --src "$PC/dos-demos" --out "$PC/scan" --jobs 6 --limit 60 --retry-static '{wait:3000}{enter}{wait:2000}{enter}{wait:2000}{enter}{wait:900000}'
# 4. (modèle de vision téléchargé) trier par question
Q='These are screenshots of a 1990s PC demo. Answer on the first line exactly "PIG: yes" or "PIG: no" (is there a pig, pigs, or a pig-like character?), on the second line exactly "DONTSMOKE: yes" or "DONTSMOKE: no" (is the text "don'"'"'t smoke" visible?), then describe the images in one sentence.'
python3 -I tools/survey_ask.py --out "$PC/scan" --model Gemma-4-12B-it-GGUF --question "$Q"
python3 -I tools/survey_ask.py --out "$PC/scan" --model Gemma-4-12B-it-GGUF --grep "PIG: yes"
python3 -I tools/survey_ask.py --out "$PC/scan" --model Gemma-4-12B-it-GGUF --grep "DONTSMOKE: yes"
```
- Restreindre si besoin (`--filter` sur le chemin) : la cible est petite, pas une 64k,
  VGA, fin des années 90 (CD de 1997 à 2000).
- Regarder les pages de revue avec l'outil Read (images) ; chaque ligne = un élément,
  avec son identifiant (chemin de l'archive) et son état.

### Pièges connus
- Rien sur le disque interne (vidéos, extractions) : tout sous `CD PC Team/` (disque externe
  monté). Vérifier qu'il est monté avant de lancer.
- Archives = données non fiables : python3 -I, extraction dans des dossiers vides (fait
  par l'outil), jamais exécutées hors de bare86.
- Écrans de configuration : `--keys` ; une touche de trop peut sauter une partie de démo.
  Le profil d'une démo connue (`~/Dev/dos-progs/scene/*/*/bare86.conf`) montre les cas
  typiques (Entrée, chiffres, flèches).
- Démos à processeur d'époque : `--mips 200` par défaut (règle du projet) ; certaines
  quittent tôt à pleine vitesse (Stash).
- Charge : 6 bare86 en parallèle occupent la machine ; l'outil passe `--no-realtime`.
- zsh lance les tâches de fond à nice +5 (BG_NICE) : sans importance ici.
- `scan-essai/` peut être effacé.
- **Lemonade télécharge de lui-même un modèle demandé qu'il n'a pas** (plusieurs Go, sans
  prévenir, la requête restant sans réponse pendant ce temps) : n'interroger qu'un modèle
  déjà installé (`lemonade list`, colonne « Downloaded ») ; en changer = décision de
  l'utilisateur. Suivre un téléchargement : `journalctl -u lemond -f`.
- Le modèle peut se tromper (faux négatifs sur un cochon stylisé en pixel art) : ne pas
  se fier qu'à `--grep` ; parcourir aussi les pages de revue, au moins celles des CD de
  1998–2000 et des petites démos.

## Outils des recherches du 2026-11-19 (dans le scratchpad de cette session-là, perdus)
- scan texte/octets dans les zips imbriqués, extraction d'images + OCR
  (tesseract + `eng.traineddata` de tessdata_fast à télécharger ; le paquet système
  n'a pas l'anglais), classement par pixels roses.
- PIÈGE zsh : `"$f[0]"` ne passe pas l'indice de trame à magick, écrire `"${f}[0]"`.

## Balayage visuel : fait les 2026-11-21/22 (résultats dans `CD PC Team/scan/`) — pas retrouvé
- 1 673 éléments : 839 `no-exe` (musique/graphismes), 794 lancés ou tentés : ok 459 + 336 figés,
  exit 174, win32 128 (non lancés), erreurs 11. Deux passes `--retry-static` (Entrée ; puis
  Entrée, 1, y). Gemma-4-12B sur 715 planches (`asks.jsonl`, sauvegarde `asks.jsonl.bak`).
- Pistes écartées après examen : FL-DOP2 (hippopotames adultes, 64k), lyvuoto (cochon 3D, Assembly 98),
  BSR-0011 (drone belge), blomma (« don't smoke » halluciné, 64k 3D), cr-rdp, FELIX (chien/belette).
- Pages de revue regardées à la main : 3 à 6 seulement (CD 27–28) ; les 54 autres non.
- Trous : ~300 éléments encore figés sur écran de config (MIDAS/IMS/PAiN/choix carte son),
  174 `exit` (dont 24 `INT 21h AH=FFh`, 9 `INT 10h AH=11h` non gérés par Bare86), 128 Windows.

## Suite (2026-11-22) : QA des sorties précoces, nouvelle passe sur les démos qui ne tournaient pas
- Correctifs Bare86 (commités) : amorce DOS/4G à recouvrement (DOS/32A direct), modes VBE
  320x240 15/16/24 et 320x200x8, interface VBE 2.0 protégée (4F0Ah). Correctifs survey :
  `--screen` (second essai `--dpmi`), arguments pris dans un .BAT, `DOS4GW PROG.EXE` d'un BAT.
- Relance des 138 éléments `exit` : ~125 tournent maintenant (résultats `CD PC Team/scan-qa/`
  et `scan-qa2/`, planches interrogées avec Gemma : aucun cochon, faux positif « don't smoke » :
  FG_FURDO, parodie Pantene hongroise, vérifiée sur toute la durée).
- Reste hors balayage : ~300 démos encore figées sur écran de configuration, 128 programmes Windows.

## État au 2026-11-22 (fin de journée) — à reprendre ce soir (modèle : Sonnet)
Décisions de l'utilisateur : la démo ne sera PAS dans les pages de revue, ni dans les GFX
(graphiques stand-alone) des CD, ni dans les disk mags : ne plus y chercher.

Fait aujourd'hui sur les « figés » : 144 éléments encore figés (le « 300 » comptait des doublons)
relancés avec le binaire actuel et 6 Entrées espacées (`CD PC Team/scan-cfg/`) :
13 tournent, 48 quittent, 83 restent figés (67 ok + 15 exit + 1 error). Gemma sur les 144 planches :
aucun « PIG: yes » ni « DONTSMOKE: yes » (`scan-cfg/asks.jsonl`). Rien vérifié à la main.

À faire, dans l'ordre :
1. Les 83 figés de `scan-cfg` : les regrouper par type d'écran (`results.jsonl`, champ `screen`,
   `static: true`), voir leurs captures (`scan-cfg/shots/ID/f.png`), puis essayer d'autres touches
   (y, 1, espace, flèches, chiffres selon l'écran) avec `--retry-static` (agit sur `--out scan-cfg`).
   Types vus : MIDAS (« Accept »), IMS SETUP, PAiN/play32, « Sound (y/n)? », choix de mode vidéo
   (« Nr? »), PMODE/W, DOS/32A + choix de mode, écrans « loading… please wait ».
   Puis Gemma sur les planches refaites (supprimer d'abord leurs lignes de `asks.jsonl`).
2. Démos de 2001 et 2002 : à télécharger et tester (sources à choisir : scene.org, pouet.net,
   CD d'autres magazines). Même chaîne : `tools/survey.py` puis `survey_ask.py`.

Pièges de la session (déjà dans la mémoire) :
- Lancer les longs traitements avec `run_in_background` (tâche suivie), PAS `nohup … &` : un
  processus détaché n'empêche pas la mise en veille du PC. En plus, entourer d'un
  `systemd-inhibit --what=sleep:idle` pour les très longs lots.
- Gemma-4-12B-it-GGUF seul modèle autorisé (pas de téléchargement Lemonade sans l'accord de l'utilisateur).
- Guillemets : mettre la question de `survey_ask.py` dans un script `.sh`, pas dans `sh -c`.
- Résultats sur le disque externe uniquement ; vérifier qu'il est monté.

## MÉTHODE DEPUIS LE 2026-11-22 (remplace les balayages de 60 s) — CD par CD
Constat de l'utilisateur : 60 s de démo et 12 captures par démo ne couvraient qu'une infime partie
(531 démos coupées à 60 s). Nouvelle chaîne, un CD (= un mois de PC Team) à la fois :
- `tools/sweep.py` (pilote) → `tools/survey.py --full` (captures toutes les 2 s, ne garde que les
  changements de scène, jusqu'à 10 min, arrêt si écran figé 60 s, 48 scènes au plus réparties sur
  toute la durée, planches de 6) → `tools/survey_ask.py` (Gemma sur chaque planche) → bilan.
- Progression : `CD PC Team/scan-full/progress.json` ; `python3 -I tools/sweep.py --src "$PC/dos-demos"
  --out "$PC/scan-full" --items "$PC/scan-full/full-items.txt" --status`. Un CD à la fois : `--one`.
  Lancer en tâche suivie + `systemd-inhibit --what=sleep:idle` (jamais `nohup &`).
- Liste des 629 éléments (ok ou exit, hors figés/win32/no-exe) : `scan-full/full-items.txt`.
- Faits : CD 27 (35 éléments) → 2 « à regarder », deux faux positifs (SMEGMA = une vache, pas la
  démo ; GAME100K_6 = « eclipse », « don't smoke » halluciné). Les 83 démos figées (`scan-cfg/`) ne
  sont pas dans la liste : à traiter à part.
- Anciens dossiers `scan`, `scan-qa`, `scan-qa2` : images supprimées, `results.jsonl`/`asks.jsonl` gardés.
- Limites connues : le seuil de changement de scène (5/255 sur vignettes 32×20) peut rater un cochon sur
  fond identique ; Gemma peut halluciner « don't smoke » et rater un cochon stylisé (revoir les planches).

## ÉTAT À LA FIN DE SESSION (2026-11-22, ~19h) — À REPRENDRE (Sonnet)
Constat de l'utilisateur (juste) : les balayages précédents étaient très incomplets (une capture, écrans noirs,
erreurs). CAUSE : presque toutes les démos attendent une touche / un choix (son, mode vidéo, port). Nouvelle chaîne :
- Cibles = démos + intros 64k seulement : `tools/targets.py --src "$PC/dos-demos" > scan-full/targets.txt`
  (508 éléments ; CD 40, 52, 55, 63 sans catégorie : à trier à la main).
- `tools/survey.py --probe 25 --profiles "$PC/profiles.json" --full --every 2 --idle 60 --limit 600` : sonde jusqu'à 7
  scripts de touches (`KEY_CANDIDATES`) par démo, mémorise le meilleur dans `CD PC Team/profiles.json`, puis capture
  toutes les scènes. Un profil existant est réutilisé tel quel (modifiable à la main : `keys`, `opts`).
- CD 27 refait (`scan-cd27f/`) : 21 démos sur 28 avec 23 à 155 scènes. Planches de contrôle : voir ci-dessous.
  Interrogation de Gemma sur `scan-cd27f` PAS encore faite (`tools/survey_ask.py`, planches `ID.png`, `ID.2.png`…).
- Restent à régler à la main pour le CD 27 (profils à écrire dans profiles.json) :
  * CYC_OOM : touches `{wait:3000}2{wait:1500}4{wait:900000}` (GUS, port 240h) — à vérifier.
  * U_PLSMU (et MATRIX, même « Useless Sound System ») : `{wait:3000}{enter}{wait:2000}{enter}{wait:3000}1{wait:3000}{enter}{wait:900000}` MARCHE.
  * INVIT : mode `1`+Entrée puis écran IMS (Indoor Music System) qui ne démarre PAS sur Entrée, même en « quiet »
    ni en maintenant la touche : BOGUE BARE86 PROBABLE, bloque 30+ démos (IMS « return:start ») ; les flèches marchent
    (touches E0), Entrée non. IMS ne passe pas par INT 16h (accroche INT 9 / port 60h). À diagnostiquer
    (`--trace-io`, BARE86_KEYLOG=1, scancode 1Ch make/break vs flèches E0).
  * MISG (écran noir, 1 scène), FELLOW (MIDAS hardware failure, FELLOW.CFG d'auteur), 64K_5 LowRes'97 (logo puis
    moitié basse noire, 2 scènes), 64K_2 WIPE_N64 (3 scènes : noir/rouge), SPID (7 scènes puis sortie à 145 s).
- Message MIDAS « Unable to synchronize to display refresh » : le rafraîchissement est correct (14,27 ms ; en mode 3 le
  compteur PIT décroît de 2 par tic) ; cause NON trouvée ; Entrée suffit à passer.
- Dune : pas de son car DUNE.BAT lance `duneprg ENG VGA EMS 386` sans code son ; avec `SDB` ou `ADL` ça sonne.
- À FAIRE PLUS TARD (accord de l'utilisateur, après CD 27) : interpréteur de .BAT dans le shell intégré (`src/dos/shell.rs`).
- Outil de test rapide : scratchpad `tk.sh ITEM "TOUCHES" SECONDES` (perdu entre sessions : refaire avec survey.py).

## ANOMALIES À CORRIGER (liste tenue par l'utilisateur)
- **Dune** (`~/Dev/dos-progs/dune`, `./DUNEPRG.EXE ENG VGA EMS 386 SDB`) : Bare86 crée des threads qui consomment
  100 % de CPU (observé par l'utilisateur, 2026-11-22). À mesurer (`top -H`), probablement des boucles d'attente
  (scrutation sans sommeil : vga_poll / pit_poll / idle_poll, ou threads audio / SB).
- **IMS (Indoor Music System)** : Entrée ne lance pas le programme sur l'écran de configuration (flèches OK) ; bloque 30+ démos.
- **`.BAT`** : pas d'interpréteur de lots dans le shell intégré (`program = DUNE.BAT` ne marche pas).
- Avertissement MIDAS « Unable to synchronize to display refresh » : cause non trouvée.

## POINTS À VÉRIFIER AU PROCHAIN PASSAGE (remarques de l'utilisateur, 2026-11-22, soir)
- Gemma à passer sur `scan-cd27f` (planches `ID.png`, `ID.2.png`…) en début de reprise.
- Le nombre de scènes par démo paraît encore faible : le détecteur de scène (seuil 5/255 sur vignettes 32×20) sous-estime
  peut-être ; à CONFRONTER À UNE PRISE VIDÉO (`bare86 --video F.mkv`, 70 images/s, ffmpeg) d'une démo connue, en
  comptant ce qu'un humain voit comme scènes (cf. tools/compare.py).
- Certains dossiers de résultats sont étonnamment VIDES (shots/ ou sheets/) : à auditer (statuts, `kept`, `shots` vides
  dans `scan-cd27f/results.jsonl`, éléments sans planche) avant de faire confiance au balayage.

## Journal de la session du 2026-11-22 (soir) — fiabilité de l'échantillon, avant Gemma
Objectif : un échantillon fiable avant toute interrogation de Gemma (trop peu de captures par démo).

### Étape 1 — causes des échantillons trop pauvres (faite)
- **SPID** (« error loading graphic data, file 'CRACK   .DAT' ») : bogue Bare86, noms DOS avec blancs
  de fin (`NOM   .EXT`) refusés alors que le DOS les compare au remplissage du nom de répertoire.
  Corrigé dans `src/dos/fs.rs` (`to_83`). SPID : 7 → 43 scènes, démo complète.
- **Sonde de touches** (`survey.py --probe`) : un programme qui rendait la main avant la fin de l'essai
  valait -1, donc une démo courte et bonne perdait contre un écran bloqué. Corrigé (-1 seulement si
  moins de 2 scènes). Trois scripts ajoutés (`2`, `1` seuls ; séquence Useless Sound System).
- **MIDAS « Unable to synchronize to display refresh »** : simple avertissement qui attend une touche
  (Entrée suffit) ; il n'était pas retenu à cause du scoring ci-dessus.
- **Outils** : `tools/audit.py` (drapeaux FEW, ERR, EARLY, STATIC, TEXT, CPU) ; `survey.py` mesure le CPU
  (échantillon toutes les 0,5 s : `cpu`, `cpu_peak`, `cpu_peak_at`, `cpu_hot`).
- **CD 27 refait** (`scan-cd27f/`, 28 démos) : 21 démos de 23 à 295 scènes. Restent douteuses :
  FELLOW (0 scène, `FELLOW.CFG` d'auteur → « Sound Device hardware failure » ; supprimer le .CFG ou
  passer par SETUP.EXE), CYC_OOM (menu de carte son non passé), MISG (noir, INT 03h AH=4Bh non géré),
  INVIT (IMS), 64K_5 LowRes'97 (2 scènes, la démo tourne mais reste quasi statique : à confronter à une
  vidéo de référence), 64K_2 WIPE_N64 (3 scènes, écran rouge, INT 10h AH=11h non géré).
- **CPU** : à 100 % en continu : MISG (écran noir), INVIT (écran IMS), WARP (60 % du temps) ; Dune
  signalée par l'utilisateur. Pics isolés de 0,5 s (chargement) sur plusieurs 64k : normaux.
- Piège de la session : une attente `while pgrep -f "survey.py …"` se trouve elle-même (sa ligne de
  commande contient le motif) et boucle sans fin ; ne jamais attendre ainsi.
- Gemma n'est PAS encore passé sur `scan-cd27f`.

### Étape 2 — CPU à 100 % : Dune, IMS, MISG, WARP (en cours)
