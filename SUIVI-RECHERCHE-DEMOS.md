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
- À FAIRE ensuite : `tools/survey.py` pourrait désormais lancer les démos qui n'ont qu'un `.BAT` (aujourd'hui il lit leurs arguments
  par `bat_args` et lance l'exécutable lui-même).

