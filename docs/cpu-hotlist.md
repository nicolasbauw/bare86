# Programmes qui poussent le CPU très haut

Généré par `tools/cpu-report.py` (survey.py mesure le CPU de Bare86 toutes les 0,5 s ; 1,0 = 100 % d'un cœur). « chaud » = `cpu_hot` ≥ 0.5 (plus de 50 % du temps au-dessus de 0,9 cœur).

## scan-full

774 éléments, 722 mesurés, **39 chauds** dont **24 « attente ? »** (écran figé : boucle d'attente probable) et 15 « calcul ». CPU moyen de l'ensemble : 0.24 cœur.

| élément | exécutable | famille | cpu | pic (à t) | chaud | scènes | durée | profil `--stats` |
|---|---|---|--:|--:|--:|--:|--:|---|
| `cd58_durangoparty2k_fastcoding_differential.zip` | plasma.exe | attente ? | 1.00 | 1.02 (36s) | 1.00 | 1 | 409s |  |
| `cd29_ANTIQ297_64KO_MCP_SPLA.ZIP` | splash.exe | calcul | 0.99 | 1.02 (130s) | 0.99 | 63 | 184s |  |
| `cd32_JUMPER97_64K_SBSLEEP.ZIP` | swalkers.exe | calcul | 0.99 | 1.02 (33s) | 0.99 | 90 | 208s |  |
| `cd71_MekkaSymposium_IN4K_occ-karpovfinal.zip` | karpov_final.com | attente ? | 0.99 | 1.02 (49s) | 0.99 | 1 | 409s |  |
| `cd30_DST97_IN4K_SYS4KX.ZIP` | 4k.exe | attente ? | 0.99 | 1.02 (43s) | 0.98 | 1 | 145s |  |
| `cd58_durangoparty2k_fastcoding_differential.zip` | plasma.exe | attente ? | 0.99 | 1.02 (56s) | 0.98 | 1 | 474s |  |
| `cd64_Dialogos_In4k_stoerfall_ost_final.zip` | stoerfall_ost_dsnd.com | attente ? | 0.99 | 1.02 (2s) | 0.98 | 1 | 409s |  |
| `cd71_MekkaSymposium_IN4K_occ-karpov.zip` | karpov.com | attente ? | 0.99 | 1.02 (58s) | 0.98 | 1 | 409s |  |
| `cd31_DH97_IN64_TPOLMVIL.ZIP` | p2.exe | calcul | 0.98 | 1.02 (58s) | 0.98 | 73 | 176s |  |
| `cd64_Dialogos_In4k_madman4k.zip` | madman.4k/madman4k.exe | calcul | 0.98 | 1.02 (5s) | 0.98 | 3 | 317s |  |
| `cd71_MekkaSymposium_IN4K_illusion-prefinal.zip` | illusion-party-prefinal.com | attente ? | 0.98 | 1.02 (50s) | 0.98 | 1 | 409s |  |
| `cd64_Dialogos_In4k_judas.zip` | judas.com | attente ? | 0.99 | 1.02 (45s) | 0.97 | 1 | 409s |  |
| `cd29_GRV97_DEMO_HC_SO.ZIP` | so.exe | calcul | 0.98 | 1.02 (233s) | 0.97 | 216 | 598s |  |
| `cd31_SAT97_IN64_JFF-SAT.ZIP` | saturnin.exe | calcul | 0.98 | 1.02 (72s) | 0.97 | 53 | 139s |  |
| `cd39_ASM98_demo_hannu__k.zip` | demo.exe | calcul | 0.98 | 1.02 (78s) | 0.97 | 116 | 348s |  |
| `cd32_TMDC2_AD_BAD.ZIP` | badmutha.exe | calcul | 0.97 | 1.02 (3s) | 0.97 | 50 | 176s |  |
| `cd34_FLAG98_in64_undersea.zip` | undersea.exe | calcul | 0.97 | 1.02 (44s) | 0.96 | 20 | 134s |  |
| `cd64_Dialogos_In4k_stoerfall_ost.zip` | stoerfall_ost_final.com | attente ? | 0.98 | 1.02 (63s) | 0.95 | 1 | 410s |  |
| `cd71_MekkaSymposium_IN4K_ILLUSION.ZIP` | illusion.com | attente ? | 0.98 | 1.02 (29s) | 0.95 | 1 | 409s |  |
| `cd48_ICING99_in64_n_jazz.zip` | jazz.exe | calcul | 0.97 | 1.02 (11s) | 0.95 | 15 | 59s |  |
| `cd32_ASSEMB_1_DEMOS_MARKKU.ZIP` | demo.exe | calcul | 0.94 | 1.02 (180s) | 0.93 | 89 | 189s |  |
| `cd60_horizon00_in64_dungha.zip` | dungha.exe | attente ? | 0.94 | 1.02 (70s) | 0.90 | 13 | 214s |  |
| `cd31_MAGS_BSR-0011.ZIP` | bsr11.exe | attente ? | 0.90 | 1.02 (49s) | 0.90 | 5 | 417s |  |
| `cd28_ASM97_DEMO_MARKKU` | DEMO.EXE | calcul | 0.91 | 1.00 (170s) | 0.89 | 85 | 195s |  |
| `cd29_GRV97_IN64_PLB_BORN.ZIP` | borntro.exe | calcul | 0.91 | 1.02 (19s) | 0.89 | 17 | 69s |  |
| `cd31_MAGS_BSR-0012.ZIP` | bsr12.exe | attente ? | 0.90 | 1.02 (45s) | 0.89 | 5 | 417s |  |
| `cd31_MAGS_RC-VOL01.ZIP` | volvo.exe | attente ? | 0.89 | 1.02 (43s) | 0.86 | 4 | 415s |  |
| `cd31_MAGS_SHINE2.ZIP` | shine#2.exe | attente ? | 0.85 | 0.98 (62s) | 0.84 | 2 | 73s |  |
| `cd59_Mekka_symposium_2000_32kgame_chess32k.zip` | chess32k.exe | attente ? | 0.85 | 1.02 (60s) | 0.84 | 2 | 419s |  |
| `cd59_Mekka_symposium_2000_32kgame_chess32k.zip` | chess32k.exe | attente ? | 0.83 | 1.02 (70s) | 0.80 | 2 | 419s |  |
| `cd32_ASSEMB_1_DEMOS_RESNOW.ZIP` | macgyver.exe | attente ? | 0.81 | 1.02 (52s) | 0.78 | 7 | 158s |  |
| `cd35_dracula_DEMO_PM_FREE.RAR` | PM_FREE.EXE | attente ? | 0.86 | 1.00 (194s) | 0.75 | 54 | 230s |  |
| `cd31_MAGS_RESTLES1.ZIP` | rst_1_x/restless.exe | attente ? | 0.79 | 1.02 (44s) | 0.75 | 9 | 165s |  |
| `cd57_ambiance2K_demo_jippu.zip` | jippu.exe | calcul | 0.73 | 1.02 (42s) | 0.69 | 27 | 99s |  |
| `cd32_COMPUS_1_INTRO_CMY_ODON.ZIP` | odonto.exe | attente ? | 0.73 | 1.02 (92s) | 0.63 | 18 | 125s |  |
| `cd50_ASSEMB_1_IN4K_SAVE_WOR.ZIP` | saveworl.com | attente ? | 0.82 | 1.02 (2s) | 0.62 | 1 | 63s |  |
| `cd50_ASSEMB_1_IN4K_SAVE_WOR.ZIP` | saveworl.com | attente ? | 0.82 | 1.00 (2s) | 0.62 | 1 | 63s |  |
| `cd36_am98_IN64.zip` | 2pi/2pikpeos.exe | calcul | 0.75 | 1.02 (47s) | 0.51 | 20 | 77s |  |
| `cd33_COMMAN_1_FANTASY_DRAGON.ZIP` | dragon.exe | attente ? | 0.58 | 1.02 (30s) | 0.51 | 25 | 274s |  |

## scan-win32b

264 éléments, 263 mesurés, **87 chauds** dont **87 « attente ? »** (écran figé : boucle d'attente probable) et 0 « calcul ». CPU moyen de l'ensemble : 0.37 cœur.

| élément | exécutable | famille | cpu | pic (à t) | chaud | scènes | durée | profil `--stats` |
|---|---|---|--:|--:|--:|--:|--:|---|
| `cd60_horizon00_demo_preampli.zip` | preampli.exe | attente ? | 1.00 | 1.02 (22s) | 1.00 | 1 | 261s |  |
| `cd48_ICING99_demo_3ssheliumfinal.zip` | helium.exe | attente ? | 0.99 | 1.02 (42s) | 1.00 | 1 | 261s |  |
| `cd56_ELEVATOR2000_demo_esko.zip` | uusmedia.exe | attente ? | 0.99 | 1.02 (10s) | 1.00 | 1 | 387s |  |
| `cd56_ELEVATOR2000_demo_ssonic.zip` | demo.exe | attente ? | 0.99 | 1.02 (45s) | 1.00 | 1 | 261s |  |
| `cd56_MIDWINTER2000_demo_dream.zip` | dream.exe | attente ? | 0.99 | 1.02 (34s) | 1.00 | 1 | 261s |  |
| `cd71_MekkaSymposium_fastintro_MS2K1FI.ZIP` | ms2k1fi.exe | attente ? | 0.99 | 1.02 (62s) | 1.00 | 1 | 261s |  |
| `cd73_paradox2001_demo_reason.zip` | reason.exe | attente ? | 0.99 | 1.02 (48s) | 1.00 | 1 | 261s |  |
| `cd79_The_Gathering_2k2_Demo_2_iv_-_racer_by_tls.` | iv.exe | attente ? | 1.00 | 1.02 (19s) | 0.99 | 1 | 261s |  |
| `cd42_DreamHack98_3dem_boing_by.zip` | boing.exe | attente ? | 0.99 | 1.02 (28s) | 0.99 | 2 | 478s |  |
| `cd50_ASSEMB_1_DEMOS_ACCEL_TRAUMATI.ZIP` | trmtik.exe | attente ? | 0.99 | 1.02 (51s) | 0.99 | 2 | 303s |  |
| `cd53_dreamhack99_demo_no-accel_furious.zip` | furious.exe | attente ? | 0.99 | 1.02 (37s) | 0.99 | 1 | 260s |  |
| `cd54_norvegia99_demo_discoteque.zip` | discoteque.exe | attente ? | 0.99 | 1.02 (55s) | 0.99 | 1 | 260s |  |
| `cd57_Trax2K_demos_yearn.zip` | yearning.exe | attente ? | 0.99 | 1.02 (22s) | 0.99 | 1 | 300s |  |
| `cd59_Mekka_symposium_2000_demo_2eme_hjb_mist.zip` | mikrostrange/fscreen.exe | attente ? | 0.99 | 1.02 (63s) | 0.99 | 1 | 261s |  |
| `cd73_Assembly_2001_Demos_5_energia_by_sunflower.` | energia_by_sunflower/energia.exe | attente ? | 0.99 | 1.02 (46s) | 0.99 | 1 | 261s |  |
| `cd78_Woest_intershoelace.zip` | intershoelace.exe | attente ? | 0.99 | 1.02 (17s) | 0.99 | 1 | 261s |  |
| `cd53_dreamhack99_demo_no-accel_rzr-mind.zip` | mindstam.exe | attente ? | 0.99 | 1.02 (57s) | 0.98 | 1 | 261s |  |
| `cd53_dreamhack99_in64_blocc_final.zip` | intro.exe | attente ? | 0.99 | 1.02 (36s) | 0.98 | 1 | 475s |  |
| `cd54_Abort99_misc_ade-3hr.zip` | 3hurry.exe | attente ? | 0.99 | 1.02 (63s) | 0.98 | 1 | 261s |  |
| `cd54_norvegia99_fastintro_lada.zip` | lada.exe | attente ? | 0.99 | 1.02 (53s) | 0.98 | 1 | 260s |  |
| `cd57_ambiance2K_demo_esanssi.zip` | esanssi.exe | attente ? | 0.99 | 1.02 (30s) | 0.98 | 1 | 261s |  |
| `cd58_Thegathering2k_in64_glass.zip` | glass.exe | attente ? | 0.99 | 1.02 (35s) | 0.98 | 1 | 261s |  |
| `cd58_Thegathering2k_in64_n_airf.zip` | airscape_final.exe | attente ? | 0.99 | 1.02 (60s) | 0.98 | 1 | 299s |  |
| `cd58_escape2k_cdx_zelig.zip` | oldskool.exe | attente ? | 0.99 | 1.02 (45s) | 0.98 | 1 | 299s |  |
| `cd60_remedy00_demo_rpl_ape.zip` | ape.exe | attente ? | 0.99 | 1.02 (31s) | 0.98 | 1 | 261s |  |
| `cd60_remedy00_demo_flt_vodka.zip` | fltvodka.exe | attente ? | 0.99 | 1.02 (7s) | 0.98 | 1 | 475s |  |
| `cd67_MindSource_2001_demo_aaeop.zip` | powah.exe | attente ? | 0.99 | 1.02 (17s) | 0.98 | 1 | 261s |  |
| `cd67_MindSource_2001_demo_sadness.zip` | sadness.exe | attente ? | 0.99 | 1.02 (64s) | 0.98 | 1 | 261s |  |
| `cd71_MekkaSymposium_fastintro_OLYMPIC.ZIP` | olympic.exe | attente ? | 0.99 | 1.02 (22s) | 0.98 | 1 | 261s |  |
| `cd75_coma3_Demos_abcdemo.zip` | abcdemo.exe | attente ? | 0.99 | 1.02 (9s) | 0.98 | 1 | 261s |  |
| `cd75_coma3_Demos_rzr-fn2-final.zip` | fn2.exe | attente ? | 0.99 | 1.02 (8s) | 0.98 | 1 | 261s |  |
| `cd77_Bestof_Demo_2001_TheParty_hybrisnemesis-tp2` | jeg_er_et_kvaj.exe | attente ? | 0.99 | 1.02 (53s) | 0.98 | 1 | 261s |  |
| `cd83_Cafe_2002_demo_wireworld.zip` | world.exe | attente ? | 0.99 | 1.04 (1s) | 0.98 | 1 | 261s |  |
| `cd49_TKOVER99_DEMOS_NOEXIT.ZIP` | noexit.exe | attente ? | 0.98 | 1.02 (43s) | 0.98 | 1 | 261s |  |
| `cd50_ASSEMB_1_DEMOS_NO-ACCEL_TRANSCEN.ZIP` | au_trans.exe | attente ? | 0.98 | 1.02 (53s) | 0.98 | 1 | 387s |  |
| `cd66_demo_plaap_-_fosfaatti.zip` | player.exe | attente ? | 0.98 | 1.02 (47s) | 0.98 | 1 | 261s |  |
| `cd52_scorpion_final.zip` | scorpion final/exec/scorpion.exe | attente ? | 0.99 | 1.02 (60s) | 0.97 | 1 | 475s |  |
| `cd57_ambiance2K_demo_straight.zip` | clock.exe | attente ? | 0.99 | 1.02 (46s) | 0.97 | 1 | 261s |  |
| `cd59_Mekka_symposium_2000_demo_4eme_hjb_feel.zip` | strange.exe | attente ? | 0.99 | 1.02 (24s) | 0.97 | 1 | 261s |  |
| `cd66_demo_unique_-_raised.zip` | raised.exe | attente ? | 0.99 | 1.02 (19s) | 0.97 | 1 | 261s |  |

… et 47 autres (`--top` pour en voir plus).

