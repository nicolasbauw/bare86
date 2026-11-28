/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt), programme de tests/hx_muldiv.rs
   MulDiv de DKRNL32 (fork de HX de Bare86) : signé, arrondi au plus proche (moitiés loin de zéro), -1 si le diviseur est
   nul ou si le résultat ne tient pas sur 32 bits, sans exception (celui de HX 2.23 faisait mul/div non signés : erreur de
   division, démo plastorama2000). Résultats dans MULDIV.TXT. */
#include <windows.h>
#include <stdio.h>

int main(void)
{
    FILE *f = fopen("MULDIV.TXT", "w");
    fprintf(f, "%d %d %d %d %d %d %d %d %d\n",
            MulDiv(5, 1, 2), MulDiv(-5, 1, 2), MulDiv(1, 1, 0), MulDiv(0x7FFFFFFF, 2, 1), MulDiv(-7, 3, 2),
            MulDiv(300000, 200000, 100000), MulDiv(10, 10, -3), MulDiv(2, 0x40000000, 1), MulDiv(-2, 0x40000000, 1));
    fclose(f);
    return 0;
}
