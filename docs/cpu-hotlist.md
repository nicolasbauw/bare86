# Programmes qui poussent le CPU très haut

Généré par `tools/cpu-report.py` (survey.py mesure le CPU de Bare86 toutes les 0,5 s ; 1,0 = 100 % d'un cœur). « chaud » = `cpu_hot` ≥ 0.5 (plus de 50 % du temps au-dessus de 0,9 cœur).

## scan-win32c

264 éléments, 263 mesurés, **55 chauds** dont **55 « attente ? »** (écran figé : boucle d'attente probable) et 0 « calcul ». CPU moyen de l'ensemble : 0.27 cœur.

| élément | exécutable | famille | cpu | pic (à t) | chaud | scènes | durée | profil `--stats` |
|---|---|---|--:|--:|--:|--:|--:|---|
| `cd53_dreamhack99_demo_no-accel_furious.zip` | furious.exe | attente ? | 1.00 | 1.02 (43s) | 1.00 | 1 | 129s |  |
| `cd57_Trax2K_demos_yearn.zip` | yearning.exe | attente ? | 1.00 | 1.02 (21s) | 1.00 | 1 | 130s |  |
| `cd54_Abort99_misc_ade-3hr.zip` | 3hurry.exe | attente ? | 0.99 | 1.02 (60s) | 1.00 | 1 | 129s |  |
| `cd58_Thegathering2k_in64_n_airf.zip` | airscape_final.exe | attente ? | 0.99 | 1.02 (24s) | 1.00 | 1 | 130s |  |
| `cd77_Bestof_Demo_2001_TheParty_hybrisnemesis-tp2` | jeg_er_et_kvaj.exe | attente ? | 0.99 | 1.02 (42s) | 1.00 | 1 | 132s |  |
| `cd53_dreamhack99_demo_accel_al_castone.zip` | demo/dh99/dreamhack99.exe | attente ? | 0.99 | 1.02 (52s) | 0.99 | 1 | 130s |  |
| `cd53_dreamhack99_demo_no-accel_yxskaft.zip` | yxskaft.exe | attente ? | 0.99 | 1.04 (2s) | 0.99 | 1 | 130s |  |
| `cd71_MekkaSymposium_fastintro_BEDAMAX.ZIP` | 0lБmpik.exe | attente ? | 0.99 | 1.02 (50s) | 0.99 | 1 | 130s |  |
| `cd73_Abstract_2001_in64_shapes.zip` | shapes.exe | attente ? | 0.99 | 1.02 (44s) | 0.99 | 1 | 130s |  |
| `cd50_ASSEMB_1_DEMOS_ACCEL_TRAUMATI.ZIP` | trmtik.exe | attente ? | 0.99 | 1.02 (14s) | 0.98 | 2 | 134s |  |
| `cd57_ambiance2K_demo_straight.zip` | clock.exe | attente ? | 0.99 | 1.02 (55s) | 0.98 | 1 | 130s |  |
| `cd71_MekkaSymposium_fastintro_MS2K1FI.ZIP` | ms2k1fi.exe | attente ? | 0.99 | 1.02 (38s) | 0.98 | 1 | 130s |  |
| `cd73_Xenox_demos_n_cv16380.zip` | trip.exe | attente ? | 0.99 | 1.02 (24s) | 0.98 | 1 | 130s |  |
| `cd75_coma3_Demos_rzr-fn2-final.zip` | fn2.exe | attente ? | 0.99 | 1.02 (59s) | 0.98 | 1 | 130s |  |
| `cd78_Woest_intershoelace.zip` | intershoelace.exe | attente ? | 0.99 | 1.02 (33s) | 0.98 | 1 | 130s |  |
| `cd65_Dreamhack_Demos_the_booze_mooses_by_tmb_and` | the booze mooses/mooze.exe | attente ? | 0.98 | 1.02 (27s) | 0.98 | 1 | 130s |  |
| `cd50_ASSEMB_1_DEMOS_NO-ACCEL_TRANSCEN.ZIP` | au_trans.exe | attente ? | 0.99 | 1.02 (57s) | 0.97 | 1 | 130s |  |
| `cd50_ASSEMB_1_DEMOS_NO-ACCEL_VIPING_B.ZIP` | viping.exe | attente ? | 0.99 | 1.02 (28s) | 0.97 | 1 | 130s |  |
| `cd57_demobit2K_demos_mushca.zip` | mushca.exe | attente ? | 0.99 | 1.02 (61s) | 0.97 | 1 | 130s |  |
| `cd58_escape2k_cdx_zelig.zip` | oldskool.exe | attente ? | 0.99 | 1.02 (55s) | 0.97 | 1 | 130s |  |
| `cd63_Arroutada_kolors.zip` | kolors.exe | attente ? | 0.99 | 1.02 (53s) | 0.97 | 1 | 132s |  |
| `cd65_Party2000_intro64k_rpl_forsake.zip` | forsake.exe | attente ? | 0.99 | 1.02 (37s) | 0.97 | 2 | 134s |  |
| `cd71_MekkaSymposium_fastintro_FAST_TSQ.ZIP` | fast.exe | attente ? | 0.99 | 1.02 (7s) | 0.97 | 1 | 131s |  |
| `cd74_demos_TRSAC_2001_03_don_quijote.zip` | 03_don quijote/fwdmosys.exe | attente ? | 0.99 | 1.02 (3s) | 0.97 | 1 | 130s |  |
| `cd83_Cafe_2002_demo_wireworld.zip` | world.exe | attente ? | 0.99 | 1.02 (32s) | 0.97 | 1 | 130s |  |
| `cd57_ambiance2K_demo_esanssi.zip` | esanssi.exe | attente ? | 0.98 | 1.02 (59s) | 0.97 | 1 | 130s |  |
| `cd67_MindSource_2001_demo_sadness.zip` | sadness.exe | attente ? | 0.98 | 1.02 (37s) | 0.97 | 1 | 130s |  |
| `cd82_remedy2002_demo_trial_error-by-_.zip` | trial_error.exe | attente ? | 0.98 | 1.02 (65s) | 0.97 | 1 | 130s |  |
| `cd54_norvegia99_demo_discoteque.zip` | discoteque.exe | attente ? | 0.99 | 1.02 (21s) | 0.96 | 1 | 129s |  |
| `cd64_Lobotomia_Demos_ffwd_by_moppi.zip` | pajaplayer.exe | attente ? | 0.98 | 1.02 (14s) | 0.96 | 1 | 130s |  |
| `cd53_dreamhack99_demo_no-accel_rzr-mind.zip` | mindstam.exe | attente ? | 0.98 | 1.02 (17s) | 0.95 | 1 | 130s |  |
| `cd53_dreamhack99_in64_blocc_final.zip` | intro.exe | attente ? | 0.98 | 1.02 (5s) | 0.95 | 1 | 130s |  |
| `cd54_norvegia99_fastintro_lada.zip` | lada.exe | attente ? | 0.98 | 1.02 (47s) | 0.95 | 1 | 129s |  |
| `cd56_ELEVATOR2000_demo_esko.zip` | uusmedia.exe | attente ? | 0.98 | 1.02 (20s) | 0.95 | 1 | 130s |  |
| `cd71_MekkaSymposium_Demos_232.ZIP` | 232.exe | attente ? | 0.98 | 1.02 (37s) | 0.95 | 1 | 130s |  |
| `cd59_Mekka_symposium_2000_in64_genesis.zip` | genesis.exe | attente ? | 0.98 | 1.02 (54s) | 0.94 | 1 | 130s |  |
| `cd53_dreamhack99_demo_no-accel_smokesom.zip` | dh_demo.exe | attente ? | 0.97 | 1.02 (38s) | 0.94 | 1 | 130s |  |
| `cd60_remedy00_demo_flt_vodka.zip` | fltvodka.exe | attente ? | 0.98 | 1.02 (18s) | 0.93 | 1 | 130s |  |
| `cd74_demos_TRSAC_2001_04_blakn.zip` | 04_blakn/blakn.exe | attente ? | 0.97 | 1.02 (22s) | 0.93 | 1 | 130s |  |
| `cd67_MindSource_2001_demo_aaeop.zip` | powah.exe | attente ? | 0.97 | 1.02 (50s) | 0.91 | 1 | 130s |  |

… et 15 autres (`--top` pour en voir plus).

