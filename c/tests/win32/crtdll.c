/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt), programme de tests/hx_crtdll.rs
   CRTDLL.DLL fabriquée par Bare86 à partir de MSVCRT.DLL (src/crtdll.rs) : chargée par LoadLibrary, ses noms propres
   (__GetMainArgs, _fmode_dll = _fmode, _osmajor_dll, _strncnt…) et un aller-retour de fichier binaire par ses fonctions
   (0x1A et CR LF intacts, fseek relatif, getc, comme le chargeur 3DS de level2). Résultats dans CRTDLL.TXT. */
#include <windows.h>
#include <stdio.h>
#include <string.h>

typedef void *(__cdecl *fopen_t)(const char *, const char *);
typedef int (__cdecl *fclose_t)(void *);
typedef size_t (__cdecl *fio_t)(void *, size_t, size_t, void *);
typedef int (__cdecl *fseek_t)(void *, long, int);
typedef int (__cdecl *getc_t)(void *);
typedef void (__cdecl *gma_t)(int *, char ***, char ***, int);
typedef size_t (__cdecl *strncnt_t)(const char *, size_t);
typedef char *(__cdecl *strinc_t)(const char *);
typedef unsigned (__cdecl *strnextc_t)(const char *);
typedef int (__cdecl *open_t)(const char *, int, ...);
typedef int (__cdecl *fstat_t)(int, void *);
typedef int (__cdecl *rd_t)(int, void *, unsigned);
typedef int (__cdecl *close_t)(int);

int main(void)
{
    HMODULE h = LoadLibrary("CRTDLL.DLL");
    FILE *out = fopen("CRTDLL.TXT", "w");
    int argc = 0, gma_ok = 0, alias = 0, osmajor = -1, str = 0, bin = 0, low = 0;
    char **argv = NULL, **envp = NULL;
    if (h) {
        gma_t gma = (gma_t)GetProcAddress(h, "__GetMainArgs");
        int *fm = (int *)GetProcAddress(h, "_fmode_dll"), *fm2 = (int *)GetProcAddress(h, "_fmode");
        int *osm = (int *)GetProcAddress(h, "_osmajor_dll");
        strncnt_t sn = (strncnt_t)GetProcAddress(h, "_strncnt");
        strinc_t si = (strinc_t)GetProcAddress(h, "_strinc");
        strnextc_t sx = (strnextc_t)GetProcAddress(h, "_strnextc");
        fopen_t fo = (fopen_t)GetProcAddress(h, "fopen");
        fclose_t fc = (fclose_t)GetProcAddress(h, "fclose");
        fio_t fw = (fio_t)GetProcAddress(h, "fwrite"), fr = (fio_t)GetProcAddress(h, "fread");
        fseek_t fs = (fseek_t)GetProcAddress(h, "fseek");
        getc_t gc = (getc_t)GetProcAddress(h, "getc");
        if (gma) {
            gma(&argc, &argv, &envp, 0);
            gma_ok = argc >= 1 && argv && argv[0] && envp;
        }
        alias = fm && fm == fm2;
        if (osm) osmajor = *osm;
        if (sn && si && sx) {
            static const char xy[] = "xy";
            str = (int)sn("abc", 2) * 1000 + (int)sn("ab", 9) * 100 + (si(xy) == xy + 1) * 10 + (sx("\xE9") == 0xE9);
        }
        if (fo && fc && fw && fr && fs && gc) {
            static const unsigned char data[8] = { 'M', 'M', 0x1A, '\r', '\n', 0x00, 0x7F, 'Z' };
            unsigned char back[4];
            void *f = fo("BIN.DAT", "wb");
            if (f) { fw((void *)data, 1, 8, f); fc(f); }
            f = fo("BIN.DAT", "rb");
            if (f) {
                int c0 = gc(f), c1;
                fs(f, 1, SEEK_CUR);                       /* saute 'M' */
                c1 = gc(f);                               /* 0x1A */
                bin = (c0 == 'M' && c1 == 0x1A && fr(back, 1, 4, f) == 4 && !memcmp(back, data + 3, 4) && gc(f) == 'Z' && gc(f) == EOF);
                fc(f);
            }
        }
        {   /* E/S bas niveau : _open(O_RDONLY|O_BINARY), taille par _fstat (st_size à l'octet 20), _read du tout */
            open_t op = (open_t)GetProcAddress(h, "_open");
            fstat_t st = (fstat_t)GetProcAddress(h, "_fstat");
            rd_t rd = (rd_t)GetProcAddress(h, "_read");
            close_t cl = (close_t)GetProcAddress(h, "_close");
            if (op && st && rd && cl) {
                int fd = op("BIN.DAT", 0x8000), size = -1, got = -1;
                long sbuf[16];
                unsigned char all[16];
                if (fd >= 0) {
                    memset(sbuf, 0, sizeof sbuf);
                    if (st(fd, sbuf) == 0) size = (int)sbuf[5];
                    got = rd(fd, all, sizeof all);
                    cl(fd);
                }
                low = size * 10 + got;
            }
        }
    }
    fprintf(out, "load=%d gma=%d alias=%d osmajor=%d str=%d bin=%d low=%d\n", h != NULL, gma_ok, alias, osmajor, str, bin, low);
    fclose(out);
    return 0;
}
