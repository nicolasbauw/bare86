	.386
	.MODEL FLAT, c
	option casemap:none
	.CODE
;--- trace output to DOS stderr (replaces int 41h ax=2 which nobody handles here)
__dbgstr_esi proc public
	push eax
	push ebx
	push ecx
	push edx
	mov edx, esi
	xor ecx, ecx
@@:
	cmp byte ptr [esi+ecx], 0
	jz @F
	inc ecx
	jmp @B
@@:
	mov ebx, 2
	mov ah, 40h
	int 21h
	pop edx
	pop ecx
	pop ebx
	pop eax
	ret
__dbgstr_esi endp


	end
