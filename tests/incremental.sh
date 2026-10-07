#!/usr/bin/env bash
. $(dirname $0)/common.inc
[ "$MACHINE" = x86_64 ] || skip
[ "$(uname)" = Linux ] || skip

export XDG_CACHE_HOME=$PWD/$t/cache
mkdir -p $XDG_CACHE_HOME
cat <<'EOF' | $CC -c -o $t/a.o -x assembler -
.globl _start
.text
_start:
  mov $60, %eax
  xor %edi, %edi
  syscall
.data
.globl value
value: .quad 1
EOF
cat <<'EOF' | $CC -c -o $t/b.o -x assembler -
.data
.quad value
.section .debug_large,"",@progbits
.space 12582912
EOF

link() {
  ./mold --no-fork -m elf_x86_64 --build-id=fast --perf --incremental -o $t/exe $t/a.o $t/b.o "$@"
}
link 2> $t/first.log
cp $t/exe $t/original
stat -c '%i:%s:%y:%z' $t/exe > $t/before
link 2> $t/null.log
stat -c '%i:%s:%y:%z' $t/exe > $t/after
cmp $t/before $t/after
cmp $t/original $t/exe
grep 'null link, rewritten=0' $t/null.log

python3 - "$t/a.o" <<'PY'
import struct, sys
p = sys.argv[1]
b = bytearray(open(p, 'rb').read())
o = struct.unpack_from('<Q', b, 40)[0]
n, strings = struct.unpack_from('<HH', b, 60)
st = struct.unpack_from('<Q', b, o + strings * 64 + 24)[0]
for i in range(n):
    h = o + i * 64
    name = st + struct.unpack_from('<I', b, h)[0]
    if b[name:b.index(0, name)] == b'.data':
        b[struct.unpack_from('<Q', b, h + 24)[0]] = 2
        break
else:
    raise RuntimeError('missing .data')
open(p, 'wb').write(b)
PY
link 2> $t/edit.log
grep 'incremental: MicroLink' $t/edit.log
grep -E 'build_id_shards=[1-3]/4' $t/edit.log
cp $t/exe $t/patched
link --no-incremental 2> $t/full.log
cmp $t/patched $t/exe
link --incremental-verify 2> $t/verify-first.log
link --incremental-verify 2> $t/verify.log
grep 'verified byte-for-byte' $t/verify.log

link 2> $t/corrupt-seed.log
state=$(python3 - "$XDG_CACHE_HOME/mold/incremental-v1" "$t/exe" <<'PYTHON'
import os,pathlib,struct,sys
metadata=os.stat(sys.argv[2])
for p in pathlib.Path(sys.argv[1]).glob('*.state'):
    b=p.read_bytes()
    roots=[b[o:o+4096] for o in [0,4096] if len(b)>=o+4096 and b[o:o+8]==b'MOLDINC\0']
    if not roots: continue
    root=max(roots,key=lambda r:struct.unpack_from('<Q',r,176)[0])
    if struct.unpack_from('<I',root,8)[0]==4 and struct.unpack_from('<Q',root,72)[0]==metadata.st_ino:
        seconds,nanos=struct.unpack_from('<QQ',root,104)
        if seconds*1000000000+nanos==metadata.st_ctime_ns:
            print(p)
            break
else: raise RuntimeError('missing current state')
PYTHON
)
printf corrupt > "$state"
link 2> $t/corrupt.log
grep 'full rewrite' $t/corrupt.log
cmp $t/patched $t/exe

python3 - "$t/a.o" <<'PYTHON'
import struct, sys
p = sys.argv[1]
b = bytearray(open(p, 'rb').read())
o = struct.unpack_from('<Q', b, 40)[0]
n = struct.unpack_from('<H', b, 60)[0]
for i in range(n):
    h = o + i * 64
    if struct.unpack_from('<I', b, h + 4)[0] != 2:
        continue
    start, size = struct.unpack_from('<QQ', b, h + 24)
    link = struct.unpack_from('<I', b, h + 40)[0]
    names = struct.unpack_from('<Q', b, o + link * 64 + 24)[0]
    for sym in range(start, start + size, 24):
        name = names + struct.unpack_from('<I', b, sym)[0]
        if b[name:b.index(0, name)] == b'value':
            struct.pack_into('<Q', b, sym + 8, 1)
