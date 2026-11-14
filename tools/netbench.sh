#!/bin/zsh
# SPDX-License-Identifier: GPL-3.0-or-later
# Débit du réseau émulé : HTGET (mTCP, sous FreeDOS) d'un fichier servi par
# un serveur HTTP de l'hôte (127.0.0.1:8000, vu de l'invité en 10.0.2.2),
# chronométré d'après la trace BARE86_NETLOG (première → dernière trame de
# données reçue par l'invité).
#
# Usage : tools/netbench.sh IMAGE TAILLE_Mo [PASSES] [SORTIE]
#   IMAGE  : FreeDOS avec C:\NET\NETUP.BAT (pilote + DHCP) et C:\NET\GET.BAT
#            (HTGET -quiet -o %1 http://10.0.2.2:8000/%2, puis « echo FIN-GET »)
#   SORTIE : fichier DOS de destination (NUL par défaut)
# NETBENCH_KEEP=RÉP garde la trace de chaque passe (RÉP/trace.N).
set -e
IMG=${1:?image}
MB=${2:?taille en Mo}
N=${3:-3}
OUT=${4:-NUL}
# BARE86 : commande de lancement (ex. « taskset -c 0-3 target/release/bare86 »).
B=${BARE86:-$(dirname $0)/../target/release/bare86}
W=$(mktemp -d)
trap 'kill $SRV 2>/dev/null; rm -rf $W' EXIT
mkdir $W/www
head -c $((MB * 1048576)) /dev/urandom > $W/www/f.bin
python3 -m http.server 8000 --bind 127.0.0.1 --directory $W/www >/dev/null 2>&1 &
SRV=$!
sleep 0.5
for i in $(seq $N); do
  cp $IMG $W/t.img
  K='{until:C:\>}C:\NET\NETUP{enter}{until:Settings written}{wait:300}C:\NET\GET '"$OUT"' f.bin{enter}{until:FIN-GET}'
  BARE86_NETLOG=1 timeout 300 ${=B} --hda $W/t.img --boot --net --keys "$K" --wav /dev/null 2>$W/log >/dev/null || true
  awk '$2=="->G" && $6!="len=0" {if(!a)a=$1; b=$1; n+=substr($6,5)} END {if (b>a) printf "passe : %d octets en %.0f ms, %.0f Ko/s\n", n, (b-a)/1000, n/1024/((b-a)/1e6); else print "passe : échec (aucune donnée)"}' $W/log
  [ -n "$NETBENCH_KEEP" ] && cp $W/log $NETBENCH_KEEP/trace.$i
done
