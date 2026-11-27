/* cible : win32 (Open Watcom, wcl386 -bt=nt -l=nt), programme de tests/hx_version.rs ; ressource : version.rc
   VERSION.DLL de HX (fork de Bare86) : GetFileVersionInfoSizeA/GetFileVersionInfoA sur le programme lui-même (déjà chargé,
   comme ijl11.dll qui lit sa propre version dans sa DllMain), VerQueryValueA sur "\" (VS_FIXEDFILEINFO), une chaîne
   (rendue en ANSI, deux fois de suite), "\VarFileInfo\Translation", un chemin absent. Résultats dans VERSION.TXT. */
#include <windows.h>
#include <stdio.h>
#include <string.h>

int main(void)
{
    char name[MAX_PATH], *s1 = "", *s2 = "", *s3 = "";
    DWORD h, size, ms = 0, ok = 0, absent, tr = 0;
    UINT len = 0, len2 = 0;
    void *data, *p;
    VS_FIXEDFILEINFO *ffi;
    FILE *f;
    GetModuleFileNameA(NULL, name, sizeof name);
    size = GetFileVersionInfoSizeA(name, &h);
    data = malloc(size ? size : 1);
    if (size && GetFileVersionInfoA(name, 0, size, data)) {
        ok = 1;
        if (VerQueryValueA(data, "\\", (void **)&ffi, &len) && len >= sizeof *ffi) ms = ffi->dwFileVersionMS;
        if (VerQueryValueA(data, "\\StringFileInfo\\040904b0\\ProductVersion", &p, &len2)) s1 = p;
        if (VerQueryValueA(data, "\\StringFileInfo\\040904B0\\ProductVersion", &p, &len2)) s2 = p;
        if (VerQueryValueA(data, "\\StringFileInfo\\040904B0\\CompanyName", &p, &len2)) s3 = p;
        if (VerQueryValueA(data, "\\VarFileInfo\\Translation", &p, &len)) tr = *(DWORD *)p;
        absent = VerQueryValueA(data, "\\StringFileInfo\\040904B0\\Nothing", &p, &len);
    }
    f = fopen("VERSION.TXT", "w");
    fprintf(f, "ok=%lu size=%d ms=%08lX pv=%s/%s co=%s tr=%08lX absent=%lu\n", ok, size > 0, ms, s1, s2, s3, tr, absent);
    fclose(f);
    return 0;
}
