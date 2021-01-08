#! /bin/bash

#cat > POSCAR
cp "$BBM_TPL_DIR/INCAR" .
cp "$BBM_TPL_DIR/POTCAR" .
cp "$BBM_TPL_DIR/KPOINTS" .

echo RMM:  15    -0.850492174942E+02   -0.22961E-01   -0.85225E-03   773   0.221E-01    0.320E+00
echo RMM:  16    -0.850454085680E+02    0.38089E-02   -0.27753E-03   735   0.157E-01
echo FORCES:
echo "     0.2014413     0.2165960    -0.1884948"
echo "    -0.1832312     0.2056558     0.2151024"
echo "  1 F= -.85045409E+02 E0= -.85044063E+02  d E =-.850454E+02  mag=     2.2094"
echo "POSITIONS: reading from stdin"

read -r xx
while read -r xx; do
    if [[ -f STOPCAR ]]; then
        echo "found STOPCAR"
        exit 1
    fi

    echo RMM:  15    -0.850492174942E+02   -0.22961E-01   -0.85225E-03   773   0.221E-01    0.320E+00
    echo RMM:  16    -0.850454085680E+02    0.38089E-02   -0.27753E-03   735   0.157E-01
    echo FORCES:
    echo "     0.2014413     0.2165960    -0.1884948"
    echo "    -0.1832312     0.2056558     0.2151024"
    echo "  1 F= -.85045409E+02 E0= -.85044063E+02  d E =-.850454E+02  mag=     2.2094"
    echo "POSITIONS: reading from stdin"

