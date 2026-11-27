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
- **Dune** (`~/Dev/dos-progs/dune`, `./DUNEPRG.EXE ENG VGA EMS 386 SDB`) : 100 % de CPU signalé le 2026-11-22. RÉGLÉ
  le 2026-11-23 : `INT 33h AX=3` servi en ROM échappait à l'endormissement (84 % → 9 %, commit f817c63).
- **IMS (Indoor Music System)** : ~~Entrée ne lance pas le programme~~ NON-BOGUE, élucidé le 2026-11-23 : « return:start » exige DEUX Entrées à moins de 36 tics (≈ 2 s) l'une de l'autre (premier appui = armement, le second confirme ; au-delà de 2 s l'armement est annulé). Nos sondes espaçaient les Entrées de 2 s pile. `{enter}{wait:500}{enter}` lance la démo (INVIT : « Starting vesa », scènes). Esc suit la même logique (Esc, puis Esc).
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
- **CPU** : à 100 % en continu : MISG (écran noir), INVIT (écran IMS), WARP (60 % du temps). Pics isolés de 0,5 s (chargement) sur plusieurs 64k : normaux.
- Piège de la session : une attente `while pgrep -f "survey.py …"` se trouve elle-même (sa ligne de
  commande contient le motif) et boucle sans fin ; ne jamais attendre ainsi.
- Gemma n'est PAS encore passé sur `scan-cd27f`.

### Étape 2 — CPU à 100 % : Dune, IMS, MISG, WARP (en cours)
- **IMS (INVIT) : cause du 100 % trouvée et corrigée.** L'invité scrute le clavier par `INT 16h AH=11h`
  à tampon vide ; la ROM (`asm/bios.S`, `int16`) le sert sans sortie de VM, donc l'hôte ne voyait jamais la
  boucle (et `probe_spin` ne la reconnaît pas : elle traverse des changements de mode de l'extendeur). Une
  scrutation sur 16 passe maintenant par l'hôte (compteur en 40:F0), dont `idle_poll` endort le vCPU
  (seuil d'espacement porté à 1 ms pour les appels clavier). IMS : 100 % → 15 % de cœur. L'appel
  `INT 21h AH=2Ch/2Ah` en boucle serrée est traité de la même façon. Tests : 161 verts.
  Le « bogue » d'IMS « Entrée sans effet » n'en est pas un : double Entrée à moins de 2 s requise (voir plus bas, 2026-11-23).
- **Dune** : en headless 14 %, en fenêtre le fil `vcpu` monte à ≈ 60 % pendant l'intro ; les compteurs
  (`spin_sleeps` > 10 000, attente sur 3DAh) montrent que l'attente de retour vertical est déjà endormie :
  le reste est du calcul réel sans bridage (blit vers A000h). Pas de boucle active à corriger pour l'instant.
- **MISG** (100 %, écran noir) : l'invité exécute en boucle un opcode invalide (INT 06h, ≈ 40 000/s) à
  `014D:002B`, octets `F0 6F 00 AF…` (LOCK + OUTSW) : il a sauté dans des données après son dépaqueteur
  (l'EXE est un exécutable compressé à routine de copie en tête). Cause non trouvée : dépaquetage
  faussé par une divergence de Bare86, ou démo attendant un autre environnement. `--trace-int` imprime
  désormais les octets de l'instruction fautive (INT 06h).
- **WARP** (60 % du temps au-dessus de 0,9 cœur) : pas encore examinée.
- **Outil : profileur d'adresses de l'invité** (`BARE86_PROFILE_IP=1`) : échantillonne CS:IP au battement
  (≈ 1 000 Hz) et imprime à la sortie, sur stderr, les 15 blocs de 16 octets les plus visités
  (adresse linéaire, part, compte). Pour trouver une boucle d'attente active là où on la constate :
  `cd ~/Dev/dos-progs/dune && BARE86_PROFILE_IP=1 bare86 --window …`, aller sur la scène, quitter.
  Dune, intro (15 s, headless) : 94 % dans trois blocs (0000ECE0, 0000EC70, 0000FE10) : boucles serrées.
- **Dune** : réglé (salle du trône, `INT 33h AX=3` en ROM : une scrutation sur 16 passe par l'hôte, 84 % → 9 %).

### Étape 3 — démos douteuses du CD 27 (faite en partie)
- **CYC_OOM** : réglée, 1 → 63 scènes. Menu à trois questions : carte (`2` = GUS), port (`4` = 240h),
  IRQ (`3` = entrée « 3)=7 »), sans Entrée. Profil écrit à la main dans `profiles.json`.
- **FELLOW** : réglée, 0 → 27 scènes. Le `FELLOW.CFG` d'auteur visait le port 220h (GUS en 240h d'où
  « hardware failure »). Nouveau champ de profil `prep` (étapes sur une COPIE du dossier : `remove`,
  `run` avec touches) : suppression du .CFG puis `SETUP.EXE` (5 Entrées) qui écrit la bonne config.
  Une touche pendant la démo la fait quitter : profil `{wait:900000}` seul.
- **WIPE_N64** (64K_2) : `INT 10h AX=1112` (police 8×8, texte 80×50) ajouté à Bare86 (BIOS + rendu :
  50 lignes de 8 points, police 8×16 lue une ligne sur deux) ; l'avertissement « not handled » a disparu.
  Toujours noir puis rouge uni après 20 s, avec beaucoup d'activité (13 000 écritures de palette, 777
  changements de début d'affichage, 4 M accès VRAM émulés, mode graphique en accès direct aux registres) :
  hypothèse = effet de palette ou de début d'affichage mal rendu ; NON résolu. Aucune touche n'y change rien.
- **WARP** (CPU 60 %) : pas une boucle d'attente : le profileur montre un code réparti sur de nombreux blocs
  (calcul d'effets) ; la démo est simplement lourde. Rien à corriger.
- **MISG, LowRes'97 (64K_5)** : pas retouchés (voir étapes précédentes).

### Pour la reprise (état au 2026-11-22, ~23 h)
1. CD 27 : relancer le balayage complet avec les correctifs (IMS endormi, FELLOW, CYC_OOM, 80×50) :
   `python3 -I tools/survey.py --src "$PC/dos-demos" --out "$PC/scan-cd27f" --items <liste des 28> --redo
   --probe 25 --profiles "$PC/profiles.json" --full --every 2 --idle 60 --limit 600 --jobs 4`, puis
   `python3 -I tools/audit.py "$PC/scan-cd27f"` : l'échantillon est « fiable » quand plus aucune démo n'a
   les drapeaux FEW/ERR/STATIC sans explication consignée. Restent douteuses : WIPE_N64, MISG, 64K_5, INVIT.
2. (Dune : réglé le 2026-11-23, plus rien à faire.)
3. MISG : comprendre pourquoi le dépaqueteur saute dans des données (LOCK OUTSW en 014D:002B).
4. WIPE_N64 : examiner le mode graphique (palette / début d'affichage) ; 64K_5 : comparer à une vidéo.
5. IMS : RÉSOLU le 2026-11-23, pas un bogue de Bare86 : double Entrée à moins de 2 s (`{enter}{wait:500}{enter}`).
   Méthode : trace pas à pas du code protégé de l'invité (désassemblage de la boucle de saisie : état 1 + comparaison
   de `40:6C` avec 0x24 tics). `tools/survey.py` a trois nouveaux scripts candidats ; profil INVIT corrigé.
   À FAIRE : relancer la sonde sur les ≈ 30 démos IMS (celles dont le profil retient une Entrée simple) et revoir le CD 27.
6. Ensuite seulement : Gemma sur `scan-cd27f` (`tools/survey_ask.py`), puis CD 28 et suivants avec la même
   chaîne (sonde de touches corrigée, audit, CPU), un CD à la fois, revue avant de continuer.
7. `.BAT` (interpréteur de lots) toujours à faire, après accord.

### Relance des démos IMS (2026-11-23, `scan-ims`, 17 éléments repérés par « Indoor Music System » à l'écran)
- Passées (scènes 29 à 170) : INVIT, AMB_NOBL (cd28 et cd32), MARKKU (cd28 et cd32), MACHINA, PM_FREE, img_del4, tour_by_, delir, vague_sp.
  Clé du succès : `{enter}{wait:500}{enter}` (IMS v0.6 l'affiche : « 2*return:start »), attente initiale de 3 à 8 s ; les profils
  de `profiles.json` sont corrigés (sauvegardes `profiles.json.bak`, `.bak-ims`). La sonde automatique ne les retrouvait pas
  toutes seule : les profils déjà mémorisés l'emportent (les retirer avant de relancer `--probe`).
- Passé IMS mais bloqué plus loin (nouveaux bogues, pas IMS) :
  * QD_FOCUS (Quad, BIZ97) : RÉGLÉ le 2026-11-23. Elle sonde le mode VBE 0136h (LFB) avant 0111h ; il manquait à notre
    liste : `(0x136, 320, 240, 16)` ajouté dans `VBE_MODE_LIST` (src/bios.rs). La démo tourne (logo « Focus », 320×240).
  * R_TV : n'est pas ce cas (aucun appel INT 10h : après l'écran IMS elle se termine aussitôt, silencieusement) ; à examiner ;
  * SHINE2 (MAGS) : « unexpected VM exit: InternalError » (erreur de l'hôte KVM, à reproduire) ;
  * shortcut (Takeover 98) : rendu au DOS après 15 s, sans message ;
  * rplbaker (movistar.exe, Evoke 98) : opcode invalide (INT 06h) comme MISG ;
  * rpl_star (ICING99, movie.exe) : `INT 68h AH=43h` non géré (pas IMS).

### Opcode invalide : MISG et rplbaker (2026-11-23)
- **Cause (commit 38dcde5)** : MISG (chargeur HJB + module résident) crée un PSP par INT 21h AH=55h, saute dans le module, qui
  se termine par `AX=3100h` (TSR) depuis sa propre pile. DOS rend alors la main à l'adresse de fin (PSP:0Ah) avec la PILE
  DE L'APPELANT d'AH=55h ; Bare86 restait sur la pile du fils : le `ret` du parent dépilait 0 et sautait en 014D:0000
  (données : `F0 6F` = LOCK OUTSW). Corrigé : `child_frames` (SS, SP, drapeaux) mémorisés à AH=55h, rétablis à la fin.
  Trouvé par une trace pas à pas de l'invité (tampon circulaire des 400 000 derniers CS:IP, hack non commité).
- **rplbaker** (movistar.exe) : l'opcode invalide a disparu avec ce correctif ; elle exigeait en plus un mode VBE 320×400
  en 16 bits (« too little colors in your box of crayons ») : mode `0x130` ajouté (et `0x12E` 320×400×8). Elle tourne (« baker selection »).
- **MISG** : passe l'amorce mais plus loin : (1) à `--mips 200` « Runtime error 200 » (Turbo Pascal : division par zéro, boucle
  de calibration du `Crt` trop rapide) ; à `--mips 100` pas d'erreur 200 mais (2) MIDAS : « unable to synchronize screen. please
  boot plain DOS! » (même famille que l'avertissement MIDAS « Unable to synchronize to display refresh » : calibration du
  retour vertical par le PIT, cause encore inconnue). Profil MISG : à relancer avec `opts: ["--mips","100"]` dès que (2) sera réglé.

### MIDAS / synchronisation d'écran : MISG réglée (2026-11-23, commit 61a627d)
- **Mécanisme** : la démo (Pascal + MIDAS) mesure la période du balayage : sur un front montant de 3DAh bit 3 elle réarme le
  canal 0 du PIT (`ctl 36`, compte 0), sur le front suivant elle le verrouille (`ctl 00`) et relit ; elle répète jusqu'à
  obtenir des mesures concordantes (≈ ± 4 tics ?), abandonne après 18 essais : « unable to synchronize screen. please boot
  plain DOS! ». Chaque lecture de 3DAh coûte ici une sortie de VM (≈ 10 µs) : la période mesurée variait de 45 à 90 µs
  (≈ ± 0,3 %), d'où ≈ 50 % d'échecs, au hasard.
- **Remède** (`vga::timer_now`) : quand une lecture de 3DAh voit le front montant de la synchronisation verticale avec moins de
  150 µs de retard, le PIT montre la date réelle du front pour les accès qui suivent dans les 100 µs (il en est de même sur un
  vrai PC, où la scrutation prend 1 µs). 20 essais sur 20 passent (avant : 3 sur 6). Même famille que l'avertissement
  MIDAS « Unable to synchronize to display refresh » : à revérifier sur les démos MIDAS concernées.
- **MISG** : `scan-ims` 140 scènes. Profil `opts: ["--mips", "100"]` (Turbo Pascal : erreur 200 au-dessus de ≈ 150 MIPS) ;
  `tools/survey.py` : l'option `--mips` d'un profil remplace désormais le bridage par défaut.

### SHINE#2 (diskmag, MAGS) : « unexpected VM exit: InternalError » réglé (2026-11-23)
- **Cause** : le programme (PMODE/W 1.33) appelle INT 10h AX=4F0Ah, puis calcule le pointeur de l'interface protégée VBE par
  `edx = [flags:ES] ; sar edx,10h ; shl edx,4 ; ptr = edx + DI + (mot signé lu en [edx+DI])` : le segment F000h, étendu avec son signe,
  devient FFFFF000h puis FFFF0000h, et le pointeur tombe en FFFFEC00h : sur un vrai PC, c'est la copie de la ROM du BIOS au sommet de
  l'espace de 4 Go (FFFF0000h–FFFFFFFFh = F0000h–FFFFFh). Rien n'y était mappé : la lecture rendait −1 et le saut allait en
  FFFFEBFFh (KVM : emulation failure). Trouvé en désassemblant le code de l'invité autour de la référence au pointeur (recherche des
  octets de l'adresse de la variable dans la mémoire de l'invité).
- **Remède** : slot KVM en lecture seule de 64 Ko en FFFF0000h, sur la ROM F0000h (`src/kvm.rs`, `ROM_ALIAS_SLOT`). Le magazine
  s'affiche (image « Shine 2 »). Ce n'est pas la cause de R_TV (même chargeur, autre problème : elle s'arrête toujours aussitôt).

### shortcut (Takeover 98, 3dfx, 1er prix) réglée (2026-11-23)
- **Lancement** : `dos4gw shortcut.exe` (START.BAT) ; écran IMS v0.6 (« 2*return:start ») : `{enter}{wait:500}{enter}`, et AUCUNE
  touche ensuite (toute touche pendant la démo la quitte : code 255 après 15 s dans nos premiers essais, c'était mon profil).
- **Bogue de Bare86** : `gu3dfGetInfo` / `gu3dfLoad` (chargement des textures .3DF) n'étaient pas implémentées dans
  `glide2x.ovl` (« Cannot load 3DFX texture ») : ajoutées dans `guest/glide2x/glide2x.c` (+ `glide2x.dll`, `glide2x.ovl`
  recompilés). Le fichier est lu par l'hôte (nouvelle commande `FILE_READ`, 45, `src/glide/proto.rs`, chemin DOS résolu par
  `Dos::read_file`) ; l'en-tête texte du .3DF (`3df v1.1`, format, `lod range`, `aspect ratio`) est analysé côté invité.
- **Pièges** : (1) `Gu3dfInfo.table` est une UNION de 1 024 octets (table NCC ou palette de 256 mots) : la structure fait 1 056 octets,
  `mem_required` à +41Ch ; avec une table de 112 octets, `malloc(0)` donnait « Assertion failed: tdfInfo[nr].data ». (2) Les texels
  de 16 bits des .3DF sont rangés poids fort d'abord : sans échange d'octets, textures en bruit coloré.
- **Résultat** : 24 scènes, fin de la démo à 55 s. R_TV reste à examiner (même chargeur PMODE/W que SHINE#2, s'arrête aussitôt).

### R_TV (Real TV, Evoke 97) réglée par un bridage (2026-11-23)
- **Symptôme** : après l'écran IMS (v0.5, double Entrée), la démo choisit le mode VBE 129h (320×240×32), fait 4F06h, puis revient
  aussitôt au texte, imprime ses crédits et sort avec le code 0A4h : ≈ 1 s de vie.
- **Cause** : dépendance à la VITESSE. Sans changement de code : `--mips 20/40/60` : la démo tourne (52 scènes, « real TV », bruit
  et logo) ; `--mips 80/100/200/0` : elle sort aussitôt (seuil entre 60 et 80 MIPS). Même famille que l'erreur 200 de Turbo
  Pascal (MISG, au-dessus de ≈ 150 MIPS) : un test de vitesse interne que les processeurs rapides faussent. Non élucidé plus
  avant (pas à pas lent : la démo ne quitte pas). Profil : `opts: ["--mips", "60"]`, clavier `{wait:8000}{enter}{wait:500}{enter}`.
- **Piège de méthode** : deux démos « IMS » (INVIT, R_TV…) demandent deux Entrées à moins de 2 s ; un `{enter}` isolé ne démarre pas.

### rpl_star (ICING99, movie.exe) réglée (2026-11-23) : pas un bogue de fond
- `INT 68h AH=43h not handled` n'était qu'un message inoffensif (appel de reconnaissance du hôte DPMI, `--dpmi`), non la cause.
  Le vrai défaut : `MOVIE.EXE` seul répond « No DPMI host present » (NOTE.TXT : « try dos4gw movie.exe ») ; lancée par
  `DOS4GW.EXE MOVIE.EXE` elle arrive à l'écran IMS v0.5 (double Entrée `{enter}{wait:500}{enter}`) puis joue (86 scènes, « Roger Superstar »).
- `tools/survey.py` : sur « No DPMI host present », si un extendeur (DOS4GW.EXE, DOS32A…) est dans le dossier, l'exécutable est relancé par
  lui avant de se rabattre sur `--dpmi`. Profil : `{wait:6000}{enter}{wait:500}{enter}`.

### Fichiers de commandes (.BAT) pris en charge par le shell interne (2026-11-23)
- Nouveau `src/dos/batch.rs` (+ branchements dans `src/dos/shell.rs`, `Pc::shell_loop` / `Pc::run_batch`, `main.rs`) : un `.BAT`
  donné comme programme (ligne de commande, `program = DUNE.BAT` d'un profil) ou tapé à l'invite est exécuté ligne à ligne :
  `@`, `ECHO ON/OFF`, `REM`, `SET`, `PATH`, `%0`–`%9`, `%*`, `%VAR%`, `%%`, `GOTO` et étiquettes (`:EOF`), `CALL` (fichier ou `:étiquette`),
  `IF [NOT] ERRORLEVEL | EXIST | a==b`, `FOR %%x IN (liste ou motifs) DO`, `SHIFT`, `PAUSE`, `LH`/`LOADHIGH`, `ECHO texte > fichier`.
  `ERRORLEVEL` = code de sortie du dernier programme lancé. Non pris en charge : redirections et tubes des programmes (`> NUL`),
  `CHOICE` (commande externe absente).
- Vérifié : `DUNE.BAT` (`logo`, `duneprg ENG VGA EMS 386`) lance Dune (désert à l'écran, sans mon essai précédent à la main) ; essai
  complet des constructions sur un `T.BAT` de test ; 2 tests unitaires (développement des paramètres, `SHIFT`).
- `tools/survey.py` (2026-11-23) : `bat_script(exe)` trouve le `.BAT` du dossier qui lance l'exécutable et que le shell sait exécuter (pas
  de COPY/CHOICE/tubes ; les BAT d'installation sont écartés). Il est lancé tel quel (`res["bat"]`) quand ses lignes échappent à
  `bat_args` (%1, redirections) ou qu'il lance plus d'un programme (`LOGO` avant `DUNEPRG`) ; sinon l'ancien chemin (exécutable +
  arguments lus dans le BAT) est conservé, plus simple. Vérifié : Dune (`DUNE.BAT`, 8 captures, état ok) et shortcut (inchangée).

- `COPY` ajoutée au shell (2026-11-23) : motifs, `a+b`, destination fichier ou répertoire (`SUB`, `SUB\\`, `SUB\\*.*`), options ignorées ; messages
  « n file(s) copied », « File not found », « File cannot be copied onto itself ». `tools/survey.py` ne l'écarte plus des BAT lancés.

### Balayage `--mips 60` des démos qui sortent en moins de 20 s (2026-11-23, `scan-m60` et `scan-m200`)
- Échantillon : 88 éléments « exit » en < 20 s dans les anciens balayages, rejoués avec la version actuelle à 60 puis à 200 MIPS.
- **59 ne sortent plus** : 55 à toute vitesse (corrigés par les travaux du jour : IMS, VBE, DOS…), 4 seulement à 60 MIPS. De ces
  4 : `immort` (Takeover 98) quitte aussitôt au-dessus de ≈ 70 MIPS comme R_TV (profil `--mips 60`) ; EFC_DEEP, PLS-SKID et
  ellenium durent plus de 40 s à 60 MIPS contre 13–18 s à 200 : intros cadencées par boucles, pas des pannes.
- **28 sortent toujours**. Triées à la main : outils ou jeux sans rapport (ADDHELP, FLR_HELP, STARDATE, TRON, ZOBMAN, AXIA_SW =
  auto-extractible Windows, GAME100K/1 = IPX), données manquantes dans l'archive (soepkip3 : `3txt.pcx`, matura97 :
  `matura97.dta`, mfever : `mfever.xm`), « run setup first » (SBM_NDIG, REHASH : MIDAS), et **deux vrais problèmes réglés** :
  * `moai` : `DOS/4GW fatal error (1004): syntax is DOS4GW <executable>` : à lancer par `dos4gw.exe moai.exe` : `tools/survey.py`
    le fait désormais (même retry que « No DPMI host present »).
  * `AI_OVERD` (Overdrive, Rage 97) : « KERNEL PANIC: EMM MANAGER DETECTED » : `--ems 0` (retry automatique du balayage sur
    `EMM manager / EMM386 / QEMM`) ; menu de mode (flèches : 16 bits hicolor). Profil : `--ems 0` + `{down}×4{enter}`.
- **Bogue de fond trouvé derrière AI_OVERD** : après « LOADING MUSIC », « unexpected VM exit: InternalError » : une instruction
  x87 (`fcomp dword [eax+13DCA4h]`) lit par un pointeur fou (EAX = 32A7BA00h) hors RAM ; KVM ne sait pas émuler les x87 en MMIO
  d'où l'erreur, alors qu'un PC rend FFh (bus ouvert). Remède (`src/kvm.rs`, `map_open_bus`, créneaux 26 et 27) : les plages sans
  rien (RAM+1 → framebuffer, framebuffer+4 Mo → FFFF0000h) sont mappées en lecture seule sur des pages FFh. Overdrive passe le
  point de plantage (71 scènes en 150 s). Les écritures y sont toujours ignorées (sondes de taille mémoire inchangées).
- orthicon : EXAMINÉE (2026-11-23), pas un bogue de Bare86 : l'archive ne contient que `DEMO.EXE`, le NFO et le DIZ ; la démo cherche
  `gfx\part2.tga`, `gfx\toriod.3ds`… (répertoire `gfx` absent, « you fucked it up » = fichier introuvable). Archive incomplète.
- Restent douteuses (non examinées) : PW-BANAN (DOS32, sort après
  lecture de son exe), INVALIDE/glamour/4kbugs (intros de 4 Ko qui s'arrêtent sans message), ms98/demo.zip (exige COMMAND.COM).

### WIPE_N64 (64K_2, Wired 97) : rendu du texte piloté par la VGA (2026-11-23)
- **Cause de l'« écran noir puis rouge »** : WIPE_N64 est une démo en MODE TEXTE qui n'utilise pas la mémoire B8000h : elle met la
  carte mémoire à A0000h (GC 06h = 06h), écrit caractères et attributs dans les PLANS 0 et 1 de la VGA (64 Ko chacun, 32 K cellules),
  redéfinit des polices dans le plan 2, les choisit par le registre de sélection de table de caractères (SEQ 03h), et fait défiler un
  « écran » logique de 160 colonnes (registre de décalage 13h = 50h) par l'adresse de début (CRTC 0Ch/0Dh) et le défilement fin
  (CRTC 08h). Bare86 ne rendait que le texte de B8000h avec sa police (le BDA), d'où un rouge uni.
- **Remède** : `render_text_hw` (src/render.rs), actif quand la carte mémoire est A0000h ou qu'une police est chargée dans le plan 2 :
  cellules des plans 0/1 (ou de B8000h), adresse de début, largeur logique, hauteur de cellule (CRTC 09h), 8/9 points, défilement
  fin, comparaison de ligne, curseur matériel, tables de caractères A/B (attribut bit 3) dans le plan 2 ; une table restée vide retombe
  sur la police de l'hôte. `INT 10h AX=1111h/1112h/1114h` règlent maintenant la hauteur de cellule du CRTC (`Vga::set_cell_height`).
- **État** : la démo ANIME des balayages vert sur rouge (des « wipes » faits de glyphes en points) au lieu d'un rouge fixe ; je n'ai pas
  de vidéo de référence pour confirmer que l'image est exacte (les lignes de bas d'écran restent suspectes). À confronter à une prise
  de référence si on en trouve une.

### 64K_5 LowRes'97 (intro d'invitation de Metabolic) réglée (2026-11-23)
- **Symptôme** : logo « LowRes'97 » puis moitié basse noire, image figée pendant 40 s.
- **Cause** : la démo cadence sa chronologie sur l'IRQ 8 de l'horloge temps réel (RTC à 1 024 Hz : CMOS reg B ← 42h), son gestionnaire
  incrémente un compteur puis CHAÎNE vers l'ancien vecteur INT 70h. Le BIOS d'un AT y lit le registre C du CMOS (acquittement) ; le nôtre
  n'y faisait que la fin d'interruption : la RTC, qui ne redemande une interruption qu'après lecture du registre C, n'en envoyait
  plus qu'une seule : chronologie arrêtée.
- **Remède** : `INT 70h` par défaut en ROM (`asm/bios.S` `int70`, `bios.rs` `INT70`) lit le registre C avant les deux fins d'interruption.
  43 000 IRQ 8 en 40 s ; les textes « METABOLIC INVITES YOU TO LOWRES 97 », « THE FIRST PARTY EVER HELD IN AMSTERDAM », « 31 OCT TILL 2 NOV
  THIS YEAR » défilent maintenant. Aucune vidéo de référence (introuvable), mais l'ensemble est cohérent.
- **Piste générale** : un vecteur d'IRQ par défaut qui ne fait pas ce que fait le BIOS réel (acquittement du périphérique) fige les
  programmes qui s'y chaînent ; à garder en tête pour d'autres IRQ (INT 08h, clavier, IRQ 12…).

### WARP (Area 512) examinée (2026-11-23) : rien à corriger
- Enchaîne ≈ 10 effets (serpent vert, anneaux bleus, flammes « AREA512 »…) puis, vers 70 s, passe à un écran BLANC et attend une
  touche (`INT 21h AH=0Bh` en boucle ; une touche la fait sortir proprement). Même chronologie à 60 et à 200 MIPS : pas de dépendance à
  la vitesse. Les 60 % de CPU sont du vrai calcul (boucle d'attente `jmp $+2` du blanc final comprise).
- **Effet de bord trouvé en la lançant** : au point d'attente finale, WARP parcourt l'espace d'adresses ; avec les pages « bus ouvert »
  (`map_open_bus`) la mémoire RÉSIDENTE du processus montait de 22 Mo/s (3 Go atteints) : les mêmes 2 Mo physiques mappés des
  centaines de fois sont comptés à chaque mappage. Corrigé (commit 09fee32) : `Backend::trim_open_bus`, MADV_DONTNEED de ces
  mappages toutes les secondes (54 Mo stables). Aucune mémoire réelle n'était occupée.

## ÉTAT À LA FIN DE LA SESSION DU 2026-11-23 (~20 h) — À REPRENDRE

### Fait aujourd'hui (tout commité et poussé sur `origin/master`)
Correctifs Bare86 : IMS (double Entrée), pile du fils d'AH=55h (MISG), calage du PIT sur le front de 3DAh (MIDAS/MISG), alias de la
ROM en FFFF0000h (SHINE#2), Glide `gu3dfGetInfo/Load` (shortcut), modes VBE 0136h/012Eh/0130h, `.BAT` et `COPY` dans le shell,
bus ouvert FFh au-dessus de la RAM (+ allègement de la mémoire résidente), texte VGA piloté par les registres (WIPE_N64), INT 70h
qui acquitte la RTC (LowRes'97). Outils : `survey.py` (IMS, DOS4GW, `--ems 0`, `.BAT`), `sweep.py` (`--profiles`, `--probe`).

### CD 27 (WIR97) : balayage complet refait avec la chaîne actuelle — `scan-cd27g/` (28 éléments, tous avec ≥ 9 scènes)
- Lancé par : `systemd-inhibit --what=sleep:idle python3 -I tools/sweep.py --src "$PC/dos-demos" --out "$PC/scan-cd27g"
  --items "$PC/cd27-items.txt" --profiles "$PC/profiles.json" --cds 27-27 --jobs 4 --model Gemma-4-12B-it-GGUF` (≈ 45 min, Lemonade
  sur http://localhost:13305, modèle `Gemma-4-12B-it-GGUF` déjà téléchargé). `$PC` = `/run/media/nicolasb/Seagate Portable Drive/ExtData/CD PC Team`.
- **Analyse Gemma 4 : « PIG yes » 1, « DONTSMOKE yes » 1, TOUS DEUX FAUX POSITIFS** (comme prévu sur ce CD) : BAHO planche 6 = une
  VACHE (clipart) et un personnage à épée ; 64K_6 planche 2 = lettres en particules illisibles prises pour « DON'T SMOKE ». La chaîne
  (démos qui vont jusqu'au bout, scènes distinctes, planches, Gemma, bilan) est validée ; les cibles sont sur des CD ultérieurs.
- **Ce qui reste sur CD 27** : rien d'obligatoire. Optionnel : `64K_8` (9 scènes seulement, « ok » : vérifier à la main) ; avertissements
  `INT 10h AH=1Ch` (SWEETY, U_PLSMU : sauvegarde/restauration de l'état vidéo, non gérée) et `INT 21h AH=FFh` (GMF, TUROK) ; les
  démos hors cibles du CD (GAME100K, ANSI, GFX, MULTICHA, RAYTRACE, 4K) ne sont volontairement pas balayées ; les 83 démos « figées »
  de `scan-cfg/` (profils de touches inconnus) restent à traiter à part.
- **Référence anti-régression** : `scan-cd27g/results.jsonl` (statut et nombre de scènes par démo). Après toute modification de
  l'émulateur (IRQ, BIOS, VGA…) : `cargo test --release` (163 verts) PUIS rejouer ce CD (`sweep.py` avec une autre `--out`) et comparer
  `kept` / `status` démo par démo avant de commiter.

### Pour la suite, dans l'ordre
1. **Audit des autres IRQ par défaut** (demande de l'utilisateur, à faire avec la référence ci-dessus) : comparer chaque gestionnaire de
   `asm/bios.S` (INT 08h/09h, 0Ah–0Fh, 70h–77h, 1Ch) à celui d'un BIOS d'AT (acquittement du périphérique : RTC reg C fait ; clavier
   (lecture de 60h), IRQ 12 souris, IRQ 13 FPU (port F0h), IRQ 14/15 disques : à vérifier) ; tester chaque changement isolément.
2. **CD 28 puis suivants**, un CD à la fois, revue des « à regarder » avant de continuer :
   `python3 -I tools/sweep.py --src "$PC/dos-demos" --out "$PC/scan-full" --items "$PC/scan-full/full-items.txt" --profiles "$PC/profiles.json"
   --probe 25 --cds 28-28 --one --jobs 4` (sous `systemd-inhibit`, en tâche suivie, JAMAIS `nohup &`; ne pas recompiler Bare86 pendant un balayage).
   `full-items.txt` : 629 éléments, CD 27 → 63 (CD 28 : 49 ; CD 29 : 96 ; CD 32 : 67). Les démos qui s'arrêtent en moins de 20 s : relancer à
   `--mips 60` (voir MISG, R_TV, immort).
3. Démos douteuses restantes : PW-BANAN, INVALIDE, glamour, 4kbugs ; WIPE_N64 (rendu animé mais non vérifié) ; orthicon (archive incomplète).
4. Idées : `CHOICE`/`XCOPY` et redirection `> NUL` des programmes dans le shell ; `INT 10h AH=1Ch` ; `INT 21h AH=FFh`.

## CD 28 balayé (2026-11-24) — `scan-full/` (49 éléments)
- Bilan : 39 `exit`, 9 `ok`, 1 `error`. Gemma : « PIG yes » 3 et « DONTSMOKE yes » 2, aucun net (à regarder à la main :
  HELIUM, M_STREAM, FELIX, DSS_GNSS, NGON).
- **AGENTO** : image sombre figée au-dessus de ~100 MIPS (chronologie bloquée, comme Carmageddon) ; profil `--mips 100` :
  109 scènes, fin à 272 s. Voir `profiles.json` (sauvegarde `.bak-agento`).
- **SD_COLF** (Children of the Light, version « buggy party release » de l'auteur) : triple faute avec EMS 4 Mo ; avec `--ems 0` la
  démo réclame `command.com` copié en `EASY.com` dans le dossier (version de développement) puis exécute un opcode invalide (INT 06h).
  Défaut de la démo / prérequis, pas d'émulation fautive à ce stade.
- **Appels non gérés traités** : `INT 21h AH=FFh` (silencieux), `INT 10h AH=1Ch` (DAC sauvé/restauré ; pas les registres matériels).
  Reste `INT 10h AH=6Fh` (SNS_7GFN, sonde de carte, sans effet). SNS_7GFN : durée instable (121 s à 719 s) avec ou sans ces changements.
- **Audit des IRQ par défaut** : INT 71h redirige maintenant vers INT 0Ah (carte sur « IRQ 2 » = IRQ 9 : le GUS). `sti` en tête d'INT 08h essayé
  puis retiré. Non faits (sans effet connu) : IRQ 2–7 par défaut qui masquent la ligne comme le BIOS réel, drapeau 40:8E des IRQ 14/15.
- **Non-régression CD 27** (`scan-cd27h` contre `scan-cd27g`) : aucun changement de statut ; SMEGMA est instable avec l'ancien binaire aussi.

## CD 29 balayé sans Gemma (2026-11-24) — `scan-full/`, planches dans `sheets/` (revue à la main par l'utilisateur, xnview)
- 96 éléments : 48 `exit`, 48 `ok` (beaucoup d'`ok` à ≈ 628 s = ANSI/images/1K qui n'« finissent » pas : normal).
- **BTMNSCKS** (« Batman Sucks », Gardening 97) quittait en 28 s sans rien afficher : le programme (Pascal) fait `Halt(0)` si `MIDAS.CFG`
  manque ; il faut lancer `SETUP.EXE` d'abord (MIDAS 0.50 : SB16, Entrée ×6 → « Configuration written to MIDAS.CFG »), puis la démo marche
  (logo Draculians, « BATMAN SUCKS », 40 s+). Piste générale : des démos « courtes sans image » = SETUP à lancer avant (SB16 220h).
- **MOTFIX** : pas une démo, un correctif qui ouvre `MOTIONS.EXE` (absent) : à ignorer.
- À traiter ensuite (balayage CD 29) : statiques (IMAGE_, IGYJARAS, CHAOS, BALOOON, QUESTKHN ×2), `INT 68h AH=43h` (HYPNO2, BALOOON),
  `INT 06h AH=B6h` (QD_FOCUS), `INT 03h AH=4Bh` (CORRUP, YOE-JOOK), `INT 10h AH=6Fh` (AI-MELON), peu de scènes (CARDRIVE, DPH_HIGH, TMBE,
  EFC_DEEP, EVK-4K, ONC, KMU_KANN, BLUECALX).
- `sweep.py --no-ask` : balayage sans modèle de vision.

### CD 29 : démos « statiques » examinées (2026-11-24)
- **BTMNSCKS** : profil avec `prep` (SETUP.EXE, SB16) ajouté ; `survey.py` applique maintenant `prep` aussi aux archives : 28 scènes, 195 s.
- **IMAGE!** / PARTYVER (intros 1K, 1 420 et 1 014 octets) et **CHAOS** (253 octets) : fonctionnent, s'animent ; le drapeau « static » vient de
  scènes sombres ou très semblables. La vidéo « APNI » trouvée est une intro 4K différente : sans objet pour IMAGE!.
- **BALOOON** : fonctionne (montgolfière, tunnel rouge). **IGYJARAS** (FiRG, hongrois, GUS) : presque tout noir par conception (texte gris minuscule,
  gag de la tronçonneuse vers 75 s), pas de défaut constaté.
- **QUESTKHN / KAHN.EXE** (The Quest of KaHN!, IDS) : figée à « Loading Data » : PIT corrigé (9230c07 : un compte du canal 0 figé, IRQ 0 non
  acquittée à cause d'un `cli` pendant une temporisation sur le PIT, ne repartait jamais ; repart après 100 ms). Elle va maintenant plus loin,
  puis DOS/32A s'arrête : « run-time (6004): extended memory blocks have been corrupted (#00B5EE90) » (même avec --mem 32/128). À CREUSER.
- Non-régression CD 27 après le correctif PIT : seul TUROK passe de `ok` à `exit` (30 scènes dans les deux cas).

### CD 29 : démos à appels non gérés, revues (2026-11-24)
- **HYPNO2** (Hypnoman 2, tpolm) : fonctionne en entier (≈ 270 s : moutons, tunnels, « hypnoman », greetings) ; le balayage la comptait `kept 0`
  (une seule image finale en texte, garbage après la sortie) : artefact de balayage, non élucidé (`--video` montre la démo complète).
- **AI-MELON** (88 scènes), **CORRUP** (124), **QD_FOCUS** (196), **YOE-JOOK** (299) : vont jusqu'au bout ; les avertissements
  (`INT 10h AH=6Fh`, `INT 03h AH=4Bh`, `INT 06h AH=B6h`) sont sans conséquence visible.
- **QUESTKHN** : l'erreur DOS/32A 6004 (« extended memory blocks have been corrupted ») est déterministe : identique avec SB16/Pro/ESS/GUS,
  `--mips` 60 à 400, `--vbe1`, `--dpmi`, `--mem` 16/32/128, `--ems 0` (seule l'adresse du bloc change). Une boucle de remplissage de zéros de la
  démo (0x131720, EDX = 0) écrase l'en-tête du bloc (surveillance d'écriture `BARE86_HWBP=B5EE90 BARE86_HWBP_W=1` avec `--debug`, `--no-jit`).
  Soupçon : débordement de tas de la démo (mode 12Dh 320×200×8 choisi dans notre liste VBE ?) ou bogue d'émulation non trouvé. À CREUSER.

### QUESTKHN élucidée (2026-11-24) : incompatibilité du DOS/32A de substitution, pas un défaut d'émulation
- KAHN.EXE n'embarque pas DOS4GW.EXE : Bare86 présente son DOS4GW.EXE intégré (= DOS/32A 9.1.2, seul redistribuable). Avec lui, la démo écrase
  l'en-tête d'un bloc de mémoire étendue (memset de 0xA40 octets en B5EE18 sur un bloc dont l'en-tête DOS/32A est à B5EE90) et DOS/32A
  s'arrête sur l'erreur 6004. Indépendant du son, de la vitesse, du VBE, de la taille mémoire, de la voie XMS.
- **Avec le vrai DOS/4GW 1.97** (`~/Dev/dos-progs/dos4g/DOS4GW.EXE`, copié dans le dossier de la démo) : la démo se déroule en entier (grenouille Kahn,
  titre, gemme, scènes 3D). De même sous FreeDOS (`--boot`) avec ce DOS4GW.
- Piste générale : une démo qui échoue avec « DOS/32A run-time … » = essayer le vrai DOS4GW.EXE ; la substitution DOS/32A n'a pas la
  tolérance de DOS/4GW aux débordements d'allocation. Pas de correction possible côté Bare86 sans fournir DOS/4GW (non redistribuable).

### Reste du CD 29 (à voir éventuellement plus tard)
- Démos à peu de scènes, surtout des intros courtes : CARDRIVE, DPH_HIGH, TMBE, EFC_DEEP, EVK-4K, ONC, KMU_KANN, BLUECALX (non examinées).
- HYPNO2 : `kept 0` du balayage (démo pourtant complète en `--video`), artefact non élucidé.

## CD 30 balayé sans Gemma (2026-11-24) — `scan-full/` (31 éléments)
- **SBM_NDIG** : SETUP.EXE d'abord (SB, 220h, DMA 1, IRQ 5, 22050 Hz) → `GO.BAT` (`demovt … /sh:maindemo.exe>nul`) ; le shell intégré retire maintenant les
  redirections (`>nul`, `>>`, `<`) de la ligne de commande ; profil `prep` + `run: GO.BAT` : 55 scènes.
- **X-398363** (EXTEND) : exige le mode VBE 0146h (320×200×32, numérotation SDD 5.3), ajouté ; MIDAS 1.1.0 : une Entrée ; 37 scènes.
- **HJB_NEON** (Haujobb) : MIDAS 0.6.1 ; sous `--gus` une Entrée (GUS d'office), sans `--gus` six Entrées (SB16) ; sort ensuite en silence (code 255) au bout
  de ≈ 3 s : NON RÉSOLU (2 scènes). **ATMOZ2** (Credex PI-mode) : `INT 6Ch AX=0005` non géré puis « Can not unpack samples » : NON RÉSOLU.
  **SYS4KX** (intro 4K MMX) : n'affiche qu'un texte répété : NON RÉSOLU. RC_HALL : fonctionne.
- Non-régression CD 27 (`scan-cd27j`) : statuts inchangés, seuls les nombres de scènes bougent.

### Reste du CD 30 (KO, à reprendre éventuellement plus tard)
- **HJB_NEON** : sort en silence (code 255) ≈ 3 s après la config MIDAS ; **ATMOZ2** : `INT 6Ch AX=0005` non géré puis « Can not unpack samples » ;
  **SYS4KX** : texte répété seulement (intro 4K MMX).

## CD 31 et CD 32 balayés sans Gemma (2026-11-24) — `scan-full/`
- **CD 31** (39 éléments) : IHME et MUFSUX fonctionnent ; magazines disque (`MAGS_*`) = peu de scènes, normal ; `DH97_MISC` ADDHELP / FLR_HELP / STARDATE =
  utilitaires qui s'arrêtent en 30 s. Rien à corriger apparemment.
- **CD 32** (67 éléments, 55 `exit`, 12 `ok`) : AGENTO (version ARJ) avait le défaut de vitesse du CD 28 : profil `--mips 100` ajouté, 109 scènes.
  SNS_7GFN = doublon du CD 28 (durée instable). À regarder à l'œil : RESNOW (7 scènes), PLS-SKID (court, 41 s), INTRO (ASTROS, 2 scènes),
  NN97_FST / NN97_MUL / NN97_GFX (NEITHE : peu de scènes, sans doute un magazine/galerie), RT-SPACE (4K statique), DR_5K (6 scènes en 389 s).
- Idée (2026-11-24) : site web de captures pour les nostalgiques de la démoscène PC/DOS (lien pouet.net, pas d'exécutables, charte de ~/Dev/pageperso) — non décidé.

### RESNOW (CD 32, jamm R « resurrection now / unfinished », MACGYVER.EXE) : NON RÉSOLU (2026-11-24)
- Après le titre « RESURRECTION », l'image de l'œil reste figée > 100 s alors que le vCPU calcule (boucle par pixel en 0x110423, par ligne de 320 octets).
  Seules 3 images utiles dans les planches. Menu MIDAS 0.7b1 : GUS = 3 Entrées (carte, 240h, mode) ; SB16 = 6 Entrées ; même blocage dans les deux cas.
- Écarté : carte son, `--jit`/`--no-jit`, gel du compte PIT (FREEZE_MAX = 0), `--mips` 60/133/200. Le programme mesure la vitesse : à ≤ 40 MIPS il affiche
  « you'll need p130 or such to run this correctly / read the .nfo file » et quitte ; l'auteur dit qu'il est « conçu pour la compomachine d'Assembly 97, un P133,
  jamais corrigé ». Un seul `rdtsc` dans l'EXE (0x132135), jamais exécuté. Port 300h lu ≈ 800 fois/s (sans périphérique). À creuser : ce que la boucle attend.

### Suivi visuel des balayages (2026-11-24)
- `tools/sweep_report.py` écrit `scan-full/ETAT.md` (texte : barre globale, tableau par CD, liste « à revoir ») et `scan-full/ETAT.html` (barres colorées,
  thème clair/sombre). `sweep.py` le rafraîchit à la fin de chaque CD ; à la main, y compris pendant un balayage :
  `python3 -I tools/sweep_report.py --out "$PC/scan-full" --items "$PC/scan-full/full-items.txt" --profiles "$PC/profiles.json"`.

## À REPRENDRE SI LA DÉMO AUX COCHONS N'EST PAS TROUVÉE (liste tenue au fil des CD, décision utilisateur 2026-11-24)
Démos ou intros qui ont posé problème au balayage (le plus souvent une question, un menu ou un défaut à régler, pas un défaut d'émulation).
- **CD 28** : SD_COLF (« buggy party release », exige `command.com` copié en `EASY.com`, triple faute) ; SNS_7GFN (durée instable) ; MINDTRAP/SEVEN/RETRO/FELIX (OK).
- **CD 29** : QUESTKHN (marche avec le vrai DOS4GW.EXE) ; HYPNO2 (`kept 0` du balayage, démo OK) ; à peu de scènes non examinées : CARDRIVE, DPH_HIGH, TMBE, EFC_DEEP, EVK-4K, ONC, KMU_KANN, BLUECALX.
- **CD 30** : HJB_NEON (sort en silence, code 255, ≈ 3 s après la config MIDAS) ; ATMOZ2 (`INT 6Ch AX=0005`, « Can not unpack samples ») ; SYS4KX (texte répété seulement).
- **CD 32** : RESNOW (image de l'œil figée, boucle d'attente inconnue) ; à regarder : PLS-SKID, INTRO (ASTROS), NN97_*, RT-SPACE, DR_5K.
- **CD 33** (verdicts de l'utilisateur) : XTC_ASS pose une question o/n (y/n) à laquelle il faut répondre ; XTC!PART (même famille, à vérifier) ; K_MLINK sur l'écran MIDAS qui attend Entrée
  (« Accept ») ; PW-BANAN sur un écran « DOS32 Version 3.5 unregistered » (étape à passer) ; ZOBMAN = une sorte de Pac-Man, un seul écran (normal).
  Profils de touches à écrire pour XTC_ASS (y), K_MLINK (Entrée) ; PW-BANAN à examiner.

### Reconnaître ce qui n'est pas exécutable (2026-11-24, `tools/classify.py`)
- Constat : les archives « GFX » / « M4CH » (MOV97) ne contiennent qu'une image ou un module + `CFXADV.COM` (publicité du groupe, identique partout) : le balayage
  exécutait la pub (299 scènes) et la comptait `ok`.
- Reconnaissance par le contenu : EXE = en-tête `MZ` (puis `LE`/`LX` = DOS étendu, `NE` = Windows 16 bits, `PE` = Win32) ; COM = pas d'en-tête (extension, ≤ 64 Ko) ;
  modules = `Extended Module:` (XM), `SCRM` (S3M), `IMPM` (IT), `M.K.`… à l'octet 1080 (MOD) ; images = GIF, PCX, BMP, JPEG, PNG ; un exécutable de même empreinte (SHA-1)
  dans ≥ 3 éléments de NOMS différents = fichier d'accompagnement (pub, DOS4GW…), pas l'exécutable de l'élément (les doublons d'une démo sur plusieurs CD ne comptent pas).
- Résultat (`scan-full/categories.json`, 629 éléments) : 592 exécutables (439 DOS, 144 COM, 137 DOS étendu, 19 Win32, 3 Win16 ; un élément peut en avoir plusieurs),
  20 images, 17 musiques. `ETAT.md/.html` : colonne « hors score » ; les images et la musique ne sont plus dans « à revoir ».
- Relancer : `python3 -I tools/classify.py --src "$PC/dos-demos" --items "$PC/scan-full/full-items.txt" --out "$PC/scan-full/categories.json"`.

### Rapport : moins de « à revoir » (2026-11-24)
- Constat : 110 éléments signalés sur 367, dont beaucoup sans objet : résultats PÉRIMÉS du CD 27 (ancien `survey.py` planté, balayage refait ensuite dans `scan-cd27j`),
  « peu de scènes » pour des intros 1K/4K ou des jeux (courts par nature), avertissements `[int]` seuls, démos déjà jugées.
- Corrigé dans `tools/sweep_report.py` : `overlays.txt` (dossiers de résultats plus récents qui remplacent les anciens : `scan-cd27j`), seuils selon le genre (dossiers 1KO/IN4K/4K… : < 2 scènes
  seulement), un avertissement `[int]` seul ne signale plus, un élément portant un VERDICT est écarté. `scan-full/verdicts.json` : `{élément: {"verdict": ok|reprendre|hors-sujet, "note"}}`
  (58 verdicts semés depuis les notes ci-dessus), à éditer à la main ou par moi ; section « Verdicts » dans `ETAT.md`. `classify.py` : catégorie « magazine » (dossiers MAGS…).
- Restant à revoir : surtout des éléments non examinés (voir `ETAT.md`). À REFAIRE : CD 27 `WIR97/4K` (résultat périmé).

## Balayage terminé et reprise des échecs (nuit du 2026-11-24 au 25)
- **Balayage complet** : 629 éléments (CD 27 à 63), 54 000+ scènes, ≈ 57 h d'exécution. Rapport : `scan-full/ETAT.md` / `ETAT.html`. Aucune démo aux cochons trouvée
  (Gemma n'a été utilisée que jusqu'au CD 29 ; revue à l'œil par l'utilisateur ensuite).
- **Cause commune réglée** : plusieurs démos s'arrêtaient figées à ≈ 93 s avec `INT 68h AH=43h` : programmes DJGPP (stub « go32stub ») qui cherchent un hôte DPMI
  (« no DPMI - Get csdpmi*b.zip »). Bare86 charge maintenant HDPMI32 d'office pour ces exécutables (`is_djgpp`, src/main.rs) ; `INT 68h` (sonde de l'hôte DPMI) ne
  signale plus rien. REVOLUTI, ARSE, IDLE_BY_, MAFIA, DIST, 23… sortent de la liste ; 49 éléments rejoués avec re-sondage des touches.
- **Profils posés** (`profiles.json`, sauvegardes `.bak-night*`) : NEWWOMAN (F1 = SBlaster) 61 scènes ; EUPHORIA (touche 1) 112 ; PRSC-PI (`--ems 16`, y) 88 ; K_MLINK (Entrée, MIDAS 1.x « Accept ») 132 ;
  REHASH (prep SETUP.EXE MIDAS 0.40a, 3 Entrées + `--mips 100` : Crt « Runtime error 200 »).
- **Classés hors sujet** : conteneurs d'archives imbriquées (AM98/in4k, MS98/demo), intros 4K Windows (DMA_GLMR), magazines, images, musiques (`classify.py`).
- **Reste à reprendre** (`scan-full/verdicts.json`, verdict « reprendre », avec la cause connue) : SOEPKIP3 (« File not found: 3txt.pcx »), MATURA97 (« .dta not found or corrupted »),
  ORTHICON (archive incomplète ?), MILLENIUMFEVER (« MIDAS error: File does not exist »), MINDCTRL (sonde « VESA 2.0 » de JVideo, 0 scène), PAST (« Video mode does not exist »),
  BAZAR (choix de mode VESA), DIFFERENTIAL (barres bleues), BREEZE (caractères illisibles), 4KBUGS, PW-BANAN (DOS32 3.5 quitte en silence), XTC_ASS / XTC!PART (SFX LHA : texte seulement ?),
  BEL_WAZO / ELE (menu de carte), I_WANT_T / ACIDMIES / 3DFX (MIDAS : séquence d'Entrées), GOATZILL, PRETZEL32K, RPL_CLIN (« No DPMI host present » même avec `--dpmi`),
  HJB_NEON, ATMOZ2, SYS4KX, RESNOW, QUESTKHN (vrai DOS4GW), SD_COLF, SNS_7GFN ; CD 27 `WIR97/4K` (résultat périmé).

### Piège rappelé
Les profils (`profiles.json`, hors dépôt, sauvegardes `.bak*`) l'emportent sur `--probe` : pour re-sonder une démo, retirer son profil.


## Reprise des éléments à 0 ou peu de captures (2026-11-25)
- **Plafond de 48 scènes** (`--max-frames`) : voulu, réparti sur toute la durée (pas les 48 premières).
- **Réglés** : MATURA97 (`MATURA97.CFG` d'auteur visait `e:\escape\...` : étape `prep` `sub` ajoutée à `survey.py`, DIRECTORY=DATA + Entrées MIDAS ; 127 scènes) ;
  BAZAR (Bare86 : modes VBE `0x164`/`0x1F8` = 320×240×8 ajoutés dans `VBE_MODE_LIST` ; AUCUNE touche, Entrée quitte ; 21 scènes) ;
  PW-BANAN (DOS32 3.5 s'arrête aussitôt avec la GUS : profil `opts: ["--no-gus"]`, nouvelle pseudo-option de `survey.py` ; 18 scènes ; piste Bare86 : sonde GUS 746h/24Fh/340h).
- **Hors sujet** (archives incomplètes) : SOEPKIP3 (3TXT.PCX absent), MILLENIUMFEVER (mfever.xm absent), ORTHICON (DEMO.EXE seul), TKOVER99/IN64 (AXIA_SW.EXE Windows).
- **Non résolu** : MINDCTRL (Bad Demo Engine : après l'énumération VBE, bloquée en `F000:E862`, ROM BIOS `cli ; jmp`).
- **Reste** à 1–3 scènes avec verdict « reprendre » : voir `ETAT.md` ; beaucoup sont des 4K/512 octets statiques par conception.
- Modifs de code (`src/bios.rs`, `tools/survey.py`) non commitées.

## Démos Win32 par HX (2026-11-25) — `scan-win32/`, 153 éléments
- **Chaîne** : `tools/survey.py --win32 --dll-pack runtime-pack --probe 25 --full` ; Bare86 télécharge les DLL VC6 de Microsoft à côté de HX (SHA-256, `src/runtime.rs`), fournit sa propre `glu32.dll` (`guest/glu32/`). Les démos attendent presque toutes une touche : SANS `--probe`, une seule scène noire.
- **Résultats** : 42 éléments ≥ 10 scènes (vérifiés à l'œil : 41 vraies images, `remedy00/in64/bitwise-array` toute noire sur 4 vues), 2 de 5 à 9, 109 à moins de 5.
- **Diagnostic des 109** (`low109.jsonl`, scratchpad) : 48 tournent à l'écran noir (`ptc.dll` 9, `ddraw.dll` 26, MIDAS 12, FMOD 9) ; 48 quittent sans message dont **41 sur 45 importent `bass.dll`** ;
  6 tournent non noirs ; 3 OpenGL sans erreur ; 2 `INT 06h AH=02h` (mushca, blocc_final) ; 2 GLU32 sans `gluProject` (corrigé, commit 2b15df0).
- **CORRECTION (2026-11-25, soir)** : le groupe « quitte sans message » était en partie un artefact des touches de la sonde (Entrée → sortie → décharge de la DLL → saut dans des données). Le vrai symptôme de ≈ 40 démos à `bass.dll` :
  Petite 2.3 affiche « ERROR! This file has been tampered with and MAY BE INFECTED BY A VIRUS! » au démarrage (contrôle d'intégrité) et la démo reste sur écran noir ; Petite 2.2 (`evolution`, `rpl_dun`) n'a pas ce contrôle.
  Les deux `INT 06h` (mushca, blocc_final) sont aussi des exécutables Petite : le décompresseur écrit en bas de la mémoire (adresse 0x78…0x178).
- **Ce qui est établi** : (1) `DPMILDR=1024` marche (essai avec une DLL à base 0x30000000) mais la base 0x10000000 est déjà prise par `DKRNL32.DLL` (signature `PX`) et par toutes les DLL de HX (même `ImageBase`) ;
  (2) rebaser ces DLL (relocations appliquées, script `/tmp/claude-1000/rebase.py`, bases 0x60000000 + 4 Mo × n) libère 0x10000000 : `bass.dll` s'y charge, `evolution` et `rpl_dun` ne plantent plus (mais restent noires) ;
  (3) le contrôle « tampered » échoue encore : hypothèse non vérifiée = HX ne mappe pas l'en-tête PE comme dans le fichier (doc DPMILD32 : seules les structures NT et la table d'objets sont chargées ; reste de la page effacé, `?CLEARHEADER`).
  Remède envisagé : recompiler DPMILD32 (JWasm + JWlink à compiler) pour mapper l'en-tête fidèlement. Rien de tout cela n'est dans Bare86 (essais à la main, copie de HX dans /tmp).
- **Cause BASS (première analyse, incomplète)** : `bass.dll` est empaquetée par **Petite**, sans relocation exploitable : HX la charge à une autre adresse que son `ImageBase` (0x10000000) et le code lit des globales à l'adresse absolue
  (`cmp [10026D08],0` → C0000005, « dkrnl32: fatal exit! »). Remède documenté : `DPMILDR=1024` (DLL à leur adresse préférée, via DPMI 0504h), SANS EFFET ici : la DLL reste à ≈ 0x36C000, même avec `--mem 512` et `--dpmi`. À élucider : pourquoi
  0504h à 0x10000000 échoue (HDPMI sous XMS de Bare86 ?). Piste de repli : bass.dll factice.
- **DLL sans source officielle** (msvcp50, msvcrt40, mfc40) ; wininet/dplayx : voir la discussion (DLL factices ou dossier de DLL déposées par l'utilisateur) ; Direct3D 8 : 16 programmes, à chercher chez d'autres avant d'écrire.

### Décisions du 2026-11-25 (utilisateur)
- **Périmètre élargi** : on cherche aussi parmi les démos Windows (CD 64 à 85 téléchargés ; 17 CD ont une section démo, CD 76 absent du serveur, 68/81/84/85 sans démo ; 5 ISO tronqués à la source).
  À « faire la publicité » dans le README une fois les chiffres arrêtés (pas encore fait).
- **DLL Microsoft** : téléchargement vérifié (SHA-256) comme HX, pas d'hébergement sur GitHub (licence de VC6 : redistribuable AVEC une application ; voir `Redist.txt`, non relu). Source : copie Wayback de
  `vc6redistsetup_enu.exe` (l'adresse Microsoft est morte). Fait : `src/runtime.rs` (`ensure_ms_runtime`), 6 DLL + `CRTDLL.DLL` (copie de MSVCRT) dans `HX/BIN`. Hors paquet : msvcp50, msvcrt40, mfc40, dplayx, wininet.
- **GLU32** : écrite pour Bare86 (`guest/glu32/`). Tessellateur et NURBS : exportés, sans effet.
- **HX plus récent** : examiné, rien d'utile (commits depuis la 2.23 = HDPMI et DKRNL32 seulement ; pas de DUSER32/DGDI32).
- **Direct3D 8** (16 démos) : chercher chez d'autres avant d'écrire quoi que ce soit.
- **BASS/Petite** : mis de côté (voir plus haut) ; reprendre éventuellement avec un DPMILD32 recompilé.
- **Dossier de DLL déposées par l'utilisateur / DLL factices wininet et dplayx** : à décider après mesure du besoin réel.
- Fichiers durables : `CD PC Team/runtime-pack/` (DLL de mes CD, hors dépôt), `scan-win32/diag/` (diagnostics), `tools/win32-diag/` (scripts).

## État en fin de session du 2026-11-25 (soir) — À REPRENDRE DEMAIN
**Corrigé aujourd'hui** (commits sur master) : détection PE par KERNEL32 ; modes VBE 0x164/0x1F8 ; téléchargement vérifié des DLL VC6 ; `glu32.dll` ; DAC VESA commutable 6/8 bits (`7e0b56c`) ;
test d'intégration `tests/hx_desktop.rs` (bureau de HX) ; `survey.py` : plafond adaptatif, détection de boucle, prolongation des longues démos (`c559e42`).
**Régression trouvée et corrigée** : depuis le 22/11 (interface VESA protégée 4F0Ah, commit `afba888`) le bureau de HX restait NOIR : toutes les boîtes de dialogue et fenêtres de HX étaient invisibles dans nos captures.
Bissection automatique (git worktree + script), cause = HX envoie la palette en 8 bits à la fonction protégée, notre DAC n'avait que 6 bits.
**Balayage Win32 en cours** (peut avoir fini) : `CD PC Team/scan-win32b/` (264 programmes, binaire figé `/tmp/claude-1000/bare86-sweep`, script `/tmp/claude-1000/run-win2.sh`, profils Win32 retirés de `profiles.json`, sauvegardes `.bak-win32b`
et `.bak-avant-retrait-win32`). Si interrompu : relancer le même script (il saute les éléments faits). Planches : `scan-win32b/sheets/`. Ancien passage (bureau invisible) : `scan-win32/`, à ne plus utiliser.
**Constat des 109 démos à peu de scènes, binaire corrigé** (messages enfin lisibles, regroupés par contenu) : 45 « This file has been tampered with… VIRUS » (Petite, contrôle d'intégrité) ; ~10 `DialogBoxParam/IndirectParam(...) called`
(bouchons de DUSER32, `SrcEmu/DUSER32/Dlgbox.asm`) ; ~5 DirectDraw (`SetDisplayMode FAILED`, `unsupported bits per pixel`, `Something went wrong with DDRAW`, `unknown error`) ; ~15 fonctions USER32/GDI32 absentes de HX
(CheckRadioButton, CreatePenIndirect, SetMessageQueue, GetStretchBltMode…) ; Entrée ne débloque rien (TRAUMATI → « unknown error », TRANSCEN → boîte de réglages bouchonnée).
**Décisions de l'utilisateur (soir)** : OK pour COMPILER HX (JWasm + JWlink à compiler d'abord) ; contributions probablement bienvenues chez l'auteur (le dépôt a déjà des contributions faites avec Claude) :
on décidera plus tard entre PR amont et fork. Ne pas viser trop ambitieux. Crédits de jetons presque épuisés (reset demain).
**Prochaines étapes, par ordre** : (1) vérifier que la chaîne se construit ici (JWasm, JWlink, makefiles de HX ; rien à modifier) ; (2) lire `scan-win32b` à la fin du balayage et relire les chiffres ;
(3) HX corrigé : en-tête PE fidèle (Petite), DialogBoxParam minimal (WM_INITDIALOG puis IDOK), fonctions USER32/GDI32 manquantes ; (4) DirectDraw : relever les modes demandés par les 5 démos en échec ;
(5) README : « publicité » de la prise en charge des démos Windows, une fois les chiffres arrêtés ; (6) Direct3D 8 (16 démos) : chercher un travail existant avant d'écrire ; (7) wininet/dplayx/msvcp50/msvcrt40/mfc40 : à décider.

### Compilation de HX : chaîne validée (2026-11-25, soir)
- JWasm 2.21 et JWlink 2.1 compilés depuis leurs sources (`make -f GccUnix.mak`, gcc seul). Script reproductible : `tools/hx-build/build-dpmild32.sh [DOSSIER]`
  (clones de HX, JWasm, JWlink ; copie en minuscules des sources, car JWasm est sensible à la casse sous Linux ; 3 assemblages + édition de liens, exactement les commandes de `DPMILD32.MAK`).
- **`DPMILD32.EXE` construit = IDENTIQUE octet pour octet à celui de HX 2.23** (18 246 o) : la chaîne est fiable, et le chargeur n'a pas changé depuis la 2.23.
- Reste à construire/valider : `HDPMI32`, `DUSER32` (`SrcEmu/DUSER32`, boîtes de dialogue bouchonnées dans `Dlgbox.asm`), `DGDI32` ; chaque DLL a son `.MAK`.
- Prochain pas pour Petite : modifier `PELOAD.ASM` (copie fidèle de l'en-tête PE, au lieu de `?CLEARHEADER`) et mesurer sur `taste` / `evolution`. Ne pas oublier qu'un HX modifié change la nature de la distribution (PR amont ou fork : à décider).
- Dernière modification de `survey.py` : plafond de scènes à la densité (une scène toutes les 5 s ou moins : toutes gardées, jusqu'à 300).

### Petite « tampered » : hypothèses éliminées (2026-11-25, fin de soirée)
Essais sur `taste` (`bass.dll` Petite 2.3, 24 démos partagent cette DLL) :
- **ÉLIMINÉ : relocation** (la DLL est à 0x10000000 avec HX rebasé + `DPMILDR=1024`, donc aucune relocation appliquée) : « tampered » persiste.
- **ÉLIMINÉ : sections « discardable »** (`0xE2000060` & 0x02000000 ; HX a `?DISCARDSECTION`) : en retirant le drapeau dans une copie de la DLL, « tampered » persiste (testé DLL à son adresse ET sans).
- **RESTE : en-tête PE** (le contrôle d'intégrité porterait sur l'image y compris la première page). HX ne la recopie pas fidèlement : il y met ses propres champs (`MZHDR.wCnt`, `dwStack`), place les en-têtes NT à base+0x40 et efface le reste
  (`?CLEARHEADER`, `PELOAD.ASM` ~l. 600-660 et 775-800). Si c'est la cause, aucun correctif simple : les champs internes de HX occupent l'en-tête MZ.
- **Aussi à examiner** : `?SETSECATTR` (protection des sections après chargement), `?SKIPCOMMENTS`, et ce que Petite 2.3 contrôle exactement (analyse statique du décompresseur ; la chaîne « tampered » est dans les données compressées).
- **Piste de test directe** : DPMILD32 construit (`tools/hx-build/build-dpmild32.sh`) avec une variante qui lit tout `SizeOfHeaders` du fichier dans la première page (sans toucher aux champs HX hors de la plage contrôlée), à comparer sur `taste`/`evolution`.
- Fichier de travail : `scratchpad/w11/` (volatil) ; `bass.dll.orig` = copie intacte.

## Part des démos Windows selon la période (2026-11-25) — à garder en tête pour l'hypothèse « démo tardive »
Éléments exécutables par CD (DOS = au moins un exécutable DOS/COM ; Win32 = uniquement Windows ; liste réelle des 264 éléments Win32, y compris ceux de l'ancien balayage) :

| CD | Période | DOS | Win32 | Part Win32 |
|---|---|--:|--:|--:|
| 27 à 33 | sept. 1997 → mars 1998 | 299 | 2 | 0 % |
| 34 à 45 | avr. 1998 → mars 1999 | 126 | 11 | 8 % |
| 46 à 55 | 1999 | 69 | 40 | 36 % |
| 56 à 63 | 2000 | 74 | 75 | 50 % |
| 64 à 85 | 2001 → 2002 | 58 | 136 | 70 % |

Win32 par CD : 28:1 31:1 34:2 35:3 36:4 37:1 42:1 46:1 48:2 49:3 50:8 52:1 53:17 54:4 55:4 56:13 57:9 58:6 59:21 60:12 62:10 63:4 64:6 65:23 66:6 67:6 69:6 70:6 71:10 72:3 73:16 74:5 75:6 77:10 78:5 79:6 82:16 83:6.
Lecture : la bascule vers Windows se fait fin 1998 / début 1999 ; avant, tout est DOS (le balayage DOS des CD 27 à 63 la couvre). Si la démo aux cochons est de 1999 ou après, il y a une vraie chance qu'elle soit Win32.
Réserve : une partie des démos récentes est hors de portée (Direct3D 8 : 16 éléments ; Petite : ≈ 45). Biais à éviter : `categories.json` de `scan-full` ne contient que les éléments NON Windows pour les CD 27-63.

## Démos « vides » : causes mesurées (2026-11-25, soir) — CHANTIER À FAIRE
Exemple du CD 27 (Wired 97). Ce qui est vide, et pourquoi :
1. **Archives à plusieurs exécutables dont UN SEUL est lancé** : 213 éléments sur 774 (`scan-full/results.jsonl`, champ `others`), soit **au moins 354 exécutables jamais capturés**.
   Exemple : `WIR97/4K` contient 6 intros (`deaf.exe`, `green.com`, `mc4k.com`, `no_fear!.com`, `partyver.com`, `poorguy!.com`) : seule `MC4K.COM` a été capturée (62 scènes). Gros foyers : CD 29 (46 éléments), 32 (16), 36 (16), 59 (14), 31 (13), 35, 50 (10), 28 (9).
   Cas extrêmes : `cd36/ms98/demo.zip` (12 autres, 0 scène), `cd36/tg98/demo.zip` (12 autres), `cd71/.../juggler_2001_final.zip` (12 versions d'une 4K).
   **À faire** : étendre `survey.py` pour lancer CHAQUE exécutable (un sous-élément par exécutable, ou une boucle sur `others`), puis revoir les planches.
2. **Anciennes erreurs « pick »** (survey planté, jamais refaites) : 12 éléments, tous au CD 27 (dont `WIR97/4K`, `GAME100K/3`, `MULTICHA/5`) : à relancer (corrigé depuis).
3. **Catégories qui ne sont pas des démos** : `GAME100K` (jeux de 100 Ko, souvent `INT 10h AH=1Ch` non géré, 0 scène), `MULTICHA` (musique) ; à jauger avant de s'y acharner.
Le reste du CD 27 est correct : 29 éléments sur 38 ont entre 11 et 297 scènes.

### Vivier à exploiter (chiffré le 2026-11-25) — côté DOS, scan-full
- 628 éléments exécutables DOS ; **103 à moins de 5 scènes** (hors « hors-sujet ») ; **161 éléments à plusieurs exécutables, soit 262 exécutables jamais lancés** (chiffre resserré aux éléments exécutables ; 354 en comptant tout `results.jsonl`).
- **Vivier distinct : 238 éléments DOS** (peu de scènes OU exécutables zappés), à ajouter aux 109 démos Win32 à peu de scènes et aux Win32 non lancées.
- Ordre conseillé : (1) lancer CHAQUE exécutable des éléments à plusieurs exécutables (avec les nouvelles règles de capture) ; (2) reprendre les 103 à peu de scènes (causes déjà notées dans `verdicts.json` pour beaucoup) ; (3) Win32 selon l'avancement de HX.

## Reconnaissance d'image sans jetons : CLIP à la place de Gemma (décision de l'utilisateur, 2026-11-25) — À FAIRE
Pourquoi : Gemma relit chaque image à chaque question (lent, hallucine : « don't smoke » à tort). CLIP/SigLIP calcule UNE fois un vecteur par image, puis toute requête (texte ou image) se fait en quelques secondes sur les vecteurs stockés.
- **Outil 1 `tools/survey_clip.py`** : découper les planches de `sheets/` (6 scènes, 3×2) en images, ou lire `shots/`, calculer les vecteurs (ViT-B/32 ou SigLIP, CPU ≈ 20-40 min pour ≈ 55 000 scènes ; plus rapide sur GPU), les ranger dans un fichier `.npy` + index (item, scène).
- **Outil 2 de requête** : par texte (plusieurs formulations : « a cartoon pig », « a pink pig », « pig face », « a pig with a cigarette ») ou par IMAGE DE RÉFÉRENCE (souvent plus fiable) ; CLASSEMENT par score, pas de seuil oui/non ; on regarde le haut du classement à l'œil.
- **Texte « don't smoke »** : OCR (Tesseract, ou EasyOCR/PaddleOCR) avec recherche approximative, plus fiable que tout modèle de vision (Tesseract : l'anglais `eng.traineddata` de tessdata_fast est à télécharger, le paquet système ne l'a pas).
- **Pré-tri** : part de pixels roses (déjà utilisée) avant CLIP.
- **Validation** : sur une scène où l'on sait qu'un cochon apparaît (ex. le cochon 3D de `lyvuoto`, Assembly 98, écarté comme hors sujet) avant de se fier aux scores. Limites : pixel art et 3D très stylisés de 1997 (faux positifs/négatifs), objets petits.
- **Confirmation du haut du classement** (si besoin) : détecteur à vocabulaire ouvert (OWL-ViT, Grounding DINO), plus précis mais plus lent.
- Avantage décisif : les vecteurs sont réutilisables pour toute question future (« cochon », « fumée », « personnage rose »…) sans repasser sur les images.

## Trous dans la collection des CD (vérifié le 2026-11-25)
- **CD 76** : INTROUVABLE sur le serveur (abandonware-magazines.org renvoie 404) : vrai trou entre les CD 75 et 77. À chercher ailleurs (autre miroir, archive.org, ou chez l'utilisateur).
- **CD 25 et 26** : jamais téléchargés (la plage commençait à 27) ; CD 24 téléchargé sans section démo. À tenter si besoin (période antérieure à sept. 1997).
- **CD 61** : une seule image dans l'archive, « Ludi CD 61.iso » (CD jeux) : le **CD PRO (celui des démos) semble absent de l'archive** : second vrai trou possible, à confirmer.
- **CD 45** : l'ISO PRO est vide (listage), seul le CD Ludi a un contenu : même suspicion.
- **CD 41, 44, 51, 68, 81, 84, 85** : téléchargés ; leurs dossiers « Demos » sont des démos de JEUX du commerce (CD Ludi) ou des images/musiques/logiciels (CD PRO) : pas de demoscene, ce n'est pas un trou.
- 5 ISO tronqués à la source (« Unexpected end of archive ») : CD 71, 73, 74, 82, 83 : contenu partiel.

### Recherche des CD 45, 61, 76 (2026-11-25, soir)
- **CD 45 : PAS un trou.** L'ISO PRO est une image BRUTE (secteurs de 2 352 o, Mode 1) que `7z` ne liste pas directement (d'où mon « vide » de tout à l'heure) ; une fois « cuite » (2 048 o utiles par secteur), son contenu est : Creations (Anims, Images, Musique),
  Demos = démos de LOGICIELS (AtGuard, Babylon, Callisto, eJay 2, Fex, Pager, SyGate), Divers, Hardware. Aucune demoscene. (Piège : un ISO « vide » à `7z l` peut être une image brute ; `cook()` de `fetch_cds.py` la convertit.)
- **CD 61** : l'archive du site ne contient que « Ludi CD 61.iso » (jeux) ; pas de CD PRO. Le fichier existe bien sur le serveur (200), le PRO n'a vraisemblablement jamais été déposé.
- **CD 76** : le serveur répond 302 vers la page 404 (aucune variante .7z/.rar/.iso essayée n'existe). La recherche web (archive.org, Korben, vogons) n'a rien donné de direct ; Internet Archive a une grande collection de CD-ROM de magazines (surtout PC Gamer) mais pas trouvé PC Team.
  Pistes restantes : autres miroirs d'abandonware, forums (Vogons), ou demander au contributeur du site. Respecter les droits des CD de magazine.
- **Repère de dates** (Team Palmtops est né comme supplément de PC Team n°46 = mai 1999 ; la numérotation est mensuelle, quelques numéros doubles possibles, donc ± 1 mois) :
  CD 45 ≈ avril 1999 ; CD 61 ≈ août 2000 ; CD 76 ≈ novembre 2001. À recouper avec les parties de ces mois (pouet.net : calendrier des parties) pour savoir quelles démos ces CD ont pu contenir.
  Parties à regarder (à VÉRIFIER sur pouet, non vérifié) : printemps 1999 (The Gathering est à Pâques), été 2000 (Assembly, début août), automne 2001. Les démos de ces parties peuvent se retrouver sur d'autres CD voisins (64-75 pour 2001).

### Décision (2026-11-25) : parties manquées — SEULEMENT si la démo aux cochons reste introuvable
- Les CD 61 (≈ août 2000) et 76 (≈ novembre 2001) manquent, plus les CD 25/26 (avant sept. 1997). Statistiquement peu probable que la démo aux cochons soit sur l'un d'eux (2 CD sur ≈ 60) : **ne pas s'y consacrer maintenant**.
- Si, et seulement si, la démo reste introuvable après les chantiers prioritaires (exécutables jamais lancés, peu de scènes, Windows/HX, recherche CLIP) :
  recouper les mois de ces CD avec les parties de l'époque (pouet.net, calendrier des parties), lister les démos sorties à ces dates absentes de nos CD voisins, et les chercher (pouet.net, scene.org, demozoo).
- Noté pour mémoire, on ne sait jamais.

### VÉRIFIÉ (2026-11-26) : le traducteur dynamique n'est jamais utilisé par les programmes Windows sous HX
**Résultat** : `GDITEST.EXE` et `under.exe` (Win32, OpenGL, 2204 échanges) : tous les compteurs `jit_*` à 0 par défaut et avec `--no-jit` ; même avec `--jit` (forcé), 0 instruction traduite (`jit_bursts` = 2, tout interprété). README exact, rien à corriger.
Texte d'origine :
Affirmé dans le README le 2026-11-25 (commit `4cf16b3`) d'après la lecture du code seulement (`common_base` de `src/jit/mod.rs` exige l'absence de pagination ; HDPMI active la pagination) : NON vérifié par un essai.
Test rapide : lancer `GDITEST.EXE` (ou une démo Win32 qui marche, ex. `under`) avec `--stats`, avec et sans `--no-jit`, et vérifier que les compteurs du traducteur (`jit_bursts`, `jit_engagements`, `jit_interpreted`…) sont à ZÉRO.
Si ce n'est pas le cas, corriger les deux phrases du README (fonctionnalités, ligne 54 ; limites, en bas).
Même vérification pour `--no-jit` « sans effet sur les programmes Windows ».

### README : exemple C avec Open Watcom (décision de l'utilisateur, 2026-11-25) — FAIT le 2026-11-26 (hello.com et hello.exe compilés et lancés avec Bare86)
La section « Writing DOS programs in C (gcc) » du README (≈ l. 700) deviendrait deux sections :
1. **« … in C (Open Watcom) » en premier** : `hello.c` avec `printf` ; commandes `.COM` (`wcl -q -bt=dos -mt -fe=hello.com hello.c -"l=com"`) et `.EXE` (`wcl -q -bt=dos -ms -fe=hello.exe hello.c`), exactement celles de `c/build.sh` ;
   une ligne sur l'obtention du compilateur (Open Watcom v2 : binaires Linux des versions de développement sur son dépôt GitHub ; ici construit dans `~/Dev/open-watcom-v2/rel`, variable `WATCOM`) ; mentionner sa licence (Sybase Open Watcom Public License).
2. **« … with gcc (nothing to install) »**, raccourcie : on garde `crt0.S`, `com.ld`, `dos.h` comme alternative sans installation.
**Avant d'écrire l'exemple : le compiler et le lancer avec Bare86** (non testé le 2026-11-25) ; l'exemple doit être vérifié, comme les tests de `c/tests`.

### Balayage Win32 `scan-win32b/` TERMINÉ (2026-11-25, nuit) — chiffres exacts : voir `results.jsonl` ; planches dans `sheets/`
- Binaire corrigé (DAC 8 bits), profils retirés puis sondés, règles de capture adaptatives (sans la règle de densité, ajoutée après son lancement).
- Liste des démos denses plafonnées à 48 à refaire avec la règle de densité : `/tmp/claude-1000/dense-a-refaire.txt` (volatile ; la recalculer : `kept > 48`, `cap == 48`, pas de `loop`).
- À lire demain : planches (à l'œil), familles d'échecs (messages dans les captures finales), comparaison avec `scan-win32` (ancien passage, bureau invisible).

### Lecture de `scan-win32b` : familles d'échecs (2026-11-26, OCR de la dernière capture de chacun des 264)
Outil : `tesseract` + `eng.traineddata` (tessdata_fast, téléchargé dans le scratchpad, à re-télécharger si perdu) ; résultat volatil `/tmp/claude-1000/ocr-win32b.json`.
Statuts : 131 exit, 128 ok, 4 error (3dnoid, ts_4k_ms, gerbera, kolor : triple faute / HLT IF=0), 1 no-exe (Woest bh-02). Seulement 45 démos gardent ≥ 10 scènes (42 dans l'ancien passage `scan-win32`) :
le correctif du bureau noir n'a presque rien changé au nombre de scènes (164 avec ≥ 1 scène contre 146 ; 2 régressions : zeekm32k, stars_by_acid_rain ; 1 amélioration à ≥ 4).
- **50** « tampered with… VIRUS » (Petite, intégrité de l'en-tête PE) → chantier HX n°1 : PELOAD.ASM, copie fidèle de l'en-tête.
- **26** exceptions dans DKRNL32 (`C0000005` accès mémoire, `C000001D` instruction illégale ; EIP variés) → famille à instruire (plantages de notre HX ou de nos instructions ?).
- **13** `DialogBoxParam(A/W) called` (bouchon DUSER32) → DialogBoxParam minimal.
- **~12** choix de plein écran / 16 bits / 32 bits (« Fullscreen? », « force 16-bit rendering », « Run the demo in FULLSCREEN mode ? », TRANSCEN « Cancel ») : une touche (Y/Entrée) suffirait, profils à régler.
- **~8** DirectDraw / DirectX : `SetDisplayMode FAILED` (dash), `Couldn't create DirectDrawFactory` (MS2K1FI), `DDEnumCallbackEx: QueryInterface FAILED` (medium), « unknown error » (TRAUMATI, discoteque, lada), « Can't access your whiteboard / Try changing desktop to 24/32 bits » (forsake).
- **4** glide2x.dll : imports `_grLfbWriteRegion@32`, `_grLfbReadRegion@28` absents (STARSYST…) → compléter notre glide2x.
- **~10** divers : `Runtime Error! abnormal program termination` (helium, jeg_er), « could not set window hook » (furious), « Unable to start the application » (dozen), PAK « Not enough memory » (racer), `out of memory` DPMILD32 (alphab), « Bad command or file name » (bustrip), « Can't open x! » (3dsrdr : fichier absent).
- **125** captures SANS texte (97 avec < 4 scènes : écrans noirs, dont 95 « exit » : le programme se termine sans rien afficher) → première cible de l'inspection à l'œil / `--trace-int`.
Ordre suggéré : Petite (50) ≫ exceptions DKRNL32 (26) ≫ DialogBoxParam (13) ≫ profils de touches plein écran (~12) ≫ écrans noirs « exit » (95, à échantillonner).

### Petite 2.3 : analyse du 2026-11-26 (rien de modifié dans HX pour l'instant)
- `NoExit.exe` (CD 49, Petite 2.3) : sections `.text` / `.rsrc` / `.petite` (le décompresseur, à l'`ImageBase` + 0x140000, point d'entrée dedans). L'en-tête du fichier n'est PAS celui d'un éditeur de liens ordinaire :
  stub DOS, en-tête Rich (0x80–0xE0), puis la chaîne « Compressed by Petite, (c) 1998-99 Ian Luck. » à 0xF0, et l'en-tête PE à `e_lfanew` = 0x11C. SizeOfHeaders = 0x400.
- HX (`PELOAD.ASM`) recopie seulement les 0x40 premiers octets, met le reste de la page à zéro (`?CLEARHEADER`), range les structures NT à 0x40 (donc la chaîne Petite et le PE à 0x11C disparaissent), écrase 0x20–0x3B avec ses champs
  (`MZHDR` : liste de modules, comptes, pile…) et force `e_lfanew` à 0x40. Le nom du module et le petit tas du chargeur suivent la table des sections (dans la même page).
- Hypothèse la plus probable pour le « tampered with… VIRUS » : contrôle (CRC ?) de cette zone d'en-tête par le décompresseur. NON vérifiée : la chaîne du message n'est pas en clair dans l'exécutable (décompressée ou masquée) ; les messages en clair sont « ERROR! » / « Corrupt Data! ».
- **Obstacle** : le reste de HX suppose les structures NT à base + 0x40 (`lea … [esi - sizeof MZHDR]` dans PELOAD.ASM, `SearchModuleInAppModList`, `dwMod32`…, et probablement DKRNL32) : déplacer le PE à `e_lfanew` d'origine est INVASIF.
- Outil utile : le débogueur de Bare86 (`--debug`, commandes `x` puis `dv ADRESSE N` : mémoire post-mortem ; `BARE86_HWBP`, points d'arrêt matériels) ; stdin doit rester ouvert (`x` attend la fin du programme).
- Pistes, du moins au plus invasif : (1) essai jetable : à la fin de `LoadPEModule`, restaurer l'en-tête d'origine (octets lus du fichier avant sa fermeture, stockés dans la page libre après SizeOfHeaders) et voir si le contrôle passe sur NoExit (confirme ou infirme l'hypothèse, sans viser la robustesse) ;
  (2) si confirmé : conserver nos champs hors de l'en-tête (ex. bloc à base − 0x1000 ou après SizeOfHeaders) et ne plus supposer base + 0x40 ; (3) sinon : suivre le décompresseur pas à pas (HWBP au point d'entrée 0x5400CE, dump de la pile).

### Essai jetable Petite (2026-11-26) : HYPOTHÈSE CONFIRMÉE
- `tools/hx-build/experiment-restore-header.patch` (sur `PELOAD.ASM` de HX 2.23, jetable, NON robuste) : avant la fermeture du fichier, lit ses 0x400 premiers octets à base+0x800 ; juste avant le lancement de l'application,
  les recopie sur l'en-tête SAUF 0x20–0x3B (champs du chargeur). Le PE d'origine (0x11C) et la chaîne Petite sont donc de retour. Construction : `build-dpmild32.sh` après avoir appliqué le patch dans le clone (`HX/Src/DPMILDR`), essai avec `XDG_DATA_HOME` pointant sur une copie de `HX/`.
- **Résultat sur `NoExit.exe` (Petite 2.3)** : la boîte « This file has been tampered with… VIRUS » DISPARAÎT (comparée à la même commande avec le DPMILD32 d'origine, captures à 4 s et 8 s). Le contrôle porte donc bien sur l'en-tête.
  L'octet 0x20–0x3B n'entre pas dans le calcul (ou reste compatible : zéros d'origine contre nos champs ; à ne pas considérer comme prouvé pour les autres versions).
- Ensuite : écran noir (bureau HX et curseur), 4,7 millions de sorties vers le port 20h en 25 s (boucle ? ou l'effet du saccage du nom du module et du tas du chargeur par la recopie, qui couvre 0x3C–0x3FF) : NON élucidé. C'est l'étape suivante.
- Plan robuste : (a) déplacer le nom du module et le tas du chargeur après SizeOfHeaders (si SectionAlignment ≥ 0x1000) ; (b) ne plus supposer les structures NT à base+0x40 (ou garder une copie à 0x40 uniquement si l'en-tête d'origine n'y met rien d'utile : la chaîne Petite et le PE sont au-dessus de 0x80) ;
  (c) mesurer sur les 50 programmes « tampered » et sur les démos qui marchent déjà (aucune régression).

### Petite 2.3 : VERSION ROBUSTE (2026-11-26) — fonctionne ; reste à décider comment la distribuer
- `tools/hx-build/petite-faithful-header.patch` (remplace l'essai jetable) : patch de `Src/DPMILDR/PELOAD.ASM` de HX 2.23. Principe : le chargeur garde SA copie des structures NT (+ table des sections, nom du module, tas) à `base + NTOFS` (= 0x600),
  et les 0x600 premiers octets du fichier sont recopiés tels quels à la base (stub, Rich, chaîne Petite, PE d'origine à `e_lfanew`). Seuls 0x20–0x3B restent au chargeur (mis à zéro puis remplis comme avant). Tous les `sizeof MZHDR` qui voulaient dire
  « décalage NT » deviennent `NTOFS` (45 sites) ; les 2 lectures de `ofsPEhdr` du chargeur aussi. Si `e_lfanew` est hors de 0x40–0x300, retour à l'ancien comportement (en-tête non fidèle, `e_lfanew` = `NTOFS`).
  Application : `cd HX/Src/DPMILDR && patch -p0 PELOAD.ASM < …/petite-faithful-header.patch` (clone de `build-dpmild32.sh`, avant de relancer le script ; fins de ligne CRLF conservées), puis relancer le script ; installer dans une copie de `HX/BIN` et lancer avec `XDG_DATA_HOME`.
- **Mesures** : les 50 programmes « tampered » : plus AUCUN message (NoExit passe, atteint sa propre erreur `USER32.dll:GetClipCursor`) ; 45 démos qui marchaient (≥ 10 scènes) : AUCUNE différence de contenu entre l'ancien et le nouveau chargeur (critère grossier : capture finale > 4 ko) ;
  `tests/hx_desktop.rs` passe avec le nouveau `DPMILD32.EXE` ; GDITEST normal. Outils jetables : `/tmp/claude-1000/reg/{run,ocr}.py` (à recréer si perdus).
- **Ce qui vient ensuite pour ces 50** : nouvelles familles visibles (échantillon de 45 captures) : `DialogBoxParam called` (7), « Fullscreen? / Run in fullscreen » (3), `joySetThresh` absent de WINMM (3), `unsupported bits per pixel` (2), `GetClipCursor` absent de USER32 (NoExit),
  DirectDraw call failed (evolution), exception DKRNL32 (hwpf), « abnormal program termination » (superstar), reste : images sans texte à regarder à l'œil.
- **Distribution (à décider avec l'utilisateur)** : Bare86 télécharge HX 2.23 (SHA-256) ; un DPMILD32 modifié change cela : PR amont chez Japheth, fork de HX, ou patch binaire/construction locale avec JWasm+JWlink (chaîne validée, mais l'utilisateur final n'a pas ces outils).

### Fork de HX publié (2026-11-26)
- Fork : https://github.com/nicolasbauw/HX (`master` = HX v2.24 + commit `1fe03c0` : en-tête fidèle, `build-linux/build-dpmild32.sh`, README). Remotes locales de `~/Dev/HX` : `origin` = fork, `upstream` = Japheth.
- Release `v2.24-bare86.1` : `DPMILD32.EXE`, SHA-256 `02a94dbb…978ec3`. `src/runtime.rs` (`ensure_fork_loader`) le télécharge (SHA-256 vérifié) à la place du chargeur officiel de 2.23 ;
  seulement si le fichier présent est l'officiel (`3a460e81…`) ; échec = avertissement, HX officiel gardé. Vérifié sur une copie : téléchargement, puis NoExit passe le contrôle Petite.
- À faire plus tard : message à Japheth (proposer le patch ? non envoyé, à valider par l'utilisateur) ; les corrections suivantes de HX (DialogBoxParam, USER32 manquantes) iront dans le même fork.

### Exceptions DKRNL32 (2026-11-26) : classement et premier correctif (DGDI32)
Reprise des 22 programmes à `exception C0000005/C000001D` de `scan-win32b` ; avec le nouveau `DPMILD32` (Petite), un classement frais (`/tmp/claude-1000/exc/cls.py`, volatil) donne 15 exceptions et 7 programmes qui passent ailleurs. Outils : `BARE86_HWBP=<adresse fautive>` + `--debug` (`g`, puis `dv` pile/DC) : l'état est celui de l'instruction fautive.
- **Famille 1, CORRIGÉE : `gdi32.dll` `blt32to08` lit au-delà de la source** (`3s-money`) : `SetDIBits(hdc, hbm, uStartScan, cScanLines, bits, bmi, 0)` ignorait `cScanLines` (copie de toute la hauteur) ; un programme qui envoie UNE ligne à la fois lisait derrière sa ligne.
  Correctif dans le fork (`DIBITMAP.ASM`) ; `3s-money` dépasse la faute (il plante ensuite sur un objet COM nul, famille 3). 45 démos qui marchaient : aucune différence.
- **Famille 2, comprise, NON corrigée : `IDirectDrawSurface::GetDC` d'une surface hors écran** (`WURST!!`, `gdi32.dll+13BE`, adresse 8EA000) : `GetDC_` (`SrcEmu/DDDRAW/SURFACE.ASM`) renvoie le DC de l'ÉCRAN et ne redirige que `pBMBits`/`pOrigin` (`_SetDCBitPtr`) ;
  largeur/hauteur du DC restent celles de l'écran (800×600) alors que le pas et le bpp sont ceux de la surface (256 px × 32 bits) : `BitBlt` croit pouvoir lire 600 lignes. Correction : `_SetDCBitPtr` doit aussi poser largeur, hauteur, pas et bpp de la surface.
- **Famille 3 : appel de méthode COM sur un pointeur NUL** (`demo.exe` de resurrection : `mov ecx,[eax]` avec EAX = 0 lit l'IVT, d'où l'adresse `F000000C` ; même signature `F0000050` (lgr), `F000000C` (tp64k), `F0000080` (3s-money après correctif)) : un objet DirectX (DirectDraw/Direct3D/DirectInput/DirectSound) n'a pas pu être créé, le programme ne le vérifie pas.
  À instruire par programme : quelle interface (voir `[ecx+N]` : N donne la méthode) et quelle création a échoué.
- **Autres** : `haetten` (`gdi32.dll+3582`, écriture à F0000140, non élucidé), 2 × `glut32.dll+F320` (adresse B4010300), 2 × `kernel32.dll` (DKRNL32 : `steam`, `timeloss`, pointeurs aberrants), 1 × « HLT with interrupts disabled » (`gerbera`), 2 × `C000001D` à `B7:13` (`cashcow`, `rpl_book` : saut à l'adresse 0x13).
- **Fork** : `DGDI32.DLL` reconstruit (script `build-linux/build-dgdi32.sh`, bibliothèques d'import de HXDEV224 vérifiées par SHA-256 ; différent d'un octet près entre deux constructions, horodatage) ; release `v2.24-bare86.2` (DPMILD32.EXE + DGDI32.DLL). `RASTERCAPS` (patch binaire de Bare86 jusque-là) est maintenant dans les sources du fork.
  `src/runtime.rs` (`ensure_fork_files`) télécharge les deux (SHA-256 vérifié) à la place des binaires officiels connus. PR amont possible plus tard (assistée par LLM, à discuter avec l'utilisateur ; non préparée).

### Famille « GetDC » et objets DirectX nuls (2026-11-26)
- **Famille 2 (WURST) : l'hypothèse « GetDC de surface hors écran » était FAUSSE** (le programme n'importe que `GetDC` de USER32 et `StretchDIBits`, pas DirectDraw). Vraie cause : `StretchDIBits` (DGDI32, `STRETCH.ASM`) crée un DC compatible et ne remplace que bpp, pas et bits : largeur/hauteur restent celles de l'écran (800×600), donc `BitBlt` lit 600 lignes dans un bitmap de 256 ;
  de plus le pas n'était pas aligné sur un mot double et une DIB à hauteur positive (bas en haut) était dessinée À L'ENVERS. Corrigé dans le fork (largeur/hauteur de l'en-tête, pas aligné, drapeau `DCF_BOTTOM_UP`) : WURST affiche son image (256×256, à l'endroit).
  **Limite connue** : `StretchBlt`/`StretchDIBits` de HX ne REDIMENSIONNENT pas (« there is no stretch in size implemented as of yet ») : WURST demande 264×283 d'une image 256×256, on la voit à l'échelle 1. Une vraie mise à l'échelle (au plus proche voisin) serait utile à beaucoup de démos : à faire. La déclaration `RC_STRETCHBLT/RC_STRETCHDIB` de `RASTERCAPS` (patch de Bare86 pour Half-Life) est donc généreuse ; le README du fork le dit.
- **Objets DirectX nuls : interface manquante = DIRECT3D** (HX n'a AUCUNE interface Direct3D ; son DDraw accepte seulement `IDirectDraw`, `2`, `4`, `7`) :
  `resurrection` (`demo.exe`), `tp64k` (dxm-mental_excession) et `lgr` : `QueryInterface(IID_IDirect3D7 = F5049E77-4861-11D2-A407-00A0C90629A8)` renvoie `0x80004002` (E_NOINTERFACE), le programme ne vérifie pas et appelle une méthode sur un pointeur NUL (lit l'IVT : `F000xxxx`) ;
  `contour` : `IDirectDrawSurface::QueryInterface(IID_IDirect3DTexture2 = 93281502-8CF8-11D0-89AB-00A0C9054129)` → erreur, même issue. Les signatures `F000000C/F0000050/F0000080` (IVT + décalage de méthode) sont donc, pour ces programmes, un symptôme de Direct3D absent.
  **`3s-money` est différent** : aucun appel DirectX n'échoue ; un `Unlock` (méthode 32 de `IDirectDrawSurface`) sur le tampon arrière `0x2702C8` plante parce que la table virtuelle de cet objet vaut alors `F0000000` (objet écrasé ; le premier `Unlock` est valide). Cause de l'écrasement NON trouvée (les points de surveillance en écriture ne montrent, dans ce qui a été lu, que l'initialisation de DDraw).
  `flt-r0x` : `Restore()` renvoie `E_NOTIMPL` puis plante dans son propre code (adresse AC84BC), non instruit.
- **Méthode (réutilisable)** : DDRAW de HX reconstruite en mode trace (`-D_DEBUG`) ; la trace passe par `OutputDebugStringA` = `INT 41h AX=2`, que rien ne traite ici ; `tools/hx-build/dll-build-debug.sh` + `dbgstub.asm` réécrivent la macro `@trace` (copie locale de `MACROS.INC`) pour écrire sur la sortie d'erreur DOS : tout apparaît dans `hxstderr.log` du dossier du programme (chaque appel DirectDraw et son code retour).
  `tools/win32-diag/ddraw-trace.py` lance la liste des programmes en exception et décode les IID ; `iid.py` nomme les IID connus. Chemins `/tmp` en dur (outils de bureau, pas de produit). UPX : `upx-lx` (variante Linux de l'utilisateur) NE décompresse PAS les PE (« not packed by UPX »).
- **Fork** : release `v2.24-bare86.3` (DPMILD32 + DGDI32 avec `StretchDIBits`) ; `src/runtime.rs` la télécharge et met à jour les DGDI32 de la `.2` ; 45 démos qui marchaient : à re-vérifier avec cette DGDI32 si le temps le permet (le correctif `StretchDIBits` ne touche aucune des 45, qui n'appellent pas `StretchDIBits`).
- **Suite logique** : (1) mise à l'échelle de `StretchBlt` ; (2) Direct3D 7 : aucun travail existant sous HX trouvé, gros chantier (au minimum `QueryInterface` honnête ET un périphérique qui échoue proprement plutôt qu'un pointeur nul : les programmes qui testent le résultat s'arrêteraient avec un message au lieu d'un plantage) ; (3) `DialogBoxParam`.

### UPX officiel (2026-11-26)
`upx-lx` (UPX maison de l'utilisateur, Linux/amd64, multi-thread) ne gère pas les PE ; l'UPX officiel 5.3.1 est dans `~/.local/opt/upx-5.3.1/upx` (hors PATH) et décompresse les intros packées (`upx -d -o sortie.exe entree.exe`) : `lgr`, `tp64k`, `3s-money`, `flt-r0x` → analyse statique possible (`objdump -d`, mêmes adresses qu'en mémoire).
Premier résultat sur `3s-money` : le tampon arrière est dans la variable globale `0x431A08` ; le code à `0x40C73D` la remet à zéro (`andl $0,0x431a08`) avant de passer son adresse à `GetAttachedSurface` ; le `Unlock` fautif (`0x40C9BD`) la relit et la trouve à 0 : une réinitialisation a eu lieu, ou un chemin d'erreur est pris, sans que la trace DDraw montre de `Release` ou de second `CreateSurface` (piste : lire la fonction qui contient `0x40C72E`-`0x40C88F`).

### StretchBlt / StretchDIBits : mise à l'échelle FAITE (2026-11-26)
- Fork : `SrcEmu/DGDI32/STRETCH.ASM` (commit « StretchBlt and StretchDIBits scale »), release `v2.24-bare86.4` (DGDI32 `19674b98…25af3`). Plus proche voisin, `SRCCOPY`, sources de 8/16/24/32 bits : le rectangle source est rééchantillonné ligne par ligne dans un bitmap temporaire (taille de la destination, limitée à celle du DC destination) de MÊME orientation que la source, puis `BitBlt` le copie (la conversion de couleurs reste dans `BitBlt`) ; mêmes tailles, autre ROP, 1/4 bits, tailles ≤ 0 : `BitBlt` comme avant.
  `StretchDIBits` : l'origine du rectangle source d'une DIB de bas en haut est son coin inférieur gauche (Windows) ; converti en ordonnée depuis le haut.
- Test : `c/tests/win32/stretch.c` (+ `stretch.exe` versionné, construit par `c/build.sh` avec `wcl386 -bt=nt -l=nt_win`) et `tests/hx_stretch.rs` : image 4×4 32 bits étirée en 160×160, sous-rectangle 2×2 bas-gauche ; vérifie échelle, orientation et sous-rectangle. Le test ÉCHOUAIT avec la DGDI32 de la release `.3` (sans mise à l'échelle) et passe avec la `.4`. `hx_desktop` passe aussi.
- Les teintes sont fortement déformées par la conversion vers la palette 8 bits de HX (le rouge pur donne (16,0,0), le blanc (16,44,44)) : `_col24to8` de DGDI32 ou la palette du bureau ; NON examiné, à regarder si des démos paraissent ternes.
- WURST : l'image 256×256 remplit maintenant le rectangle 264×283 demandé. Les 45 démos de référence n'appellent ni `StretchBlt` ni `StretchDIBits` (imports vérifiés) ; non relancées avec la `.4`.

### Conversion de couleurs vers l'écran 8 bits : CORRIGÉE (2026-11-26) — fork `v2.24-bare86.5`
Symptôme (test `stretch.exe`/`px2.c`) : image 32 bits sur le bureau 8 bits aux couleurs fausses (rouge pur → (16,0,0), blanc → (16,44,44), gris → rouge clair). Quatre causes empilées, trouvées par dump de la table au point d'arrêt de `blt32to08` (`BARE86_HWBP`, `dv EBX`) :
1. **VESA32 (`Src/VESA32/SETPAL.ASM`)** : lecture de la palette par les PORTS du DAC : le rouge lu est gardé dans `bh` mais l'entrée est complétée par `mov al,bl` (l'INDICE) : composante rouge = indice, à partir de l'indice 10 (`GetSystemPaletteEntries` : `10  10 252  84`). Deux branches (DAC 6 et 8 bits), corrigées en `mov al,bh`. La palette système que DGDI32 réécrit au DAC au démarrage était fausse partout.
2. **DGDI32 `BitBlt` vers l'écran 8 bits** : la table de couleurs du DC est la palette STOCK par défaut (20 entrées, le reste à zéro : « ELAP » = `PALE` en tête de table, `cntEntries = 0x14`) ; `_col24to8` parcourt 256 entrées → noir ou les 20 couleurs statiques. Corrigé : on utilise `g_syspal.ColorTab` (256 entrées) pour un DC écran de 8 bits.
3. **Ordre R/B** : cette table est au format `PALETTEENTRY` (R,G,B) alors que `_col24to8` reçoit `00RRGGBB` (ordre DIB) et comparait rouge et bleu en croix. Variable `g_bPalPE` posée par `BitBlt` (1 si DC écran) : `_col24to8` échange alors les rôles. Les DC mémoire à DIB 8 bits (`RGBQUAD`, B,G,R) sont inchangés.
4. (aucune autre : le test donne maintenant R,G,B,W,gris,jaune,orange justes.)
- Constructions : `build-linux/build-vesa32.sh` (nouveau ; JWlink ne résout pas les noms courts du `.RSP` sur ces objets : exports écrits avec les noms décorés `_Nom@N` relevés dans les objets ; même taille que la DLL officielle, 9216 octets) ; `build-dgdi32.sh` inchangé.
- Test : `tests/hx_stretch.rs` exige maintenant (à ±8) rouge, vert, bleu, blanc purs ; il passe avec les fichiers téléchargés (release `.5`).
- **Non-régression** : 45 démos de référence relancées : 40 avec contenu avant et après ; comparaison fine des images finales : 12 identiques, les autres diffèrent surtout par l'animation ; `bitwise-array` (64k Remedy, ENTIÈREMENT NOIRE jusqu'ici, voir 2026-11-25) AFFICHE maintenant son logo « BITWISE » (gain réel). `sprite`, `101_by_xaos` : même scène à un autre instant.
- Release `v2.24-bare86.5` : DPMILD32 + DGDI32 (`d41ae9fc…`) + VESA32 (`710540d6…`) ; `src/runtime.rs` télécharge les trois (SHA-256 vérifié) à la place des officiels et met à jour les DGDI32 des versions précédentes.
- À revérifier plus tard : les ~125 captures « sans texte » de `scan-win32b` étaient peut-être en partie dues à cette palette : un nouveau balayage Win32 mesurerait le gain.

### DialogBoxParam : FAIT (2026-11-26) — fork `v2.24-bare86.6`
- `SrcEmu/DUSER32/Dlgbox.asm` : `DialogBoxParamA/W` et `DialogBoxIndirectParamA` n'affichaient qu'une boîte « called » ; maintenant : `WM_INITDIALOG` (avec le paramètre d'initialisation), puis `WM_COMMAND`/`IDOK`, puis `IDCANCEL`, puis `WM_CLOSE`, jusqu'à ce que la procédure appelle `EndDialog` ; la fonction renvoie le code de `EndDialog` (1 si jamais fermée, pour ne rien bloquer) ; `EndDialog` fonctionne, y compris pour des boîtes imbriquées (liste de `DLGSTATE` sur la pile des appelants ; le « handle » est un pointeur opaque).
  Il n'y a toujours ni gestionnaire de dialogues, ni gabarit lu, ni contrôles : `GetDlgItem` etc. renvoient 0 comme avant (les procédures qui exigent de vrais contrôles ne feront pas grand-chose). `CreateDialogParam` (non modal) inchangé (renvoie 0).
- Construction : `build-linux/build-duser32.sh` (nouveau ; demande `wrc` d'Open Watcom pour les ressources de DUSER32 — curseurs — avec l'option `-zm` (format Microsoft, lisible par JWlink) ; libs de HXDEV224 ; `libc32u.lib` du dépôt). Même taille que la DLL officielle (43008 octets).
- Test : `c/tests/win32/dialog.c` (+ `dialog.exe` versionné, sous-système console : `c/build.sh` détecte « wcl386 -bt=nt -l=nt) » dans le source) et `tests/hx_dialog.rs` : `ok=42 flags=3 cancel=7 close=9 never=1 outer=5 inner=42` ; avec le DUSER32 officiel, le programme reste bloqué sur la boîte « called ».
- **Les 13 programmes** (`DialogBoxParam called` dans `scan-win32b`) : plus aucune boîte « called » (Venom : pas d'exécutable). Ce qu'ils montrent ensuite, 12 programmes :
  5 × `C000001D` à `B7:13` (saut à l'adresse 0x13 : backtothefuture, neveragain, slavery, abcdemo, + `cashcow`/`rpl_book` déjà vus : FAMILLE à instruire, 7 programmes au total) ;
  2 × `gdi32.dll+3582` écriture à `F0000140` (yxskaft, obs_dp ; même signature que `haetten`, 3 programmes : FAMILLE à instruire, probablement une écriture dans le LFB après un changement de mode) ;
  2 × « unsupported bits per pixel » (zelig, cv16380 : DirectDraw, mode demandé) ;
  1 × exception C++ MSVC `0xe06d7363` (`n_brain`) : `throw` non rattrapé, à instruire ;
  2 × écran vide sans message (glass, MP2) ; 1 × affiche `[ANA]` (ana_ana : peut-être sain).
- Familles du balayage restantes, par taille : saut à `B7:13` (7), écriture `F0000140` dans gdi32 (3), Direct3D 7 absent (4+), « unsupported bits per pixel » (2+), `GetClipCursor`/`joySetThresh` absents (USER32/WINMM : petits ajouts faciles), choix « Fullscreen? » (touches).

### Saut à l'adresse 0x13 : ÉLUCIDÉ (2026-11-26) — fork `v2.24-bare86.8`
`C000001D` à `B7:13` = un `call` à l'adresse 0 : le code exécuté est la TABLE DES VECTEURS (IVT) jusqu'à la première instruction invalide (octets `F0 50` à 0x13). Deux causes différentes :
1. **Mon identifiant de boîte de dialogue** (`DialogBoxParam` de la `.6`) : un pointeur opaque vers une structure de pile, que `SendMessage(hDlg, WM_SETICON…)` de HX lisait comme un objet de fenêtre (`backtothefuture`, `neveragain`, `abcdemo`, `slavery` plantaient À CAUSE DE MOI, après avoir été débloqués). Corrigé (`.7`) : la boîte est une vraie fenêtre cachée de classe `HXDlg` (sa procédure passe tout à la procédure de dialogue). `abcdemo`, `backtothefuture`, `neveragain` tournent ; `slavery` passe à une autre faute (écriture `F0000…`, accès dans son propre code). Test : `dialog.c` envoie `WM_SETICON` à sa boîte et lit son rectangle.
2. **`MSVFW32.DLL` absente de HX** (`cashcow`, `rpl_book`… : DrawDibOpen/DrawDibDraw/DrawDibClose via `LoadLibrary`/`GetProcAddress`) : le pointeur reste NUL et l'appel saute à 0. Nouvelle DLL du fork (`SrcEmu/MSVFW32/MSVFW32.ASM`, 14 fonctions DrawDib ; `DrawDibDraw` passe par `StretchDIBits`, qui redimensionne ; −1 = taille du bitmap, `lpBits` nul accepté, origine du rectangle source en HAUT à gauche). Pas de fonctions de compression (IC…), de capture ni d'AVI.
   Bare86 l'INSTALLE si elle manque (`ensure_fork_files`, liste de binaires officiels vide). Test `tests/hx_drawdib.rs` + `c/tests/win32/drawdib.c` (LoadLibrary comme les démos ; couleurs, sous-rectangle, `lpBits` nul).
- Diagnostic : adresse de retour au sommet de la pile (`dv` à ESP sous `BARE86_HWBP=13`) → appel depuis le code de l'application ; octets désassemblés avec `objdump -D -b binary -mi386 --adjust-vma=…` après un `dv` ; la variable de pointeur de fonction était nulle ; les chaînes proches (`MSVFW32.DLL`, `DrawDibOpen`) ont donné la DLL. (`cashcow` est packé avec un UPX trop ancien pour l'UPX officiel actuel : « obsolete version ».)
- Résultats : `cashcow` dessine ses images (bandes horizontales par endroits : non examiné) ; `ana_ana` affiche une scène 3D ; `rpl_book` arrive à « Can't access your whiteboard. Try changing desktop to 24/32 bits depth. » (DirectDraw : profondeur du bureau, autre famille).
- Familles restantes : écriture `F0000140` dans gdi32 (3 : yxskaft, obs_dp, haetten), Direct3D 7 (4+), « unsupported bits per pixel » (2+), profondeur 24/32 bits exigée (rpl_book…), exception C++ `0xe06d7363` (n_brain), `GetClipCursor`/`joySetThresh` manquants, `slavery`.

### Écriture à `F0000140` dans gdi32 : ÉLUCIDÉ (2026-11-26) — fork `v2.24-bare86.9`
- Symptôme (`haetten`, `yxskaft`, `obs_dp`) : `C0000005` dans `gdi32.dll` (`SelectObject`), accès à `F0000140`. Trace (DGDI32 reconstruite en `-D_DEBUG`, `dbgstub`) : `SelectObject(00000030, 00AB0020)` puis `PatBlt(00000030…)` : le contexte d'affichage est `0x30`, pas `0x00AB0030` ; `F0000140` est simplement ce que `SelectObject` lit à l'adresse 0x30 (l'IVT).
- Cause : la procédure de fenêtre de ces démos fait `and eax,0FFFFh` sur `wParam` avant `DefWindowProc` (`WM_ERASEBKGND` : `wParam` = le DC). Habitude Win16/Win9x où les handles GDI tiennent sur 16 bits ; sous HX un handle est une adresse de 32 bits. (Trouvé par `SendMessageA(..., 0x14, 00AB0030)` suivi de `DefWindowProc(..., 0x14, 00000030)` dans la trace, puis en lisant le code de la démo : `25 ff ff 00 00`.)
- Correctif (DUSER32) : `GetDC` retient les 8 derniers DC ; `_RecoverDC` retrouve le handle complet d'une valeur < 0x10000 qui est le mot bas de l'un d'eux ; `DefWindowProc` l'applique à `WM_ERASEBKGND`. Les autres messages à handle coupé ne sont PAS récupérés (limite connue).
- Test : `c/tests/win32/erase16.c` + `tests/hx_erase16.rs` (fond rouge peint, pas d'exception ; échoue avec le DUSER32 de la `.8`).
- Résultats : `haetten`, `yxskaft`, `obs_dp` ne plantent plus. `haetten` affiche une fenêtre grise de réglages SANS ses boutons radio (« 22 Khz », « 44 Khz » : `CreateWindowEx` renvoie 0 : HX n'a pas les classes de contrôles standard BUTTON/STATIC/EDIT…) → la démo attend probablement un clic sur un bouton absent. `yxskaft` : message « Not enough… » (à lire). 
- NOUVELLE FAMILLE : **classes de contrôles standard absentes** (BUTTON, STATIC, EDIT, COMBOBOX, LISTBOX…) : toute démo à boîte de réglages faite de contrôles reste bloquée ou inerte. Gros chantier (contrôles + gestionnaire de dialogues avec gabarits) ; une première étape utile serait `BUTTON` (clic sur « Start »/OK) et `STATIC`.
- Outil : `duser-dbg-build` (DUSER32 en `-D_DEBUG`, modules qui ne passent pas recompilés sans trace : `keyboard.asm`) ; `/tmp/claude-1000/duser-dbg-build.sh` (volatil).

### Petits ajouts (GetClipCursor, joySetThreshold & Cie) : FAITS (2026-11-26) — fork `v2.24-bare86.10`
- **Méthode** : au lieu de deviner, comparaison SYSTÉMATIQUE des imports des 264 programmes Win32 (exe + DLL du dossier, tables d'import lues en Python, `/tmp/claude-1000/imp/scan.py`) aux exports des DLL de HX (clés : `DKRNL32→kernel32`, `DUSER32→user32`, `DGDI32→gdi32`, `DADVAPI→advapi32`, `DDDRAW→ddraw`…). Limite : les programmes PACKÉS (UPX, Petite) résolvent leurs imports à l'exécution par `GetProcAddress` : leurs noms n'apparaissent pas (c'est le cas de `GetClipCursor`, vu dans NoExit, absent de la liste statique).
- **Ajouts** : DUSER32 `GetClipCursor` (rectangle posé par `ClipCursor`, ou l'écran), `FrameRect` (cadre d'un pixel, vérifié au pixel près), `GetDCEx`, `HideCaret`/`ShowCaret`, `ExcludeUpdateRgn`, barres de défilement (`SetScrollInfo`, `GetScrollInfo`, `GetScrollPos`, `GetScrollRange`, `ShowScrollBar`, `EnableScrollBar` : aucune barre, valeurs neutres), `DrawEdge`, `DrawFrameControl` (rien n'est dessiné), `CheckRadioButton`, `SetMessageQueue`, `InsertMenuItemA`, MDI (`DefMDIChildProcA`, `DefFrameProcA`, `TranslateMDISysAccel`), `DefDlgProcA`, `CreateDialogIndirectParamA` (renvoie NULL comme `CreateDialogParamA`), `ActivateKeyboardLayout`, `Begin/Defer/EndDeferWindowPos` ; DGDI32 `GetBrushOrgEx`/`SetBrushOrgEx`, `CreatePenIndirect`, `GetBkMode` ; WINMM `joySetThreshold` (`joyGetThreshold`, `joyConfigChanged` ; pas de manette : `MMSYSERR_NODRIVER` comme les autres `joy*`), `waveOutSetPlaybackRate`/`Get…` (non supporté), `timeGetSystemTime`.
- **Test** : `c/tests/win32/smallfuncs.c` IMPORTE ces fonctions statiquement (sinon HX refuse de charger le programme) + `tests/hx_smallfuncs.rs` (résultats dans SMALL.TXT, cadre rouge vérifié aux 4 côtés). `c/build.sh` lie maintenant `winmm.lib`.
- **WINMM** : demande l'import de `SB16.DLL` dont la bibliothèque d'import n'est pas dans HXDEV ; `build-linux/build-winmm.sh` déclare les imports à l'éditeur de liens (`import '_SndPause@0' 'SB16.DLL'.SndPause`, noms décorés relevés dans les sources de SB16) — la tentative de fabriquer une bibliothèque d'import à la main (format court Microsoft, deux membres d'index, `//`) n'a PAS été lue par JWlink. Piège : sans `.DLL` dans le nom du module, l'import s'appelle `SB16` et DPMILD32 répond « File not found error ». Le `WINMM.DLL` officiel est dans `HXGUI`, pas `HXRT`.
- **Pas fait** (compteurs sur les 264 programmes) : `ImageList_*` de COMCTL32 (4 à 6 programmes, des applis MFC), `EnumFontsA` (2), `CoBuildVersion` (4)/`IsEqualGUID` (3) d'OLE32, `SetBrushOrgEx`… (fait), `ChooseColorA` (2). Autres DLL absentes de HX, par nombre de programmes : `d3d8.dll` (22 !), `wininet.dll` (11), `d3drm.dll` (7), `d3dxof.dll` (5), `avifil32.dll` (4), `dinput8.dll` (2), `glide3x.dll` (1).
- **À noter** : Direct3D 8 (`d3d8.dll`) pèse 22 programmes statiques : plus que les 4 de Direct3D 7 vus au traçage DDraw. Gros chantier ; aucun travail existant sous HX connu.

## TROUS DU BALAYAGE DOS — À COMBLER (consigné le 2026-11-26 à la demande de l'utilisateur ; sujets évoqués la veille)
Les démos DOS sont majoritaires jusqu'à fin 1998 (tableau « Part des démos Windows selon la période » plus haut : CD 27-33 : 299 DOS pour 2 Win32 ; CD 34-45 : 126 pour 11) : la fiabilisation du balayage DOS pèse donc plus que le Win32 pour ces années. Quatre chantiers :
1. **Démos de 1997 (CD 27 à 33, sept. 1997 → mars 1998) à peu de captures : À RELANCER.** `scan-full` : 371 éléments sur ces CD, dont **94 à moins de 5 scènes et 26 à 0 scène** (chiffres d'avant les reprises du 25/11, qui en ont déjà corrigé une partie : refaire le décompte). Causes déjà connues : anciennes erreurs « pick » (12 éléments, tous au CD 27), catégories qui ne sont pas des démos (`GAME100K`, `MULTICHA`), archives incomplètes (données absentes), exécutable choisi qui n'est pas la démo. À relancer avec les règles de capture actuelles de `survey.py` (plafond adaptatif, détection de boucle, `prep`, `--probe`).
2. **1998 : captures IDENTIQUES avec un texte bleu « future dimension » (AMB98).** Constat vérifié le 2026-11-26 : 8 éléments (`cd35_AMB98_{grtc_x_mbeast, infs_x_amb98, mfst_x_higher, mfst_x_oxygen, mfst_x_weknow, mfst_x_zero, mmul_x_ambiva}`, `cd36_am98_xamb`) ont tous `exc-whq.exe` comme exécutable principal, 213 ou 214 « scènes » identiques (l'image « Future Dimension » + un numéro de téléphone, bruit animé) et `exc-info.exe` en `others` (jamais lancé) ; 5 autres éléments portent `exc_whq.exe`. C'est une PUBLICITÉ de groupe (même nature que `cfxadv.com`, déjà repéré) choisie à la place du contenu de la démo.
   **À faire** : écarter ces exécutables « publicitaires » du choix de l'exécutable principal (liste de noms : `exc-whq`, `exc_whq`, `cfxadv`, à vérifier `#pitrlz#.exe` (17 éléments), `guide.exe`, `demo.exe` d'une taille type…) et lancer les AUTRES (`exc-info.exe`, ou le reste de l'archive) ; refaire un contrôle de doublons d'images médianes sur toute la collection (groupes de ≥ 3 éléments à image identique : le script de la section a trouvé ces 4 + REHASH, dracula…, `tg98/3dfx`, `bazar`, `i_want_t`).
3. **Démos jamais testées parce qu'elles sont DANS des archives** : (a) archives à plusieurs exécutables dont un seul est lancé : **213 éléments sur 774, ≥ 354 exécutables jamais capturés** (chantier décrit dans « Démos vides : causes mesurées » : `survey.py` doit lancer CHAQUE exécutable, un sous-élément par exécutable) ; cas extrêmes `cd36/ms98/demo.zip` et `cd36/tg98/demo.zip` (12 autres chacun), `juggler_2001_final.zip` (12 versions d'une 4K) ; (b) conteneurs d'archives IMBRIQUÉES classés « hors sujet » (`AM98/in4k`, `MS98/demo`, `classify.py`) dont le contenu n'a jamais été ouvert : à dépaqueter récursivement puis traiter comme des éléments.
4. **Démos longues ou riches en scènes : dépasser le plafond de 48 captures.** Le plafond (`--max-frames`, voulu, réparti sur toute la durée) a été complété le 25/11 par la règle de densité (une scène toutes les 5 s ou moins : toutes gardées, jusqu'à 300) ; les éléments déjà traités avant cette règle restent plafonnés à 48 : liste à recalculer (`kept > 48`, `cap == 48`, pas de `loop`, dans les `results.jsonl` récents ; `/tmp/claude-1000/dense-a-refaire.txt` était volatil) et à relancer. Les `results.jsonl` de `scan-full` n'ont pas encore le champ `cap` : on repart des plus anciens (CD 27 à 63).
- Méthode commune : (a) une exécution par EXÉCUTABLE, (b) liste d'exécutables « publicitaires » à écarter, (c) refaire le décompte « < 5 scènes » par année, (d) garder les `results.jsonl` anciens (`scan-full`) intacts et écrire dans de nouveaux dossiers (`scan-dos2`…), comme pour `scan-win32c`.

### Outillage DOS prêt (2026-11-26) — rien n'est encore lancé (balayage Win32 `scan-win32c` en cours, 4 machines en parallèle)
- `tools/survey.py` : (1) `AD_NAMES` : les exécutables publicitaires `exc-whq`/`exc_whq`/`cfxadv` ne sont plus choisis comme exécutable principal quand l'élément en a d'autres (chantier 2) ; (2) `--exe-list FICHIER` : « élément<TAB>exécutable dans l'élément » par ligne ; lance CET exécutable, un résultat par ligne (identifiant `élément__exécutable`, champ `only`), à reprise comme le reste (chantier 3a).
- `CD PC Team/exe-list-dos.tsv` : **276 exécutables jamais lancés dans 133 éléments**, tirés du champ `others` de `scan-full` (installations/configurations/lecteurs écartés : 78 ; publicités écartées). Par CD : 36 (75), 29 (54), 71 (23), 31 (22), 32 (15), 59 (14), 50 (13), 57 (11). Limite : `others` est tronqué à 12 par élément (4 éléments concernés) ; les exécutables Windows de la liste sont notés `win32` (non lancés sans `--win32`).
- Commande prévue (après le balayage Win32) : `python3 -I tools/survey.py --src "CD PC Team/dos-demos" --out "CD PC Team/scan-dos2" --exe-list "CD PC Team/exe-list-dos.tsv" --full --limit 600 --every 2 --idle 60 --jobs 4 --probe 25 --profiles "CD PC Team/profiles.json"` (mêmes réglages que `scan-win32b`, binaire figé comme `tools/sweep-win32.sh`).
- Reste à outiller : (a) archives imbriquées (chantier 3b) ; (b) décompte « < 5 scènes » par année et liste des éléments de 1997 à relancer (chantier 1) ; (c) liste des éléments plafonnés à 48 (chantier 4) ; (d) contrôle de doublons d'images médianes sur toute la collection (chantier 2).

## Trace de ce qui pousse le CPU très haut — OUTILS + PREMIÈRE CORRECTION (2026-11-26, à la demande de l'utilisateur ; optimisation = chantier de seconde phase)
- **Ce qui existait** : `survey.py` échantillonne le CPU de Bare86 toutes les 0,5 s (`cpu`, `cpu_peak`, `cpu_peak_at`, `cpu_hot` : part du temps au-dessus de 0,9 cœur) dans chaque `results.jsonl` (722 éléments mesurés sur 774 pour `scan-full`, 263 sur 264 pour `scan-win32b`). Mais rien ne les rassemblait ni ne gardait le POURQUOI.
- **`tools/cpu-report.py`** → `docs/cpu-hotlist.md` (à régénérer après chaque balayage ; plusieurs `results.jsonl` possibles) : par balayage, nombre de programmes « chauds » (cpu_hot ≥ 0,5) et leur tableau, en deux familles : **« attente ? »** (CPU haut ET écran figé / arrêt pour inactivité / ≤ 2 scènes : boucle d'attente probable, que Bare86 pourrait endormir : piste la plus rentable) et **« calcul »** (CPU haut ET l'écran bouge : travail réel, relève du JIT, du bridage `--mips`, de la machine).
- **`tools/cpu-profile.py`** → `cpu-profiles.jsonl` à côté des résultats : relance les N programmes les plus chauds 20 s avec `--stats` (survey.py arrête le programme par un signal et Bare86 n'écrit ses compteurs qu'à sa fin normale, donc ils ne sont pas dans les résultats) et range un résumé : exits/s, appels à l'hôte/s, irq/s, nombre d'endormissements, instructions interprétées, jit, ports les plus lus. `cpu-report.py` les ajoute au tableau.
- **Constats chiffrés (premier rapport)** : `scan-full` (DOS) : **39 programmes chauds sur 774, dont 24 « attente ? »**, CPU moyen 0,24 cœur ; `scan-win32b` : **87 chauds sur 264, TOUS « attente ? »** (CPU moyen 0,37 cœur) : les démos Win32 qui échouent (boîte d'erreur) attendent une touche en occupant un cœur pendant des minutes.
- **Première cause trouvée et corrigée** : la boucle d'attente de HX (boîte de message, écran d'erreur) appelle sans cesse `INT 2Fh AX=1680h` (« libérer la tranche de temps ») en alternance avec `INT 21h AX=4400h` ; Bare86 répondait aussitôt (57 000 appels à l'hôte/s, aucun endormissement : `idle_poll` n'était pas appelé pour `INT 2F/16`, et `INT 21h/44h` remettait son compteur à zéro). `src/machine.rs` : `INT 2Fh AH=16h` appelle `idle_poll(false)` ; `INT 21h AH=44h` est neutre. Boîte de message en attente : **1,00 → 0,09 cœur**, réaction à une touche inchangée (2,03 s pour une touche à 2 s). Test `tests/hx_idle.rs` (+ `c/tests/win32/msgbox.c`) : mesure le temps CPU du processus Bare86 sur 8 s, exige < 0,35 cœur ; il tombe à 0,99 sans la correction. Piège de test : `Stdio::null()` ferme l'entrée standard et la fin de fichier VALIDE la boîte : garder un tube ouvert.
- **Autres profils déjà relevés (exemples, DOS)** : `durangoparty2k_fastcoding_differential` 0,99 cœur : 86 000 sorties KVM/s, 858 000 écritures du port `3C9h` (palette : fondus) et 30 millions d'instructions INTERPRÉTÉES (chemin lent : candidat JIT) ; `JUMPER97_64K_SBSLEEP` 0,96 cœur : 180 000 sorties/s, **1,7 million de lectures de `3DAh`** (attente du retour de balayage : boucle à endormir : 4 endormissements seulement), irq 746/s ; `ANTIQ297…MCP_SPLA` : 0,01 cœur (ne reproduit pas la mesure du balayage).
- **À faire (seconde phase, optimisation)** : (1) relancer `cpu-profile.py` sur les 15 plus chauds de chaque balayage (DOS et Win32 `scan-win32c`) après le balayage en cours, et ranger les causes par type : attente par `3DAh`/clavier/minuterie (endormir), fondus de palette (`3C9h` : sorties à regrouper), code interprété (JIT), appels hôte (autres INT d'attente) ; (2) pour chaque type, un correctif général + un test comme `hx_idle` ; (3) comparer le CPU moyen avant/après sur `scan-win32c` : objectif ≪ 0,37 cœur.
- **Mesure du balayage Win32 en cours** (`scan-win32c`) : lancé AVANT cette correction (binaire figé `bare86-sweep-scan-win32c`) : ses chiffres de CPU sont donc « avant » ; refaire un balayage ou `cpu-profile.py` pour le « après ».

## Balayage Win32 `scan-win32c` terminé (2026-11-27)

- **Bilan (264 éléments)** : ≥ 4 scènes : 46 → 64 (≥ 10 scènes : 62) ; CPU moyen 0,37 → 0,27 cœur ; programmes chauds 87 → 55 (tous « attente ? » ; balayage lancé AVANT la correction `INT 2Fh`). Liste régénérée : `docs/cpu-hotlist.md`.
- **Régressions (13 éléments perdent ≥ 3 scènes)** : cause principale **mémoire** : `madeby.exe` (BSS de 32 Mo) et `le petit prince` finissaient en triple faute dans HDPMI à 64 Mo (HX charge toute l'image BSS comprise). Correctif `b33320d` : sans `--mem`, 256 Mo pour un programme Windows ; test `tests/hx_bigbss.rs`. Relances : madeby 1→44, kprincefinal 1→302, whiskey 20→87, unet 1→82, doas 16→52, sprite 22→445, taz_mexicana 64→67.
- **`jx_exinf.exe` (COVEN2000/demo) : 38 → 3 scènes, NON corrigé.** Démo OpenGL qui appelle `DialogBoxParamA` (réglages). Avec les DUSER32/DGDI32/VESA32 officiels elle s'anime ; avec notre DUSER32 elle reste noire après « Loading » (bisect : DUSER32 seul en cause). Hypothèse NON vérifiée : notre `DialogBoxParam` envoie `IDOK` sans vrais contrôles (liste déroulante/boutons absents) et la démo lit un réglage vide → mauvais mode. Piste : contrôles standard minimaux (COMBOBOX/BUTTON/EDIT/STATIC) — chantier déjà listé.
- Outil : `tools/sweep-dos.sh NOM` (balayage des 276 exécutables de `exe-list-dos.tsv`, binaire figé) ; lancé le 2026-11-27 dans `scan-dos2`.
- Diagnostic utile : à l'arrêt « triple fault » l'état du vCPU est déjà remis à zéro (registres illisibles) ; comparer `--trace-int` avec/sans fork, et `XDG_DATA_HOME` sur une copie de HX (ajouter un octet nul au fichier officiel pour que Bare86 ne le remplace pas).

### Exécutables « publicitaires » dans `exe-list-dos.tsv` (2026-11-27, signalé par l'utilisateur)
- **Constat** : dans `scan-dos2`, des éléments d'archives différentes ont les MÊMES captures (`AI_SPICE.ZIP__#pitrlz#.exe` = `CN_G.ZIP__#pitrlz#.exe` ; `FL-DOP2.ZIP__izxcppkm.com` = `INF-FRM.ZIP__uicliznd.com`). Ce ne sont pas des démos : `#PITRLZ#.EXE` (14 676 octets, CRC 92695dc8, présent dans 19 archives du CD 29), `!ART!.EXE` (6 archives du CD 31) et les `.COM` à nom aléatoire (`izxcppkm.com`, `uicliznd.com` : 2578/2524 octets) sont des programmes publicitaires/étiquettes de distributeur glissés dans l'archive ; la vraie démo (`SPICELES.EXE`, `DOP2.EXE`) est déjà passée au balayage `scan-full`.
- **Cause** : la liste `others` de `scan-full` contenait tout ce qui n'était pas l'exécutable principal, publicités comprises (filtre `AD_NAMES` limité à AMB98).
- **Actions** : `AD_NAMES` étendu (`#pitrlz#`, `!art!`) ; `exe-list-dos.tsv` 276 → 244 lignes (sauvegarde `.bak-276`) : un seul représentant par fichier identique (CRC identique dans plusieurs archives : 8 fichiers, 40 lignes) et aucun pour les deux publicités connues ; balayage `scan-dos2` relancé (reprend, 57 faits dont 11 `#pitrlz#`, à ignorer). Les `.COM` à nom aléatoire ne sont pas détectables par le nom : à repérer après coup avec **`tools/dup-shots.py scan-dos2`** (groupes d'éléments aux captures identiques, avec leur exécutable).
- Limite : seuls 144 des 276 exécutables ont un CRC lisible (archives ZIP) ; les autres (dossiers, RAR, LHA) n'ont pas pu être comparés.
- **Vérifié le 2026-11-27 (crainte : perdre la vraie démo)** : dans `scan-full`, 17 éléments avaient `#pitrlz#.exe` comme exécutable principal (CD 28 et 29) et 44 un exécutable publicitaire en tout (`#pitrlz#`, `!art!`, `cfxadv`, `exc-whq`). Pour tous ceux dont l'archive contient une autre démo (`water.com`, `aa_1kint.com`, `szarbump.com`…), l'autre exécutable est dans `exe-list-dos.tsv` : la vraie démo est donc relancée par `scan-dos2` et n'est pas perdue ; les 13 restants (`exc-whq` + `exc-info.exe`, séries `X_*`) sont des images/musiques sans démo. Il y a aussi les `.COM` polymorphes à nom aléatoire de BIZ97 (`izxcppkm`, `uicliznd`, `bentlhyu` : 2524–2580 octets, contenu différent à chaque archive, même écran publicitaire « 8 lines v34/ISDN » ; l'archive porte un `-I-ACE-Z-.XC`) : seules les captures les trahissent → `tools/dup-shots.py`.
- `tools/dup-shots.py scan-full` : 38 groupes ; hors `exc-whq`, ce sont des doublons légitimes (même démo dans deux archives, séries de magazines `PAIN*`, `shine#`), aucun autre exécutable publicitaire à craindre dans `scan-full`.
- **`ZMEP5.EXE` (CD 28, EUS97/IN4K : CLMINTRO, PIEINTRO ; signalé par l'utilisateur)** : fichier identique octet pour octet (3504 octets, compressé « LZ91 ») dans les deux dossiers ; il affiche le logo « EUSKAL » (jaune) : c'est l'écran du parti Euskal Party livré avec chaque intro, pas la démo (`INTRO.EXE`, `PIE.EXE`, déjà passées dans `scan-full`). Ajouté à `AD_NAMES` (`zmep5`). Ma déduplication par CRC ne couvrait que les ZIP : vérifié aussi les dossiers (SHA-256), c'est le seul doublon de ce type dans la liste. Résultats `scan-dos2` de ces deux éléments à ignorer.
- **Règle à retenir** : tout exécutable « secondaire » est suspect s'il apparaît à l'identique (ou avec les mêmes captures) dans plusieurs archives d'un même événement ; contrôle final par `tools/dup-shots.py` avant d'interpréter `scan-dos2`.
- **`VNT.EXE` (CD 28 et CD 32, `M_MELONI.ZIP` ; signalé par l'utilisateur)** : PKLITE, 7937 octets daté de 1996, texte seul : « Note: This addy didn't contain shadebobs or donuts, chromefaces or ducks » (publicité de BBS) puis retour au DOS (1 capture). Cas différent des précédents (pas de captures communes avec d'autres archives, mais deux copies de la même archive) : la vraie démo, `MELONI.EXE` (288 et 292 scènes), est déjà dans `scan-full`. Ajouté à `AD_NAMES` (`vnt`) ; résultats `scan-dos2` correspondants à ignorer. `dup-shots.py` ne l'attrapera pas : repérer aussi les exécutables à 1 capture / texte seul / sortie immédiate.

## Catalogue des résultats (décidé le 2026-11-27 ; à construire après `scan-dos2`)
- **But** : réunir les résultats des ~22 dossiers `scan-*` (nommages différents : `scan-full` = dossier par élément, `scan-dos2` = `élément__exécutable`) sans rien déplacer ni supprimer.
- **Index maître** `catalogue.jsonl` : une ligne par (élément, exécutable) : dossier de captures RELATIF à `CD PC Team`, source du balayage, date, statut, scènes, CPU, année, classe (démo / publicité / outil / installateur / visionneuse / image-musique), meilleur résultat retenu et raison, étiquettes manuelles (« à revoir », « mauvais exécutable »…). Les exclusions sont motivées, jamais silencieuses.
- **Pas de liens symboliques** : le disque est en NTFS (`ntfs3`) ; Windows les lit mal, les chemins absolus Linux n'y valent rien. Pages HTML avec chemins relatifs (`../scan-full/shots/…`) : aucune copie, aucun risque d'écraser. Noms contenant `#`, `!`, espaces finaux : signalés, pas renommés sans accord.
- **Aspect** : reprendre celui du site personnel (`~/Dev/pageperso` : HTML statique, `index.css`, polices JetBrains Mono, fond `#222021`, titres `#a7bfb3`, texte `#169179`, liens `#9db07a`, accent orange, menu à listes déroulantes, titre « The Orange Nerd »), pour une intégration future : menu Computers → rubrique demoscene → page par ANNÉE puis par DEMOPARTY → page de la démo (planche, captures, lien pouet.net, aucun exécutable distribué). Génération statique par `tools/catalogue.py` depuis `catalogue.jsonl` ; le CSS du site est référencé, pas dupliqué (chemin configurable).
- **Ordre** : outil + essai sur `scan-full` et `scan-dos2`, structure présentée à l'utilisateur AVANT la production complète ; tri des exécutables secondaires (doublons de captures, 1 capture / texte seul / sortie immédiate) à la fin de `scan-dos2`.

### `ebobr.zip` : mêmes captures pour les 11 démos (2026-11-27, signalé par l'utilisateur)
- **Cause** : `survey.py` lançait `RUNIT.BAT` (qui enchaîne `call briscols.exe`, `call rainbows.exe`, … les 11 démos) pour CHAQUE exécutable ; chaque dossier de `scan-dos2` rejoue donc la même série (captures : `C:\>call jplattic.exe`…). Dans `scan-full`, la seule exécution (via `briscols.exe`) est cette série entière. Ce n'est pas une publicité.
- **Correctif** : `survey.py` : pour un exécutable demandé précisément (`only`), un .BAT qui lance plus de deux programmes (collection) n'est plus utilisé. Dans `scan-dos2` seul `ebobr` est concerné parmi les 17 éléments lancés par un .BAT. Liste de relance : `CD PC Team/exe-list-ebobr.tsv` (11 exécutables) ; à lancer après `scan-dos2` dans `scan-dos3` ; les 10 dossiers `ebobr…__*.exe` de `scan-dos2` sont à ignorer.
- **Défaut de Bare86 trouvé en chemin** : `INT 10h AX=1B00h` (informations de fonctionnalité/état VGA, utilisée par des programmes compilés pour reconnaître une VGA) n'était pas gérée (« INT 10h AH=1Bh not handled », AL inchangé = « pas de VGA »). Ajoutée (`vga_info` dans `src/bios.rs`, test `asm/tests/vgainfo.S`).

## Complétion des interruptions BIOS/DOS (2026-11-27, décidée avec l'utilisateur : « la complétion est aussi importante que la fiabilisation »)
- **Constat** : `INT 10h AH=1Bh` manquait (cf. ebobr) ; revue du reste : en mode graphique, tout le BIOS texte (09h/0Ah/0Eh/13h) et le défilement (06h/07h) ne faisaient RIEN (garde `if s.text`), et les pixels (0Ch/0Dh) manquaient : un menu de setup en mode 13h, un `PRINT`, un écran d'erreur n'apparaissaient pas.
- **Fait (commit `3e289d8`)** : `src/bios/gfx.rs` : pixels (modes 13h, 0Dh, 0Eh, 10h, 12h, CGA 04h–06h), caractères depuis la police de INT 43h (nouvelle police 8×8 en ROM, `FONT_8X8` = F000:3000, tirée de la 8×16), défilement par pixels, `tty` dessine aussi pour le DOS (`INT 21h`) grâce à la VGA mémorisée par thread ; AH=11h AL=21h–24h (polices graphiques), AL=30h BH=3/4 (8×8), 0Bh, 04h, 14h, 12h BL=20h/37h/38h ; tests `asm/tests/gfxbios.S`, `vgainfo.S`.
- **Fait ensuite** : INT 14h (aucun port série : AX=8000h), INT 17h (imprimante prête, octets perdus), INT 16h AH=04h/09h/0Ah, INT 15h AH=80h–82h/83h (attente d'événement)/85h et services PS/2 absents sans message, INT 21h AH=03h/04h/05h, 1Bh/1Ch (allocation), 1Fh/32h (DPB : AL=FFh), 5Ch (verrou accordé), 5Eh/5Fh (réseau : erreur 1) ; test `asm/tests/biosmisc.S`.
- **Revue INT 21h** : 92 fonctions gérées ; restent absentes : 53h (traduction BPB), 64h, 69h, 6Dh, 6Eh (rarissimes). **INT 33h** : très complète. **INT 15h manette (84h)** : déjà servie par la machine.
- **Limites connues** : pages d'affichage ignorées pour les pixels/caractères graphiques ; modes 0Fh/11h (monochromes) et VBE : pas de dessin BIOS ; l'attribut de fond de 0Bh est seulement mémorisé dans la BDA ; polices utilisateur 11h/00h/10h en mode texte : acceptées sans effet.
- **À refaire** : relancer les démos dont le message était « INT 10h AH=… not handled » (ebobr : `scan-dos3`), puis juger l'effet sur les écrans de setup.
- **Résultat `scan-dos3` (ebobr, 2026-11-27)** : chaque démo tourne seule (~22 s puis retour au DOS), 4 à 12 scènes distinctes, `dup-shots.py` : 0 groupe. Le texte « CREATED BY JURAJ PAULIK » (quijotte) s'affiche désormais (texte BIOS en mode graphique). Reste `rainbows.exe` : 1 scène, écran noir, non terminée en 64 s → à examiner. `scan-dos2` terminé (198 éléments) : tri des exécutables secondaires à faire.

## Tri de `scan-dos2` (2026-11-27) — `tools/triage.py`, `docs/triage-scan-dos2.tsv`
- `tools/triage.py DOSSIER [--newer AUTRE]` classe chaque résultat (une ligne par identifiant ; la dernière fait foi : un balayage repris ajoute des lignes) : `pub`, `doublon` (via `dup-shots.py`), `outil`, `a-relancer` (message d'une fonction BIOS ajoutée depuis), `setup` (OCR : menu de carte son/configuration), `win32`, `court` (rend la main en < 12 s : les captures toutes les 2 s sont trop rares), `vide`, `peu`, `bon`, `remplace`.
- **Bilan `scan-dos2`** (219 identifiants) : bon 101, peu 10, pub 15, doublon 10, outil 6, a-relancer 4 (+ 12 relancés), court 20, vide 14, setup 6, win32 15, remplace 18.
- **Constat marquant** : beaucoup de « 1 scène » sont des intros 1K/4K qui se terminent en 4–5 s : avec une capture toutes les 2 s, il n'en reste qu'une. À relancer avec `--every 0.5` (`scan-dos4`).
- **Relances faites** : `scan-dos3` : ebobr (11) + 12 démos `win00` de PAIN0997/0897 (INT 10h 0Ch/0Dh : de 1 à 2–4 scènes). `scan-dos4` : les 20 « court » à 0,5 s ; `scan-dos5` : les 15 programmes Windows avec `--win32`. Listes : `exe-list-rerun1/2/3.tsv`.
- **À traiter à la main** : `setup` (sus_x/check.exe, d32_x/sinx.exe, juggler « GMIDI @ 3x0h? ESC for no music ») : trouver les touches ; `vide` (heresy « calculating scenes », chess32k menu, insomnia, urknall…) ; `outil` (s3vbe20, aweutil, restore.com, mouse.exe, dos32, P.EXE) : confirmer que ce ne sont pas des démos.

### Menus de son et relances `scan-dos4/5/6` (2026-11-27)
- **Profils par exécutable** : `survey.py` accepte la clé `élément|exécutable` dans `profiles.json` (archives imbriquées : un profil par archive ne distingue pas leurs exécutables) ; la sonde y enregistre aussi sous cette clé. Sauvegarde : `profiles.json.bak-before-sound-menus`.
- **Touches trouvées** : MIDAS (`sus_x/check.exe`) : 9 × bas puis Entrée = Sound Blaster 16 ; Useless Sound System (`d32_x/sinx.exe`) : Entrée (carte SB16 par défaut), Entrée (port 220), Entrée (IRQ 5), 2 × haut puis Entrée (DMA 1), Entrée (44100 Hz) → 200 scènes ; `juggler` (GMIDI) : Échap = sans musique.
- **`scan-dos4`** (20 courts à 0,5 s) : 10 passent à 2–4 scènes ; **`scan-dos5`** (15 Windows) : 4 bons (heaven7w, varus compatible : 204, nicht), 3 peu ; le reste sort aussitôt (famille Win32 connue).
- **Verdicts manuels** (`docs/triage-notes.tsv`, lu par `triage.py`) : pub `prophecy.exe` (BBS Prophecy) ; outils `pbpt_pv.exe` (« needs windows »), `m2.exe` (aide du megademo EXTERN), `varus w2k/w98/wme.com` (lanceurs d'une démo Windows), `!trivia.exe` (texte seul).
- **À étudier (défaut de Bare86 probable)** : `ewox_dem.exe` (lancée par `dos4gw ewox_dem.exe`) : « Valid video mode not found » ; `luckyd.exe` : DOS/4GW « Mode not found » : deux démos du « loader system » balayent les modes VBE 100h–1F8h (4F01 répond OK pour chacun) sans en trouver un acceptable → reconstituer ce qu'elles cherchent (attributs, LFB, fenêtres). `kolakola.exe` : « Run under WIN95 ».

### VBE : « Valid video mode not found » (ewox) et « Mode not found » (luckyd) — résolus (2026-11-27)
- **ewox_dem.exe** (Gathering 98) : le code de SciTech (LFBPROF) calcule l'adresse linéaire de la liste des modes par `(ptr >> 12) + (ptr & 0xFFFF)`, ce qui ajoute le quartet haut du décalage : avec `F000:EA00` la liste était lue 14 octets trop loin (les 7 premiers modes sautés) ; les vrais BIOS ont un décalage < 1000h. **Correctif** : tous les pointeurs du bloc 4F00 (modes, OEM, fabricant, produit, révision) sont normalisés (segment:décalage avec décalage < 16). Trouvé en lisant le code de la démo au débogueur (l'achat de temps : l'outil `--trace-int` montre maintenant DI et le bloc rendu par 4F01 ; `BARE86_HWBP_W=rw` surveille aussi les lectures).
- **luckyd.exe** (BugX, Mekka 98) : son source `main.c` est dans l'archive : `BugxModeCode(400,300,16)` = mode 400×300 en 16 bits, absent de la liste. **Correctif** : modes 400×300 (8/15/16/24/32 bits : 137h–13Bh).
- Les deux démos tournent (scène 3D, logo THX). Test : `asm/tests/vbeinfo.S`.
- **Autres trouvailles par OCR des éléments à 0–2 scènes de `scan-full`** (78) : `clinique.exe` (rpl_clin.zip, CD 48) : menu « select your video mode » → profil `2` (320×240, 16 bits), à relancer ; `scenial4.exe` (CD 31) : « Cannot initialize 64kb color mode / install UNIVBE » après des sondes `INT 10h AH=6Fh/5Fh` (détection de chipsets, jamais de VBE 4F00) → hors de portée pour l'instant.
- **Relance groupée des profils nouveaux (2026-11-27)** : `scan-dos7` : clinique 1 → 51 scènes (touche `2`), luckyd 132 scènes ; `scan-dos8` : ewox_dem.exe 257 scènes (219 images distinctes), arrêtée par la limite après 2219 s (démo longue ; profil par exécutable `{wait:900000}` : Entrée à 3 s la faisait quitter).

## Périmètre : magazines sur disquette exclus (2026-11-27, décision de l'utilisateur)
- Les « diskmags » (`MAGS/PAIN*`, `STG-*`/`TGD-*` (Technoguide), `RC-VOL01` (volvo), `PLSCENE*`, `SHINE*`…) ne nous intéressent pas : 2 à 4 captures de texte y sont normales, inutile de les paginer. Verdict manuel `magazine` dans `docs/triage-notes.tsv` (règle `MAGS_`, exceptions `SCENIAL4` et `TRIP5` qui restent à voir). À écarter des décomptes « < 5 scènes » par année.

## Périmètre : dossiers `demo`/`demos` et `in64` seulement (2026-11-27, précisé par l'utilisateur)
- Les CD sont organisés par demoparty ; **seuls les dossiers `demo` (ou `demos`) et `in64` nous intéressent** ; le reste (musiques, diskmags, intros 4K, images, jeux, utilitaires…) est ignoré. `tools/triage.py` : classe `hors-perimetre` (expression `SCOPE`) ; `64k`/`64ko`/`in64k` sont des variantes de nom de `in64` (CD 27 : `WIR97/64K`) : comptées dans le périmètre.
- **Mesure sur `scan-full`** (687 éléments) : 350 dans `demo`/`demos`/`in64`, dont 43 à moins de 5 scènes (hors win32) ; par CD : 27 : 15, 28 : 37, 29 : 49, 30 : 27, 31 : 11, 32 : 23, 33 : 6, 34 : 7, 35 : 13, 37 : 10, 38 : 11, 39 : 18, 42 : 16 …
- Conséquence : les intros 1K/4K (`1KO`, `4KO`, `IN4K`), `GAME100K`, `MISC`, `PICS`, `MAGS`, `GRTC`/`MMUL` (graphismes, musique) sortent des décomptes et des relances.

## Défaut de `survey.py` trouvé (2026-11-27) : les réessais EFFAÇAIENT de bonnes captures
- Cas `hypno2.exe` (le mouton) : 138 scènes au premier essai, mais 0 au résultat. **Cause** : après un premier essai réussi, le programme qui rend la main en affichant un texte contenant « usage/parameter/dpmi/…» déclenchait des réessais (`param_candidates`, `--dpmi`, `--ems`) ; chaque réessai commence par EFFACER les captures du précédent, et le réessai (qui échoue en 2 s) écrasait le résultat. Probablement une cause de nombreux « exit à 0–1 scène » des anciens balayages (20 éléments de `scan-full` portent la trace d'un réessai avec ≤ 1 scène).
- **Correctif** : plus aucun réessai dès que le premier essai garde ≥ 2 scènes ; `SURVEY_DEBUG=1` trace chaque `execute` et chaque capture vue.
