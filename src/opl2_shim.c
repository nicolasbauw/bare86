// SPDX-License-Identifier: GPL-3.0-or-later
// Allocation des puces Nuked OPL2 et CQM (third_party/libadlmidi) pour Rust, qui
// ne connaît pas la taille de leurs structures.
#include <stdlib.h>
#include "chips/nuked/nukedopl2.h"

opl2_chip *bare86_opl2_new(uint32_t samplerate) {
    opl2_chip *chip = calloc(1, sizeof(opl2_chip));
    if (chip) {
        OPL2_Reset(chip, samplerate);
    }
    return chip;
}

void bare86_opl2_free(opl2_chip *chip) {
    free(chip);
}

// Synthèse FM « CQM » de Creative (SB16 Vibra, AWE64), Nuked CQM.
#include "chips/nuked_cqm/cqm.h"

cqm_t *bare86_cqm_new(uint32_t samplerate) {
    cqm_t *chip = calloc(1, sizeof(cqm_t));
    if (chip) {
        CQM_Reset(chip, samplerate, 49716);
    }
    return chip;
}

void bare86_cqm_free(cqm_t *chip) {
    free(chip);
}
