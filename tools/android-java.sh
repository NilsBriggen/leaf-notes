#!/bin/sh
# Adds the small Java part (android/java: file picker, sharing, key store) to an APK built by
# cargo-apk, then signs it again. Run after every `cargo apk build`:
#   sh tools/android-java.sh target/release/apk/LeafNotes.apk
set -e
APK=${1:-target/release/apk/LeafNotes.apk}
SDK=${ANDROID_HOME:?set ANDROID_HOME}
BT=$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)
JAR="$SDK/platforms/android-34/android.jar"
OUT=target/android-java
rm -rf "$OUT" && mkdir -p "$OUT/classes"
javac -nowarn -Xlint:-options -source 8 -target 8 -bootclasspath "$JAR" -d "$OUT/classes" $(find android/java -name '*.java')
"$BT/d8" --release --min-api 26 --lib "$JAR" --output "$OUT" $(find "$OUT/classes" -name '*.class')
cp "$APK" "$OUT/app.apk"
(cd "$OUT" && zip -q app.apk classes.dex)
"$BT/zipalign" -f -p 4 "$OUT/app.apk" "$OUT/aligned.apk"
"$BT/apksigner" sign --ks android/dev.keystore --ks-pass pass:android --out "$APK" "$OUT/aligned.apk"
echo "Java part added: $APK"
