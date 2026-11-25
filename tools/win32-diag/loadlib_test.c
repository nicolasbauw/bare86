typedef unsigned int u32;
static void putc_(char c){ __asm__ volatile("int $0x21"::"a"(0x0200),"d"((u32)c):"memory"); }
static void puthex(u32 v){ for(int i=28;i>=0;i-=4){ int d=(v>>i)&15; putc_(d<10?'0'+d:'A'+d-10);} }
static void puts_(const char*s){ while(*s) putc_(*s++); }
void *__stdcall LoadLibraryExA(const char *name, void *f, u32 flags);
static void try_alloc(u32 base,u32 size){
    u32 eax=0x0504, ebx=base, ecx=size, edx=1, esi, cf;
    __asm__ volatile("int $0x31\n\tsetc %%dl\n\tmovzbl %%dl,%0" : "=r"(cf), "+a"(eax), "+b"(ebx), "+c"(ecx), "=S"(esi) : "d"(edx) : "memory","cc");
    puts_("req "); puthex(base); puts_(" -> CF="); puthex(cf); puts_(" AX="); puthex(eax&0xFFFF); puts_("\r\n");
}
int __stdcall mainCRTStartup(void){
    try_alloc(0x10000000,0x1000);
    try_alloc(0x30000000,0x1000);
    void *h = LoadLibraryExA("opengl32.dll",0,1); puts_("opengl32 -> "); puthex((u32)h); puts_("\r\n");
    return 0;
}
