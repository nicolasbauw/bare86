/* SPDX-License-Identifier: GPL-3.0-or-later */
/* cible : com */
/* Critère de l'étape 2 : programme Open Watcom en mode texte.
 * printf (DOS), int86 vers INT 10h (BIOS), getch (clavier). */
#include <stdio.h>
#include <conio.h>
#include <i86.h>

static void video(unsigned ax, unsigned bx, unsigned cx, unsigned dx)
{
    union REGS r;

    r.w.ax = ax;
    r.w.bx = bx;
    r.w.cx = cx;
    r.w.dx = dx;
    int86(0x10, &r, &r);
}

int main(void)
{
    const char *s = "Open Watcom en mode texte";
    int c, i;

    video(0x0003, 0, 0, 0);                 /* mode 3 */
    for (i = 0; s[i]; i++) {
        video(0x0200, 0, 0, 0x0409 + i);    /* ligne 5, colonne 10+i */
        video(0x0900 | (unsigned char)s[i], 0x001E, 1, 0);
    }
    video(0x0200, 0, 0, 0x0609);
    printf("Touche ? ");
    c = getch();
    printf("%c (%d)\n", c, c);
    return c == 'k' ? 0 : 1;
}
