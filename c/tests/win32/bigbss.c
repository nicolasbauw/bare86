/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt)
   Programme de tests/hx_bigbss.rs : 32 Mo de données non initialisées (BSS) dans l'image, plus 40 Mo de tas, soit 72 Mo
   (comme madeby.exe : 32 Mo de BSS puis des images JPEG décodées). Avec 64 Mo de mémoire, HX/HDPMI finissait en triple
   faute ; le programme écrit BIG.TXT s'il a pu toucher toute la mémoire. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned char big[32 * 1024 * 1024];

int main(void)
{
    unsigned long sum = 0;
    size_t i, heap = 40 * 1024 * 1024;
    unsigned char *p = malloc(heap);
    FILE *f;
    for (i = 0; i < sizeof big; i += 4096) big[i] = (unsigned char)(i >> 12);
    for (i = 0; i < sizeof big; i += 4096) sum += big[i];
    if (p)
        for (i = 0; i < heap; i += 4096) p[i] = 1, sum += p[i];
    f = fopen("BIG.TXT", "w");
    if (f) {
        fprintf(f, "%s %lu\n", p ? "big ok" : "no heap", sum);
        fclose(f);
    }
    return 0;
}
