#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
VER0 {
  *missing1*suffix1*;
  *missing2*suffix2*;
  *missing3*suffix3*;
  *missing4*suffix4*;
  *missing5*suffix5*;
  *missing6*suffix6*;
  *missing7*suffix7*;
  *missing8*suffix8*;
  *missing9*suffix9*;
  *missing10*suffix10*;
  *missing11*suffix11*;
  *missing12*suffix12*;
  *missing13*suffix13*;
  *missing14*suffix14*;
  *missing15*suffix15*;
  *missing16*suffix16*;
  *missing17*suffix17*;
  *missing18*suffix18*;
  *missing19*suffix19*;
  *missing20*suffix20*;
  *missing21*suffix21*;
  *missing22*suffix22*;
  *missing23*suffix23*;
  *missing24*suffix24*;
  *missing25*suffix25*;
  *missing26*suffix26*;
  *missing27*suffix27*;
  *missing28*suffix28*;
  *missing29*suffix29*;
  *missing30*suffix30*;
  *missing31*suffix31*;
  *missing32*suffix32*;
  *missing33*suffix33*;
  *missing34*suffix34*;
  *missing35*suffix35*;
  *missing36*suffix36*;
  *missing37*suffix37*;
  *missing38*suffix38*;
  *missing39*suffix39*;
  *missing40*suffix40*;
  *missing41*suffix41*;
  *missing42*suffix42*;
  *missing43*suffix43*;
  *missing44*suffix44*;
  *missing45*suffix45*;
  *missing46*suffix46*;
  *missing47*suffix47*;
  *missing48*suffix48*;
  *missing49*suffix49*;
  *missing50*suffix50*;
  *missing51*suffix51*;
  *missing52*suffix52*;
  *missing53*suffix53*;
  *missing54*suffix54*;
  *missing55*suffix55*;
  *missing56*suffix56*;
  *missing57*suffix57*;
  *missing58*suffix58*;
  *missing59*suffix59*;
  *missing60*suffix60*;
  *missing61*suffix61*;
  *missing62*suffix62*;
  *missing63*suffix63*;
  *missing64*suffix64*;
};
VER1 { *alpha*omega*; };
VER2 { *alpha*middle*; };
VER3 { *common*one*; };
VER4 { *common*two*; };
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
void xalpha_middle_omega() {}
void xcommon_one_two() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o
readelf -W --dyn-syms $t/c.so > $t/log
grep -F 'xalpha_middle_omega@@VER2' $t/log
grep -F 'xcommon_one_two@@VER4' $t/log