open(p, 'wb').write(b)
PYTHON
link 2> $t/environment.log
grep 'full rewrite (semantic surfaces)' $t/environment.log
cp $t/exe $t/environment
link --no-incremental 2> $t/environment-full.log
cmp $t/environment $t/exe

(umask 077; link 2> $t/umask-first.log)
(umask 077; link 2> $t/umask-null.log)
grep 'null link' $t/umask-null.log
stat -c %a $t/exe | grep '^700$'

mkdir -p $t/early $t/late
printf '.data\n.quad 42\n' | $CC -c -o $t/lib.o -x assembler -
ar cr $t/late/libprobe.a $t/lib.o
./mold --no-fork --incremental --perf -L$t/early -L$t/late -lprobe $t/a.o -o $t/search 2> $t/search1.log
./mold --no-fork --incremental --perf -L$t/early -L$t/late -lprobe $t/a.o -o $t/search 2> $t/search2.log
grep 'null link' $t/search2.log
cp $t/late/libprobe.a $t/early/libprobe.a
./mold --no-fork --incremental --perf -L$t/early -L$t/late -lprobe $t/a.o -o $t/search 2> $t/search3.log
not grep 'null link' $t/search3.log

mkdir -p $t/early/nested $t/late/nested
printf 'V1 { global: _start; local: *; };\n' > $t/late/nested/versions.map
version_link() {
  ./mold --no-fork --incremental --perf --version-script=nested/versions.map -L$t/early -L$t/late $t/a.o -o $t/version-search
}
version_link 2> $t/version1.log
version_link 2> $t/version2.log
grep 'null link' $t/version2.log
printf 'V2 { global: _start; local: *; };\n' > $t/early/nested/versions.map
version_link 2> $t/version3.log
not grep 'null link' $t/version3.log

./mold --no-fork --incremental --perf --directory="$PWD" -m elf_x86_64 -o $t/directory-output $t/a.o 2> $t/directory.log
grep 'full rewrite (target/options/cache unavailable)' $t/directory.log

mkdir -p $t/reloc
cat <<'ASM' | $CC -c -o $t/reloc/a.o -x assembler -
.globl _start
.text
_start:
  call target
  mov $60,%eax
  xor %edi,%edi
  syscall
.section .debug_micro,"",@progbits
.quad target+1
.section .rodata.str1.1,"aMS",@progbits,1
.asciz "retained merged contribution"
ASM
printf '.globl target\n.text\ntarget: ret\n' | $CC -c -o $t/reloc/b.o -x assembler -
reloc_link() {
  ./mold --no-fork --incremental-verify --perf --build-id -o $t/reloc/exe $t/reloc/a.o $t/reloc/b.o
}
reloc_link 2> $t/reloc/seed.log
python3 - "$t/reloc/a.o" <<'PY'
import struct, sys
p=sys.argv[1]
b=bytearray(open(p,'rb').read())
o=struct.unpack_from('<Q',b,40)[0]
n=struct.unpack_from('<H',b,60)[0]
for i in range(n):
    h=o+i*64
    if struct.unpack_from('<I',b,h+4)[0]==4:
        start,size=struct.unpack_from('<QQ',b,h+24)
        for r in range(start,start+size,24):
            a=struct.unpack_from('<q',b,r+16)[0]
            if a==1: struct.pack_into('<q',b,r+16,2)
open(p,'wb').write(b)
PY
reloc_link 2> $t/reloc/edit.log
grep 'MicroLink objects=1' $t/reloc/edit.log
grep 'verified byte-for-byte' $t/reloc/edit.log
not grep 'split_contents' $t/reloc/edit.log

ar crD $t/reloc/libmember.a $t/reloc/b.o
archive_link() {
  ./mold --no-fork --incremental-verify --perf --build-id -o $t/reloc/archive $t/reloc/a.o $t/reloc/libmember.a
}
archive_link 2> $t/reloc/archive-seed.log
printf '.globl target\n.text\ntarget: nop\n' | $CC -c -o $t/reloc/b.o -x assembler -
ar rD $t/reloc/libmember.a $t/reloc/b.o
archive_link 2> $t/reloc/archive-edit.log
grep 'MicroLink objects=1' $t/reloc/archive-edit.log
grep 'verified byte-for-byte' $t/reloc/archive-edit.log

