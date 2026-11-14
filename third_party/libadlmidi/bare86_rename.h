/* Bare86 : renomme les fonctions de la copie de Nuked OPL3 embarquée par
 * libADLMIDI, pour ne pas les confondre avec third_party/nuked-opl3 (OPL3
 * de la carte son). Inclus de force (-include) à la compilation. */
#define OPL3_Generate ADL_OPL3_Generate
#define OPL3_Generate4Ch ADL_OPL3_Generate4Ch
#define OPL3_Generate4ChResampled ADL_OPL3_Generate4ChResampled
#define OPL3_Generate4ChStream ADL_OPL3_Generate4ChStream
#define OPL3_GenerateResampled ADL_OPL3_GenerateResampled
#define OPL3_GenerateStream ADL_OPL3_GenerateStream
#define OPL3_Reset ADL_OPL3_Reset
#define OPL3_WritePan ADL_OPL3_WritePan
#define OPL3_WriteReg ADL_OPL3_WriteReg
#define OPL3_WriteRegBuffered ADL_OPL3_WriteRegBuffered
