#!/bin/sh
# usage: arun.sh <name> <file on the device or ""> [key=value ...]   one run of KOReader on the virtual device
D=${ANDROID_SERIAL:-emulator-5580}; here=${EBK_ANDROID_OUT:-.}; n=$1; f=$2; shift 2
adb -s $D shell am force-stop org.koreader.launcher
{ echo "name=$n"; for kv; do echo "$kv"; done; } > $here/ebk-test.conf
adb -s $D push $here/ebk-test.conf /sdcard/koreader/ebk-test.conf > /dev/null
adb -s $D shell rm -f /sdcard/koreader/ebk-test-$n.txt
adb -s $D logcat -c
start() {
    if [ -n "$f" ]; then adb -s $D shell am start -W -n org.koreader.launcher/.MainActivity -a android.intent.action.VIEW -d "file://$f" > /dev/null
    else adb -s $D shell am start -W -n org.koreader.launcher/.MainActivity > /dev/null; fi
}
# KOReader sometimes leaves at once when it is started right after it was stopped: start it again then
for try in 1 2 3; do
    start; sleep 6
    [ -n "$(adb -s $D shell pidof org.koreader.launcher)" ] && break
    echo "$n: KOReader left at once, starting again"
done
i=0; until adb -s $D shell test -f /sdcard/koreader/ebk-test-$n.txt 2>/dev/null; do sleep 3; i=$((i+1)); [ $i -gt 80 ] && { echo "$n: no report after 240 s"; break; }; done
sleep 2
mkdir -p $here/out; adb -s $D shell "cat /sdcard/koreader/ebk-test-$n.txt" > $here/out/$n.txt 2>/dev/null
for p in $(adb -s $D shell "ls /sdcard/koreader/ | grep '^ebk-test-$n\..*png$'" | tr -d '\r'); do adb -s $D pull /sdcard/koreader/$p $here/out/ > /dev/null; done
adb -s $D logcat -d | grep -i -E "KOReader|luajit" | grep -v "pmsg0" | grep -i -E "EBK|error|dlopen|denied|crash|Fatal" | cut -c1-260 | tail -8 > $here/out/$n.logcat
echo "== $n"; cat $here/out/$n.txt; cat $here/out/$n.logcat
