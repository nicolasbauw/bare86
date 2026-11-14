// SPDX-License-Identifier: GPL-3.0-or-later
// Allocation de la puce Nuked OPL2 (third_party/libadlmidi) pour Rust, qui
// ne connaît pas la taille de opl2_chip.
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