printf 'irrelevant' > $t/early/libunrelated.so
./mold --no-fork --incremental --perf -L$t/early -L$t/late -lprobe $t/a.o -o $t/search 2> $t/search4.log
./mold --no-fork --incremental --perf -L$t/early -L$t/late -lprobe $t/a.o -o $t/search 2> $t/search5.log
grep 'null link' $t/search5.log
printf 'another irrelevant file' > $t/early/libalso_unrelated.so
./mold --no-fork --incremental --perf -L$t/early -L$t/late -lprobe $t/a.o -o $t/search 2> $t/search6.log
grep 'null link' $t/search6.log

printf '%s\n' "$t/reloc/a.o" "$t/reloc/b.o" '-o' "$t/response-output" > $t/command.rsp
./mold --no-fork --incremental --perf @$t/command.rsp 2> $t/response1.log
cp $t/command.rsp $t/new-command.rsp
mv $t/new-command.rsp $t/command.rsp
./mold --no-fork --incremental --perf @$t/command.rsp 2> $t/response2.log
grep 'null link' $t/response2.log

python3 - "$t/reloc/a.o" "$t/reloc/b.o" "$t" <<'PY'
import fcntl, os, pathlib, struct, subprocess, sys, time
base=['./mold','--no-fork','--incremental',sys.argv[1],sys.argv[2]]
outputs=[str(pathlib.Path(sys.argv[3])/name) for name in ['concurrent-a','concurrent-b']]
for output in outputs: subprocess.run(base+['-o',output],check=True)
inode=os.stat(outputs[0]).st_ino
state=next(p for p in pathlib.Path(os.environ['XDG_CACHE_HOME']).rglob('*.state') if p.stat().st_size>=176 and struct.unpack_from('<Q',p.read_bytes(),72)[0]==inode)
with state.with_suffix('.keylock').open('r+b') as lock:
    fcntl.flock(lock,fcntl.LOCK_EX)
    blocked=subprocess.Popen(base+['-o',outputs[0]])
    try:
        time.sleep(.05)
        assert blocked.poll() is None
        subprocess.run(base+['-o',outputs[1]],check=True,timeout=3)
    finally:
        fcntl.flock(lock,fcntl.LOCK_UN)
        assert blocked.wait(timeout=3)==0
PY

mkdir -p $t/cells
cat <<'ASM' | $CC -c -o $t/cells/a.o -x assembler -
.globl _start
.text
_start:
  mov value@GOTPCREL(%rip),%rax
  mov $tls_value@TPOFF,%rax
  call target
  lea readonly_value(%rip),%rax
  .long value@GOTPCREL
  ret
.data
.globl value
value: .quad 1
.section .tdata,"awT",@progbits
.globl tls_value
.type tls_value,@tls_object
tls_value: .long 0
.section .rodata.fold,"a",@progbits
.globl readonly_value
readonly_value: .quad 1
ASM
cells_link() {
  ./mold --no-fork --incremental-verify --perf --build-id -o $t/cells/exe $t/cells/a.o $t/reloc/b.o
}
cells_link 2> $t/cells/seed.log
python3 - "$t/cells/a.o" <<'PY'
import struct,sys
p=sys.argv[1];b=bytearray(open(p,'rb').read());o=struct.unpack_from('<Q',b,40)[0];n,strings=struct.unpack_from('<HH',b,60);names=struct.unpack_from('<Q',b,o+strings*64+24)[0]
for i in range(n):
    h=o+i*64;name=names+struct.unpack_from('<I',b,h)[0]
    if b[name:b.index(0,name)]==b'.text':
        start,size=struct.unpack_from('<QQ',b,h+24);b[start+size-1]^=1
open(p,'wb').write(b)
PY
cells_link 2> $t/cells/edit.log
grep 'MicroLink objects=1' $t/cells/edit.log
grep 'verified byte-for-byte' $t/cells/edit.log

ar crsDT $t/reloc/libthin.a $t/reloc/b.o
thin_link() {
  ./mold --no-fork --incremental-verify --perf --build-id -o $t/reloc/thin $t/reloc/a.o $t/reloc/libthin.a
}
thin_link 2> $t/reloc/thin-seed.log
printf '.globl target\n.text\ntarget: ret\n' | $CC -c -o $t/reloc/b.o -x assembler -
ar rDT $t/reloc/libthin.a $t/reloc/b.o
thin_link 2> $t/reloc/thin-edit.log
grep 'MicroLink objects=1' $t/reloc/thin-edit.log
grep 'verified byte-for-byte' $t/reloc/thin-edit.log

