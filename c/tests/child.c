/* SPDX-License-Identifier: GPL-3.0-or-later */
/* Programme enfant lancé par files.exe (INT 21h AH=4Bh) : code de sortie 7. */
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv)
{
    printf("enfant : %s\n", argc > 1 ? argv[1] : "?");
    return argc > 1 && strcmp(argv[1], "salut") == 0 ? 7 : 1;
}
