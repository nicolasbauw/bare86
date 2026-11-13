// SPDX-License-Identifier: GPL-3.0-or-later
// Allocation de la puce pour Rust, qui ne connaît pas la taille de opl3_chip.
#include <stdlib.h>
#include "opl3.h"

opl3_chip *bare86_opl3_new(uint32_t samplerate) {
    opl3_chip *chip = calloc(1, sizeof(opl3_chip));
    if (chip) {
        OPL3_Reset(chip, samplerate);
    }
    return chip;
}

void bare86_opl3_free(opl3_chip *chip) {
    free(chip);
}