./mold --no-fork --incremental-verify --perf --icf=all --gc-sections --ignore-data-address-equality --build-id -o $t/cells/icf $t/cells/a.o $t/reloc/b.o 2> $t/cells/icf-seed.log
python3 - "$t/cells/a.o" <<'PY'
import struct,sys
p=sys.argv[1];b=bytearray(open(p,'rb').read());o=struct.unpack_from('<Q',b,40)[0];n,strings=struct.unpack_from('<HH',b,60);names=struct.unpack_from('<Q',b,o+strings*64+24)[0]
for i in range(n):
    h=o+i*64;name=names+struct.unpack_from('<I',b,h)[0]
    if b[name:b.index(0,name)]==b'.data':b[struct.unpack_from('<Q',b,h+24)[0]]^=1
open(p,'wb').write(b)
PY
./mold --no-fork --incremental-verify --perf --icf=all --gc-sections --ignore-data-address-equality --build-id -o $t/cells/icf $t/cells/a.o $t/reloc/b.o 2> $t/cells/icf-edit.log
grep 'MicroLink objects=1' $t/cells/icf-edit.log
grep 'verified byte-for-byte' $t/cells/icf-edit.log

python3 - "$t/cells/a.o" <<'PYTHON'
import struct,sys
p=sys.argv[1];b=bytearray(open(p,'rb').read());o=struct.unpack_from('<Q',b,40)[0];n,strings=struct.unpack_from('<HH',b,60);names=struct.unpack_from('<Q',b,o+strings*64+24)[0]
for i in range(n):
    h=o+i*64;name=names+struct.unpack_from('<I',b,h)[0]
    if b[name:b.index(0,name)]==b'.rodata.fold':b[struct.unpack_from('<Q',b,h+24)[0]]^=1
open(p,'wb').write(b)
PYTHON
./mold --no-fork --incremental-verify --perf --icf=all --gc-sections --ignore-data-address-equality --build-id -o $t/cells/icf $t/cells/a.o $t/reloc/b.o 2> $t/cells/icf-change.log
grep 'full rewrite (semantic surfaces)' $t/cells/icf-change.log

./mold --no-fork --incremental-verify --perf --allow-multiple-definition --build-id -o $t/reloc/duplicate $t/reloc/a.o $t/reloc/b.o $t/reloc/b.o 2> $t/reloc/duplicate-seed.log
printf '.globl target\n.text\ntarget: nop\n' | $CC -c -o $t/reloc/b.o -x assembler -
./mold --no-fork --incremental-verify --perf --allow-multiple-definition --build-id -o $t/reloc/duplicate $t/reloc/a.o $t/reloc/b.o $t/reloc/b.o 2> $t/reloc/duplicate-edit.log
grep 'full rewrite (object admission)' $t/reloc/duplicate-edit.log
./mold --no-fork --no-incremental --allow-multiple-definition --build-id -o $t/reloc/duplicate-full $t/reloc/a.o $t/reloc/b.o $t/reloc/b.o
cmp $t/reloc/duplicate $t/reloc/duplicate-full

mkdir -p $t/selective
cat <<'ASM' | $CC -c -o $t/selective/a.o -x assembler -
.globl _start
.text
_start:
  lea one(%rip), %rax
  lea two(%rip), %rdx
  mov $60, %eax
  xor %edi, %edi
  syscall
ASM
printf '.globl one,two\n.data\none: .long 1\ntwo: .long 2\n' | $CC -c -o $t/selective/b.o -x assembler -
selective_link() {
  ./mold --no-fork --incremental-verify --perf --build-id -o $t/selective/exe $t/selective/a.o $t/selective/b.o
}
selective_link 2> $t/selective/seed.log
python3 - "$t/selective/a.o" <<'PYTHON'
import struct,sys
p=sys.argv[1];b=bytearray(open(p,'rb').read());o=struct.unpack_from('<Q',b,40)[0];n=struct.unpack_from('<H',b,60)[0]
for i in range(n):
    h=o+i*64
    if struct.unpack_from('<I',b,h+4)[0]==4:
        r=struct.unpack_from('<Q',b,h+24)[0]
        first=struct.unpack_from('<Q',b,r+8)[0]
        second=struct.unpack_from('<Q',b,r+32)[0]
        struct.pack_into('<Q',b,r+8,second)
        struct.pack_into('<Q',b,r+32,first)
open(p,'wb').write(b)
PYTHON
selective_link 2> $t/selective/edit.log
grep 'SelectiveRelink objects=1' $t/selective/edit.log
grep 'semantic_closure objects=1 sections=1 generated_chunks=0' $t/selective/edit.log
grep 'verified byte-for-byte' $t/selective/edit.log
selective_link 2> $t/selective/repeat.log
grep 'MicroLink objects=0' $t/selective/repeat.log

cat <<'ASM' | $CC -c -o $t/selective/merge-a.o -x assembler -
.globl _start
.text
_start: mov $60,%eax; xor %edi,%edi; syscall
.section .debug_str,"MS",@progbits,1
.asciz "foo","bar"
ASM
printf '.section .debug_str,"MS",@progbits,1\n.asciz "foo","bar","baz"\n' | $CC -c -o $t/selective/merge-b.o -x assembler -
merge_link() {
  ./mold --no-fork --incremental-verify --perf --build-id -o $t/selective/merge $t/selective/merge-a.o $t/selective/merge-b.o
}
merge_link 2> $t/selective/merge-seed.log
python3 - "$t/selective/merge-a.o" <<'PYTHON'
import sys
p=sys.argv[1];b=open(p,'rb').read();assert b.count(b'foo\0bar\0')==1
open(p,'wb').write(b.replace(b'foo\0bar\0',b'bar\0baz\0'))
PYTHON
merge_link 2> $t/selective/merge-edit.log
grep 'SelectiveRelink objects=1' $t/selective/merge-edit.log
grep 'merge contribution_changes=2 merged_bytes_written=0' $t/selective/merge-edit.log
grep 'verified byte-for-byte' $t/selective/merge-edit.log
python3 - "$t/selective/merge-a.o" <<'PYTHON'
import sys
p=sys.argv[1];b=open(p,'rb').read();open(p,'wb').write(b.replace(b'bar\0baz\0',b'new\0baz\0'))
PYTHON
merge_link 2> $t/selective/merge-new.log
grep 'full rewrite (merge fragment proof)' $t/selective/merge-new.log

printf '.globl imported\n.type imported,@function\n.text\nimported: ret\n' | $CC -c -o $t/selective/dso.o -x assembler -
./mold --no-fork --shared --no-incremental -o $t/selective/libimport.so $t/selective/dso.o
cat <<'ASM' | $CC -c -o $t/selective/import.o -x assembler -
.globl _start
.section .text.a,"ax"
_start: lea one(%rip),%rax; mov $60,%eax; xor %edi,%edi; syscall
.section .text.b,"ax"
call imported
ASM
import_link() {
  ./mold --no-fork --incremental-verify --perf --build-id -o $t/selective/import $t/selective/import.o $t/selective/b.o $t/selective/libimport.so
}
import_link 2> $t/selective/import-seed.log
python3 - "$t/selective/import.o" <<'PYTHON'
import struct,sys
p=sys.argv[1];b=bytearray(open(p,'rb').read());o=struct.unpack_from('<Q',b,40)[0];n=struct.unpack_from('<H',b,60)[0];rels=[]
for i in range(n):
    h=o+i*64
    if struct.unpack_from('<I',b,h+4)[0]==4:rels.append(struct.unpack_from('<Q',b,h+24)[0])
assert len(rels)==2
old=struct.unpack_from('<Q',b,rels[0]+8)[0];target=struct.unpack_from('<Q',b,rels[1]+8)[0]
struct.pack_into('<Q',b,rels[0]+8,(target&0xffffffff00000000)|(old&0xffffffff))
open(p,'wb').write(b)
PYTHON
import_link 2> $t/selective/import-edit.log
grep 'full rewrite (selective relocation requirement)' $t/selective/import-edit.log
./mold --no-fork --no-incremental --build-id -o $t/selective/import-full $t/selective/import.o $t/selective/b.o $t/selective/libimport.so
cmp $t/selective/import $t/selective/import-full
